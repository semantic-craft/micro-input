import Combine
import Foundation

@MainActor
final class DSHSessionController: ObservableObject {
    @Published private(set) var enabled = false
    @Published private(set) var state: DSHConnectionState = .disabled
    @Published private(set) var rows: [DSHSessionRow] = []
    @Published private(set) var selected: String?
    @Published private(set) var unavailableSessionIds: [String] = []
    @Published private(set) var lastChanges: [DSHSessionChange] = []
    @Published private(set) var cursor: UInt64 = 0
    @Published private(set) var lastResult: DSHOpenResult?
    @Published private(set) var lastCommandResult: DSHCommandResult?
    @Published private(set) var atomicTurnCheckSupported = true
    @Published private(set) var pendingUnknownCommand: DSHQueuedCommand?
    @Published private(set) var voiceReplyEnabled: Bool = false

    func setVoiceReply(enabled: Bool) async -> (Bool, String) {
        voiceReplyEnabled = enabled
        return (true, enabled ? "已开启 DSH 语音回复服务" : "已关闭 DSH 语音回复服务")
    }

    #if DEBUG
    func setSnapshotForTesting(rows: [DSHSessionRow], selected: String?) {
        self.rows = rows
        self.selected = selected
    }
    #endif
    private let client: any DSHSessionServing
    private let pollNanoseconds: UInt64
    private var snapshot: DSHSessionSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var nextRequestSequence: UInt64 = 0
    private var openingEpoch: UInt64?
    private var refreshingEpoch: UInt64?
    private var suspended = false
    private var connecting: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var closing: Task<Void, Never>?

    init(client: any DSHSessionServing = DSHSessionClient(), pollNanoseconds: UInt64 = 2_000_000_000) {
        self.client = client
        self.pollNanoseconds = pollNanoseconds
    }
    func setAtomicTurnCheckSupported(_ supported: Bool) {
        atomicTurnCheckSupported = supported
    }
    func setEnabled(_ value: Bool) {
        enabled = value
        invalidate()
        if value && !suspended { start() }
    }
    func suspend() { suspended = true; invalidate() }
    func resume() { suspended = false; if enabled { start() } }
    func shutdown() async { enabled = false; invalidate(); await closing?.value }
    private func invalidate() {
        epoch &+= 1
        sequence &+= 1
        openingEpoch = nil; refreshingEpoch = nil
        let pendingConnection = connecting
        pendingConnection?.cancel(); connecting = nil
        polling?.cancel(); polling = nil
        snapshot = nil; rows = []; selected = nil; lastResult = nil; lastCommandResult = nil
        pendingUnknownCommand = nil
        unavailableSessionIds = []; lastChanges = []; cursor = 0
        state = .disabled
        let previous = closing
        closing = Task { [client] in
            await previous?.value
            await pendingConnection?.value
            await client.close()
        }
    }
    private func start() {
        let expected = epoch, previous = closing
        state = .connecting
        connecting = Task { [weak self, client] in
            await previous?.value
            guard let self, self.current(expected) else { return }
            do {
                let result = try await client.connect()
                guard self.current(expected) else { return }
                try self.accept(result)
                self.polling = Task { [weak self] in
                    while !Task.isCancelled {
                        do { try await Task.sleep(nanoseconds: self?.pollNanoseconds ?? 2_000_000_000) }
                        catch { return }
                        guard let self, self.current(expected) else { return }
                        await self.refresh()
                    }
                }
            } catch { await self.fail(error, epoch: expected) }
        }
    }
    private func current(_ expected: UInt64) -> Bool {
        epoch == expected && enabled && !suspended && !Task.isCancelled
    }
    private func accept(_ value: DSHSessionSnapshot) throws {
        try value.validate(after: snapshot?.cursor)
        if let snapshot {
            guard snapshot.hostId == value.hostId, snapshot.generation == value.generation,
                  value.sessionIds.count >= snapshot.sessionIds.count,
                  Array(value.sessionIds.prefix(snapshot.sessionIds.count)) == snapshot.sessionIds else { throw DSHFailure.stale }
        }
        if let explicitSelected = value.selected {
            selected = explicitSelected
        } else if let currentSelected = selected, !value.rows.contains(where: { $0.sessionId == currentSelected }) {
            selected = nil
        }
        snapshot = value; rows = value.rows; state = .ready
        unavailableSessionIds = value.unavailableSessionIds; lastChanges = value.changes; cursor = value.cursor
    }
    private func fail(_ error: Error, epoch expected: UInt64) async {
        guard current(expected) else { return }
        rows = []; selected = nil; snapshot = nil; lastResult = nil; lastCommandResult = nil
        pendingUnknownCommand = nil
        unavailableSessionIds = []; lastChanges = []; cursor = 0
        state = .unavailable(error as? DSHFailure ?? .unavailable)
        polling?.cancel(); polling = nil
        let previous = closing
        let close = Task { [client] in await previous?.value; await client.close() }
        closing = close
        await close.value
    }
    func refresh() async {
        let expected = epoch
        await connecting?.value
        guard current(expected), state == .ready, openingEpoch == nil, refreshingEpoch == nil else { return }
        refreshingEpoch = expected
        defer { if refreshingEpoch == expected { refreshingEpoch = nil } }
        sequence &+= 1
        let request = sequence
        do {
            let value = try await client.read()
            guard current(expected), sequence == request else { return }
            try accept(value)
        } catch {
            guard sequence == request else { return }
            await fail(error, epoch: expected)
        }
    }
    func selection(for sessionId: String) -> DSHSelection? {
        guard enabled, !suspended, state == .ready, let snapshot,
              let row = rows.first(where: { $0.sessionId == sessionId }) else { return nil }
        return DSHSelection(sessionId: sessionId, workspaceId: row.workspaceId, hostId: snapshot.hostId,
                              generation: snapshot.generation, localEpoch: epoch)
    }
    private func makeRequestId(_ explicit: String?) -> String {
        if let explicit, !explicit.isEmpty { return explicit }
        nextRequestSequence &+= 1
        return "dsh-req-\(epoch)-\(nextRequestSequence)"
    }
    func createIntent(workspaceId: String, requestId: String? = nil) -> DSHQueuedCommand? {
        guard enabled, !suspended, state == .ready, pendingUnknownCommand == nil, let snapshot,
              !workspaceId.isEmpty, (rows.isEmpty || rows.contains(where: { $0.workspaceId == workspaceId })) else { return nil }
        return .create(DSHCreateIntent(
            workspaceId: workspaceId,
            requestId: makeRequestId(requestId),
            hostId: snapshot.hostId,
            generation: snapshot.generation,
            localEpoch: epoch
        ))
    }
    func promptIntent(for sessionId: String, mode: DSHPromptMode, text: String, requestId: String? = nil) -> DSHQueuedCommand? {
        guard enabled, !suspended, state == .ready, pendingUnknownCommand == nil, let snapshot else { return nil }
        guard selected == sessionId else {
            lastCommandResult = .unconfirmed(selected: selected, reasonCode: "guard.selectionMismatch")
            return nil
        }
        guard let row = rows.first(where: { $0.sessionId == sessionId }),
              !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        if mode == .steer && !row.running {
            lastCommandResult = .denied(reasonCode: "guard.steerRequiresRunningTurn")
            return nil
        }
        return .prompt(DSHPromptIntent(
            sessionId: sessionId,
            workspaceId: row.workspaceId,
            requestId: makeRequestId(requestId),
            mode: mode,
            text: text,
            hostId: snapshot.hostId,
            generation: snapshot.generation,
            localEpoch: epoch
        ))
    }
    func cancelIntent(for sessionId: String, expectedTurnOverride: Int? = nil, requestId: String? = nil) -> DSHQueuedCommand? {
        guard enabled, !suspended, state == .ready, pendingUnknownCommand == nil, let snapshot else { return nil }
        guard selected == sessionId else {
            lastCommandResult = .unconfirmed(selected: selected, reasonCode: "guard.selectionMismatch")
            return nil
        }
        guard let row = rows.first(where: { $0.sessionId == sessionId }) else { return nil }
        guard row.running else {
            lastCommandResult = .noop(reasonCode: "guard.notRunning", currentTurn: row.currentTurn, queuedCount: row.queuedCount)
            return nil
        }
        guard atomicTurnCheckSupported, let resolvedTurn = expectedTurnOverride ?? row.currentTurn else {
            lastCommandResult = .denied(reasonCode: "guard.unprovableTurnAtomicity")
            return nil
        }
        return .cancel(DSHCancelIntent(
            sessionId: sessionId,
            workspaceId: row.workspaceId,
            requestId: makeRequestId(requestId),
            expectedTurn: resolvedTurn,
            hostId: snapshot.hostId,
            generation: snapshot.generation,
            localEpoch: epoch
        ))
    }
    func queryIntent(for sessionId: String? = nil, workspaceId: String? = nil, requestId: String) -> DSHQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot, !requestId.isEmpty else { return nil }
        return .query(DSHQueryIntent(
            sessionId: sessionId,
            workspaceId: workspaceId,
            requestId: requestId,
            hostId: snapshot.hostId,
            generation: snapshot.generation,
            localEpoch: epoch
        ))
    }
    func row(for sessionId: String) -> DSHSessionRow? {
        rows.first(where: { $0.sessionId == sessionId })
    }
    func resolveIntent(for sessionId: String, interactionId: String, interactionType: DSHInteractionType,
                       decision: DSHInteractionDecision, optionId: String? = nil, requestId: String? = nil) -> DSHQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot,
              let row = rows.first(where: { $0.sessionId == sessionId }),
              !interactionId.isEmpty else { return nil }
        return .resolve(DSHResolveIntent(
            sessionId: sessionId,
            workspaceId: row.workspaceId,
            requestId: makeRequestId(requestId),
            interactionId: interactionId,
            interactionType: interactionType,
            decision: decision,
            optionId: optionId,
            hostId: snapshot.hostId,
            generation: snapshot.generation,
            localEpoch: epoch
        ))
    }
    func switchModelIntent(for sessionId: String, provider: String, model: String, thought: String? = nil,
                            requestId: String? = nil) -> DSHQueuedCommand? {
        guard enabled, !suspended, state == .ready, let snapshot,
              let row = rows.first(where: { $0.sessionId == sessionId }),
              !provider.isEmpty, !model.isEmpty else { return nil }
        return .switchModel(DSHSwitchModelIntent(
            sessionId: sessionId,
            workspaceId: row.workspaceId,
            requestId: makeRequestId(requestId),
            provider: provider,
            model: model,
            thought: thought,
            hostId: snapshot.hostId,
            generation: snapshot.generation,
            localEpoch: epoch
        ))
    }
    func open(_ selection: DSHSelection?, admitted: @escaping @MainActor () -> Bool) async -> DSHOpenResult {
        guard let selection, openingEpoch == nil, refreshingEpoch == nil, let snapshot, enabled, !suspended, state == .ready,
              selection.localEpoch == epoch, selection.hostId == snapshot.hostId,
              selection.generation == snapshot.generation,
              snapshot.sessionIds.contains(selection.sessionId),
              rows.contains(where: { $0.sessionId == selection.sessionId && $0.workspaceId == selection.workspaceId }), admitted() else { return .stale }
        let expected = epoch
        openingEpoch = expected
        sequence &+= 1
        defer { if openingEpoch == expected { openingEpoch = nil } }
        do {
            let response = try await client.open(selection)
            guard current(expected), admitted() else { return .stale }
            guard response.hostId == selection.hostId, response.generation == selection.generation else {
                await fail(DSHFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastResult = .stale
                return .stale
            }
            let result: DSHOpenResult = response.status == .confirmed && response.selected != selection.sessionId
                ? .unconfirmed : response.status
            if result == .confirmed { selected = selection.sessionId }
            if [.stale, .denied, .unavailable].contains(result) {
                await fail(result == .denied ? DSHFailure.denied : result == .stale ? .stale : .unavailable, epoch: expected)
                guard current(expected) else { return result }
            }
            lastResult = result
            return result
        } catch {
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: DSHOpenResult = error as? DSHFailure == .denied ? .denied : error as? DSHFailure == .stale ? .stale : .unavailable
            lastResult = result
            return result
        }
    }
    func execute(_ command: DSHQueuedCommand?, admitted: @escaping @MainActor () -> Bool) async -> DSHCommandResult {
        guard let command else {
            let fallback = lastCommandResult ?? .stale
            lastCommandResult = fallback
            return fallback
        }
        guard openingEpoch == nil, refreshingEpoch == nil, let currentSnapshot = snapshot,
              enabled, !suspended, state == .ready,
              command.localEpoch == epoch, command.hostId == currentSnapshot.hostId,
              command.generation == currentSnapshot.generation, admitted() else {
            lastCommandResult = .stale
            return .stale
        }
        if let pending = pendingUnknownCommand, pending.requestId != command.requestId {
            let blocked = DSHCommandResult.unknown(requestId: pending.requestId, reasonCode: "guard.unknownOutcomeRequiresReconcile")
            lastCommandResult = blocked
            return blocked
        }
        switch command {
        case .create(let intent):
            guard rows.contains(where: { $0.workspaceId == intent.workspaceId }) else {
                let denied = DSHCommandResult.denied(reasonCode: "guard.unauthorizedWorkspace")
                lastCommandResult = denied
                return denied
            }
        case .prompt(let intent):
            guard selected == intent.sessionId else {
                let mismatch = DSHCommandResult.unconfirmed(selected: selected, reasonCode: "guard.selectionMismatch")
                lastCommandResult = mismatch
                return mismatch
            }
            guard currentSnapshot.sessionIds.contains(intent.sessionId),
                  let row = rows.first(where: { $0.sessionId == intent.sessionId && $0.workspaceId == intent.workspaceId }) else {
                lastCommandResult = .stale
                return .stale
            }
            if intent.mode == .steer && !row.running {
                let denied = DSHCommandResult.denied(reasonCode: "guard.steerRequiresRunningTurn")
                lastCommandResult = denied
                return denied
            }
        case .cancel(let intent):
            guard atomicTurnCheckSupported else {
                let denied = DSHCommandResult.denied(reasonCode: "guard.unprovableTurnAtomicity")
                lastCommandResult = denied
                return denied
            }
            guard selected == intent.sessionId else {
                let mismatch = DSHCommandResult.unconfirmed(selected: selected, reasonCode: "guard.selectionMismatch")
                lastCommandResult = mismatch
                return mismatch
            }
            guard currentSnapshot.sessionIds.contains(intent.sessionId),
                  let row = rows.first(where: { $0.sessionId == intent.sessionId && $0.workspaceId == intent.workspaceId }) else {
                lastCommandResult = .stale
                return .stale
            }
            if row.running && row.currentTurn == nil {
                let denied = DSHCommandResult.denied(reasonCode: "guard.unprovableTurnAtomicity")
                lastCommandResult = denied
                return denied
            }
        case .resolve(let intent):
            guard selected == intent.sessionId else {
                let mismatch = DSHCommandResult.unconfirmed(selected: selected, reasonCode: "guard.selectionMismatch")
                lastCommandResult = mismatch
                return mismatch
            }
            guard currentSnapshot.sessionIds.contains(intent.sessionId),
                  rows.contains(where: { $0.sessionId == intent.sessionId && $0.workspaceId == intent.workspaceId }) else {
                lastCommandResult = .stale
                return .stale
            }
        case .switchModel(let intent):
            guard selected == intent.sessionId else {
                let mismatch = DSHCommandResult.unconfirmed(selected: selected, reasonCode: "guard.selectionMismatch")
                lastCommandResult = mismatch
                return mismatch
            }
            guard currentSnapshot.sessionIds.contains(intent.sessionId),
                  let row = rows.first(where: { $0.sessionId == intent.sessionId && $0.workspaceId == intent.workspaceId }) else {
                lastCommandResult = .stale
                return .stale
            }
            if let targetOption = row.supportedModels?.first(where: { $0.model == intent.model && $0.provider == intent.provider }) {
                if let thought = intent.thought, !targetOption.supportedThoughts.contains(thought) {
                    let refused = DSHCommandResult.unsupportedThought(reasonCode: "guard.unsupportedThoughtLevel")
                    lastCommandResult = refused
                    return refused
                }
            }
        case .query:
            break
        }
        let expected = epoch
        openingEpoch = expected
        sequence &+= 1
        defer { if openingEpoch == expected { openingEpoch = nil } }
        do {
            let response: DSHCommandReply
            switch command {
            case .create(let intent): response = try await client.create(intent)
            case .prompt(let intent): response = try await client.prompt(intent)
            case .cancel(let intent): response = try await client.cancel(intent)
            case .query(let intent): response = try await client.query(intent)
            case .resolve(let intent): response = try await client.resolve(intent)
            case .switchModel(let intent): response = try await client.switchModel(intent)
            }
            guard current(expected), admitted() else {
                lastCommandResult = .stale
                return .stale
            }
            guard response.hostId == command.hostId, response.generation == command.generation else {
                await fail(DSHFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastCommandResult = .stale
                return .stale
            }
            let mapped = mapCommandReply(response, command: command)
            if case .unknown = mapped {
                pendingUnknownCommand = command
            } else if pendingUnknownCommand?.requestId == command.requestId {
                pendingUnknownCommand = nil
            }
            if case .stale = mapped {
                await fail(DSHFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
            } else if case .unavailable = mapped {
                await fail(DSHFailure.unavailable, epoch: expected)
                guard current(expected) else { return .unavailable }
            } else if case .denied(let reasonCode) = mapped, !(reasonCode?.hasPrefix("guard.") ?? false) {
                await fail(DSHFailure.denied, epoch: expected)
                guard current(expected) else { return mapped }
            }
            lastCommandResult = mapped
            return mapped
        } catch {
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: DSHCommandResult = error as? DSHFailure == .denied
                ? .denied(reasonCode: nil)
                : error as? DSHFailure == .stale ? .stale : .unavailable
            lastCommandResult = result
            return result
        }
    }
    private func mapCommandReply(_ response: DSHCommandReply, command: DSHQueuedCommand) -> DSHCommandResult {
        switch response.status {
        case "confirmed":
            guard let createdId = response.sessionId, let wsId = response.workspaceId,
                  response.selected == createdId else {
                return .unconfirmed(selected: response.selected, reasonCode: response.reasonCode ?? "reply.selectionMismatch")
            }
            selected = createdId
            applyCreatedSessionLocally(sessionId: createdId, workspaceId: wsId,
                                       currentTurn: response.currentTurn, queuedCount: response.queuedCount ?? 0)
            return .created(sessionId: createdId, workspaceId: wsId, selected: createdId, duplicate: response.duplicate)
        case "queued":
            guard let sessionId = response.sessionId, response.delivery == .queue,
                  let queuedCount = response.queuedCount else { return .unavailable }
            updateRowLocally(sessionId: sessionId, running: nil, currentTurn: response.currentTurn, queuedCount: queuedCount)
            return .queued(sessionId: sessionId, requestId: response.requestId,
                           currentTurn: response.currentTurn, queuedCount: queuedCount, duplicate: response.duplicate)
        case "steered":
            guard let sessionId = response.sessionId, response.delivery == .steer,
                  let queuedCount = response.queuedCount else { return .unavailable }
            updateRowLocally(sessionId: sessionId, running: nil, currentTurn: response.currentTurn, queuedCount: queuedCount)
            return .steered(sessionId: sessionId, requestId: response.requestId,
                            currentTurn: response.currentTurn, queuedCount: queuedCount, duplicate: response.duplicate)
        case "cancelled":
            guard let sessionId = response.sessionId, let expectedTurn = response.expectedTurn,
                  let queuedCount = response.queuedCount else { return .unavailable }
            updateRowLocally(sessionId: sessionId, running: false, currentTurn: response.currentTurn, queuedCount: queuedCount)
            return .cancelled(sessionId: sessionId, requestId: response.requestId,
                              expectedTurn: expectedTurn, currentTurn: response.currentTurn,
                              queuedCount: queuedCount, duplicate: response.duplicate)
        case "cancelledQueueRemaining":
            guard let sessionId = response.sessionId, let expectedTurn = response.expectedTurn,
                  let queuedCount = response.queuedCount, queuedCount > 0 else { return .unavailable }
            updateRowLocally(sessionId: sessionId, running: false, currentTurn: response.currentTurn, queuedCount: queuedCount)
            return .cancelledQueueRemaining(sessionId: sessionId, requestId: response.requestId,
                                            expectedTurn: expectedTurn, currentTurn: response.currentTurn,
                                            queuedCount: queuedCount, duplicate: response.duplicate)
        case "noop":
            if let sessionId = response.sessionId, let queuedCount = response.queuedCount {
                updateRowLocally(sessionId: sessionId, running: nil, currentTurn: response.currentTurn, queuedCount: queuedCount)
            }
            return .noop(reasonCode: response.reasonCode ?? "guard.noop",
                         currentTurn: response.currentTurn, queuedCount: response.queuedCount)
        case "unknown":
            return .unknown(requestId: response.requestId, reasonCode: response.reasonCode ?? "query.unknownRequest")
        case "unconfirmed":
            return .unconfirmed(selected: response.selected, reasonCode: response.reasonCode ?? "guard.unconfirmed")
        case "denied":
            return .denied(reasonCode: response.reasonCode)
        case "stale":
            return .stale
        case "resolved":
            guard let sessionId = response.sessionId else { return .unavailable }
            clearPendingInteractionLocally(sessionId: sessionId)
            return .resolved(sessionId: sessionId, requestId: response.requestId, duplicate: response.duplicate)
        case "interactionTypeMismatch":
            return .interactionTypeMismatch(reasonCode: response.reasonCode ?? "guard.interactionTypeMismatch")
        case "modelSwitched":
            guard let sessionId = response.sessionId, let config = response.modelConfig else { return .unavailable }
            updateModelConfigLocally(sessionId: sessionId, modelConfig: config)
            return .modelSwitched(sessionId: sessionId, requestId: response.requestId, modelConfig: config, duplicate: response.duplicate)
        case "unsupportedThought":
            return .unsupportedThought(reasonCode: response.reasonCode ?? "guard.unsupportedThoughtLevel")
        default:
            return .unavailable
        }
    }
    private func applyCreatedSessionLocally(sessionId: String, workspaceId: String, currentTurn: Int?, queuedCount: Int) {
        guard let currentSnapshot = snapshot else { return }
        var updatedIds = currentSnapshot.sessionIds
        if !updatedIds.contains(sessionId) { updatedIds.append(sessionId) }
        var updatedRows = rows.map { row in
            DSHSessionRow(
                sessionId: row.sessionId,
                workspaceId: row.workspaceId,
                title: row.title,
                active: row.sessionId == sessionId,
                running: row.running,
                pendingInput: row.pendingInput,
                currentTurn: row.currentTurn,
                queuedCount: row.queuedCount,
                pendingInteraction: row.pendingInteraction,
                modelConfig: row.modelConfig,
                supportedModels: row.supportedModels
            )
        }
        if !updatedRows.contains(where: { $0.sessionId == sessionId }) {
            updatedRows.append(DSHSessionRow(
                sessionId: sessionId,
                workspaceId: workspaceId,
                title: "New Session",
                active: true,
                running: false,
                pendingInput: 0,
                currentTurn: currentTurn,
                queuedCount: queuedCount
            ))
        }
        rows = updatedRows
        snapshot = DSHSessionSnapshot(
            hostId: currentSnapshot.hostId,
            generation: currentSnapshot.generation,
            sessionIds: updatedIds,
            cursor: currentSnapshot.cursor,
            rows: updatedRows,
            unavailableSessionIds: currentSnapshot.unavailableSessionIds,
            changes: currentSnapshot.changes,
            selected: sessionId
        )
    }
    private func updateRowLocally(sessionId: String, running: Bool?, currentTurn: Int?, queuedCount: Int) {
        guard let currentSnapshot = snapshot else { return }
        let updatedRows = rows.map { row -> DSHSessionRow in
            guard row.sessionId == sessionId else { return row }
            return DSHSessionRow(
                sessionId: row.sessionId,
                workspaceId: row.workspaceId,
                title: row.title,
                active: row.active,
                running: running ?? row.running,
                pendingInput: row.pendingInput,
                currentTurn: currentTurn,
                queuedCount: queuedCount,
                pendingInteraction: row.pendingInteraction,
                modelConfig: row.modelConfig,
                supportedModels: row.supportedModels
            )
        }
        rows = updatedRows
        snapshot = DSHSessionSnapshot(
            hostId: currentSnapshot.hostId,
            generation: currentSnapshot.generation,
            sessionIds: currentSnapshot.sessionIds,
            cursor: currentSnapshot.cursor,
            rows: updatedRows,
            unavailableSessionIds: currentSnapshot.unavailableSessionIds,
            changes: currentSnapshot.changes,
            selected: currentSnapshot.selected
        )
    }
    private func clearPendingInteractionLocally(sessionId: String) {
        guard let currentSnapshot = snapshot else { return }
        let updatedRows = rows.map { row -> DSHSessionRow in
            guard row.sessionId == sessionId else { return row }
            return DSHSessionRow(
                sessionId: row.sessionId,
                workspaceId: row.workspaceId,
                title: row.title,
                active: row.active,
                running: row.running,
                pendingInput: row.pendingInput,
                currentTurn: row.currentTurn,
                queuedCount: row.queuedCount,
                pendingInteraction: nil,
                modelConfig: row.modelConfig,
                supportedModels: row.supportedModels
            )
        }
        rows = updatedRows
        snapshot = DSHSessionSnapshot(
            hostId: currentSnapshot.hostId,
            generation: currentSnapshot.generation,
            sessionIds: currentSnapshot.sessionIds,
            cursor: currentSnapshot.cursor,
            rows: updatedRows,
            unavailableSessionIds: currentSnapshot.unavailableSessionIds,
            changes: currentSnapshot.changes,
            selected: currentSnapshot.selected
        )
    }
    private func updateModelConfigLocally(sessionId: String, modelConfig: DSHModelConfig) {
        guard let currentSnapshot = snapshot else { return }
        let updatedRows = rows.map { row -> DSHSessionRow in
            guard row.sessionId == sessionId else { return row }
            return DSHSessionRow(
                sessionId: row.sessionId,
                workspaceId: row.workspaceId,
                title: row.title,
                active: row.active,
                running: row.running,
                pendingInput: row.pendingInput,
                currentTurn: row.currentTurn,
                queuedCount: row.queuedCount,
                pendingInteraction: row.pendingInteraction,
                modelConfig: modelConfig,
                supportedModels: row.supportedModels
            )
        }
        rows = updatedRows
        snapshot = DSHSessionSnapshot(
            hostId: currentSnapshot.hostId,
            generation: currentSnapshot.generation,
            sessionIds: currentSnapshot.sessionIds,
            cursor: currentSnapshot.cursor,
            rows: updatedRows,
            unavailableSessionIds: currentSnapshot.unavailableSessionIds,
            changes: currentSnapshot.changes,
            selected: currentSnapshot.selected
        )
    }
}
