import Combine
import Foundation

@MainActor
final class ZCodeSessionController: ObservableObject {
    struct LatchedCommand: Equatable, Sendable {
        let scope: ZCodeScope
        let target: ZCodeTaskAddress?
        let commandId: String
        let clientId: String
        let sessionId: String?
        let expectedExecutionId: String?
    }

    @Published private(set) var enabled = false
    @Published private(set) var state: ZCodeConnectionState = .disabled
    @Published private(set) var rows: [ZCodeTaskRow] = []
    @Published private(set) var selected: ZCodeTaskAddress?
    @Published private(set) var lastResult: ZCodeOpenResult?
    @Published private(set) var lastCommandResult: ZCodeCommandResult?
    @Published private(set) var lastCommandReply: ZCodeCommandReply?
    @Published private(set) var pendingCommands: [String: LatchedCommand] = [:]
    @Published private(set) var voiceReplyEnabled: Bool = false

    func setVoiceReply(enabled: Bool) async -> (Bool, String) {
        voiceReplyEnabled = enabled
        return (true, enabled ? "已开启 ZCode 语音回复服务" : "已关闭 ZCode 语音回复服务")
    }

    #if DEBUG
    func setSnapshotForTesting(rows: [ZCodeTaskRow], selected: ZCodeTaskAddress?) {
        self.rows = rows
        self.selected = selected
    }
    #endif
    private var resolvedCommands: [String: (address: ZCodeTaskAddress, interactionId: String, interactionType: ZCodeInteractionType)] = [:]
    private let client: any ZCodeSessionServing
    private let pollNanoseconds: UInt64
    private var snapshot: ZCodeSessionSnapshot?
    private var epoch: UInt64 = 0
    private var sequence: UInt64 = 0
    private var opening = false
    private var suspended = false
    private var connecting: Task<Void, Never>?
    private var polling: Task<Void, Never>?
    private var closing: Task<Void, Never>?

    init(client: any ZCodeSessionServing = ZCodeSessionClient(), pollNanoseconds: UInt64 = 2_000_000_000) {
        self.client = client
        self.pollNanoseconds = pollNanoseconds
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
        connecting?.cancel(); connecting = nil
        polling?.cancel(); polling = nil
        snapshot = nil; rows = []; selected = nil; lastResult = nil
        lastCommandResult = nil; lastCommandReply = nil; pendingCommands.removeAll(); resolvedCommands.removeAll()
        state = .disabled
        let previous = closing
        closing = Task { [client] in await previous?.value; await client.close() }
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
    private func accept(_ value: ZCodeSessionSnapshot) throws {
        try value.validate()
        if let snapshot {
            guard snapshot.instanceId == value.instanceId, snapshot.generation == value.generation,
                  snapshot.scopes == value.scopes else { throw ZCodeFailure.stale }
        }
        snapshot = value; rows = value.rows; selected = value.selected; state = .ready
    }
    private func fail(_ error: Error, epoch expected: UInt64) async {
        guard current(expected) else { return }
        rows = []; selected = nil; snapshot = nil; lastResult = nil
        pendingCommands.removeAll(); resolvedCommands.removeAll()
        state = .unavailable(error as? ZCodeFailure ?? .unavailable)
        polling?.cancel(); polling = nil
        let previous = closing
        let close = Task { [client] in await previous?.value; await client.close() }
        closing = close
        await close.value
    }
    func refresh() async {
        let expected = epoch
        await connecting?.value
        guard current(expected), state == .ready, !opening else { return }
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
    func selection(for address: ZCodeTaskAddress) -> ZCodeSelection? {
        guard enabled, !suspended, state == .ready, let snapshot,
              rows.contains(where: { $0.address == address }) else { return nil }
        return ZCodeSelection(address: address, instanceId: snapshot.instanceId,
                              generation: snapshot.generation, localEpoch: epoch)
    }
    private func createLatchKey(_ scope: ZCodeScope) -> String {
        "create:\(scope.workspacePath)|\(scope.workspaceIdentity ?? "")|\(scope.sourceServerId ?? "")|\(scope.remoteSessionId ?? "")"
    }
    private func sessionLatchKey(_ address: ZCodeTaskAddress) -> String {
        "session:\(address.workspacePath)|\(address.workspaceIdentity ?? "")|\(address.sourceServerId ?? "")|\(address.remoteSessionId ?? "")|\(address.taskId)"
    }
    func latchedCommand(forScope scope: ZCodeScope) -> LatchedCommand? {
        pendingCommands[createLatchKey(scope)]
    }
    func latchedCommand(forAddress address: ZCodeTaskAddress) -> LatchedCommand? {
        pendingCommands[sessionLatchKey(address)]
    }
    var defaultScope: ZCodeScope? {
        if let selected, let row = rows.first(where: { $0.address == selected }) {
            return row.address.scope
        }
        return rows.first?.address.scope ?? snapshot?.scopes.first
    }
    func createIntent(
        for scope: ZCodeScope,
        commandId: String = UUID().uuidString,
        clientId: String = "micro-input",
        sessionId: String? = nil,
        firstInputText: String? = nil
    ) -> ZCodeCreateIntent? {
        guard enabled, !suspended, state == .ready, let snapshot,
              scope.valid, snapshot.scopes.contains(scope),
              !commandId.isEmpty, !clientId.isEmpty else { return nil }
        if let latched = latchedCommand(forScope: scope) {
            return ZCodeCreateIntent(
                scope: scope,
                commandId: latched.commandId,
                clientId: latched.clientId,
                sessionId: latched.sessionId,
                firstInputText: firstInputText,
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                localEpoch: epoch
            )
        }
        let resolvedSessionId = sessionId ?? "zcode-session-\(commandId)"
        return ZCodeCreateIntent(
            scope: scope,
            commandId: commandId,
            clientId: clientId,
            sessionId: resolvedSessionId,
            firstInputText: firstInputText,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }
    func sendIntent(
        for address: ZCodeTaskAddress? = nil,
        text: String,
        requestedDelivery: ZCodeDelivery? = nil,
        commandId: String = UUID().uuidString,
        clientId: String = "micro-input"
    ) -> ZCodeSendIntent? {
        let target = address ?? selected
        guard let target, let selection = selection(for: target),
              !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              !commandId.isEmpty, !clientId.isEmpty else { return nil }
        let latched = latchedCommand(forAddress: target)
        return ZCodeSendIntent(
            selection: selection,
            commandId: latched?.commandId ?? commandId,
            clientId: latched?.clientId ?? clientId,
            text: text,
            requestedDelivery: requestedDelivery
        )
    }
    func stopIntent(
        for address: ZCodeTaskAddress? = nil,
        expectedExecutionId: String? = nil,
        commandId: String = UUID().uuidString,
        clientId: String = "micro-input"
    ) -> ZCodeStopIntent? {
        let target = address ?? selected
        guard let target, let selection = selection(for: target),
              let row = rows.first(where: { $0.address == target }),
              !commandId.isEmpty, !clientId.isEmpty else { return nil }
        let latched = latchedCommand(forAddress: target)
        let executionId = latched?.expectedExecutionId ?? expectedExecutionId ?? row.executionId
        guard let executionId, !executionId.isEmpty else { return nil }
        return ZCodeStopIntent(
            selection: selection,
            commandId: latched?.commandId ?? commandId,
            clientId: latched?.clientId ?? clientId,
            expectedExecutionId: executionId
        )
    }
    func resolveIntent(
        for address: ZCodeTaskAddress? = nil,
        interactionId: String? = nil,
        interactionType: ZCodeInteractionType? = nil,
        decision: ZCodeInteractionDecision,
        optionId: String? = nil,
        commandId: String = UUID().uuidString,
        clientId: String = "micro-input"
    ) -> ZCodeResolveIntent? {
        let target = address ?? selected
        guard let target, let selection = selection(for: target),
              let row = rows.first(where: { $0.address == target }),
              !commandId.isEmpty, !clientId.isEmpty else { return nil }
        let latched = latchedCommand(forAddress: target)
        let resolvedInteractionId = interactionId ?? row.pendingInteraction?.interactionId
        let resolvedInteractionType = interactionType ?? row.pendingInteraction?.interactionType
        guard let resolvedInteractionId, !resolvedInteractionId.isEmpty,
              let resolvedInteractionType else { return nil }
        return ZCodeResolveIntent(
            selection: selection,
            commandId: latched?.commandId ?? commandId,
            clientId: latched?.clientId ?? clientId,
            interactionId: resolvedInteractionId,
            interactionType: resolvedInteractionType,
            decision: decision,
            optionId: optionId
        )
    }
    func switchModelIntent(
        for address: ZCodeTaskAddress? = nil,
        provider: String? = nil,
        model: String? = nil,
        thought: String,
        commandId: String = UUID().uuidString,
        clientId: String = "micro-input"
    ) -> ZCodeSwitchModelIntent? {
        let target = address ?? selected
        guard let target, let selection = selection(for: target),
              let row = rows.first(where: { $0.address == target }),
              !commandId.isEmpty, !clientId.isEmpty,
              !thought.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        let latched = latchedCommand(forAddress: target)
        let resolvedProvider = provider ?? row.modelConfig?.provider
        let resolvedModel = model ?? row.modelConfig?.model
        guard let resolvedProvider, !resolvedProvider.isEmpty,
              let resolvedModel, !resolvedModel.isEmpty else { return nil }
        return ZCodeSwitchModelIntent(
            selection: selection,
            commandId: latched?.commandId ?? commandId,
            clientId: latched?.clientId ?? clientId,
            provider: resolvedProvider,
            model: resolvedModel,
            thought: thought
        )
    }
    func queryIntent(forScope scope: ZCodeScope, target: ZCodeTaskAddress? = nil, commandId: String? = nil) -> ZCodeQueryIntent? {
        guard enabled, !suspended, state == .ready, let snapshot,
              scope.valid, snapshot.scopes.contains(scope) else { return nil }
        let latched = target.flatMap { latchedCommand(forAddress: $0) } ?? latchedCommand(forScope: scope)
        guard let resolvedCommandId = commandId ?? latched?.commandId, !resolvedCommandId.isEmpty else { return nil }
        return ZCodeQueryIntent(
            scope: scope,
            target: target ?? latched?.target,
            commandId: resolvedCommandId,
            instanceId: snapshot.instanceId,
            generation: snapshot.generation,
            localEpoch: epoch
        )
    }
    func open(_ selection: ZCodeSelection?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeOpenResult {
        guard let selection, !opening, let snapshot, enabled, !suspended, state == .ready,
              selection.localEpoch == epoch, selection.instanceId == snapshot.instanceId,
              selection.generation == snapshot.generation,
              snapshot.scopes.contains(selection.address.scope),
              rows.contains(where: { $0.address == selection.address }), admitted() else { return .stale }
        opening = true
        sequence &+= 1
        let expected = epoch
        defer { opening = false }
        do {
            let response = try await client.open(selection)
            guard current(expected), admitted() else { return .stale }
            guard response.instanceId == selection.instanceId, response.generation == selection.generation else {
                await fail(ZCodeFailure.stale, epoch: expected)
                guard current(expected) else { return .stale }
                lastResult = .stale
                return .stale
            }
            let result: ZCodeOpenResult = response.status == .confirmed && response.selected != selection.address
                ? .unconfirmed : response.status
            if result == .confirmed { selected = selection.address }
            if [.stale, .denied, .unavailable].contains(result) {
                await fail(result == .denied ? ZCodeFailure.denied : result == .stale ? .stale : .unavailable, epoch: expected)
                guard current(expected) else { return result }
            }
            lastResult = result
            return result
        } catch {
            await fail(error, epoch: expected)
            guard current(expected) else { return .stale }
            let result: ZCodeOpenResult = error as? ZCodeFailure == .denied ? .denied : error as? ZCodeFailure == .stale ? .stale : .unavailable
            lastResult = result
            return result
        }
    }
    func execute(_ command: ZCodeQueuedCommand?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let command else { return .stale }
        switch command {
        case .create(let intent): return await create(intent, admitted: admitted)
        case .send(let intent): return await send(intent, admitted: admitted)
        case .stop(let intent): return await stop(intent, admitted: admitted)
        case .resolve(let intent): return await resolve(intent, admitted: admitted)
        case .switchModel(let intent): return await switchModel(intent, admitted: admitted)
        case .query(let intent): return await query(intent, admitted: admitted)
        }
    }
    func create(_ intent: ZCodeCreateIntent?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let intent, !opening, let snapshot, enabled, !suspended, state == .ready,
              intent.localEpoch == epoch, intent.instanceId == snapshot.instanceId,
              intent.generation == snapshot.generation,
              intent.scope.valid, snapshot.scopes.contains(intent.scope),
              !intent.commandId.isEmpty, !intent.clientId.isEmpty, admitted() else { return .stale }
        let latchKey = createLatchKey(intent.scope)
        if let latched = pendingCommands[latchKey] {
            let reconcileIntent = ZCodeQueryIntent(
                scope: intent.scope,
                target: latched.target,
                commandId: latched.commandId,
                instanceId: intent.instanceId,
                generation: intent.generation,
                localEpoch: intent.localEpoch
            )
            return await query(reconcileIntent, admitted: admitted)
        }
        opening = true
        sequence &+= 1
        let expected = epoch
        let resolvedSessionId = intent.sessionId ?? "zcode-session-\(intent.commandId)"
        let expectedTarget = ZCodeTaskAddress(
            workspacePath: intent.scope.workspacePath,
            workspaceIdentity: intent.scope.workspaceIdentity,
            sourceServerId: intent.scope.sourceServerId,
            remoteSessionId: intent.scope.remoteSessionId,
            taskId: resolvedSessionId
        )
        defer { opening = false }
        do {
            var effectiveIntent = intent
            effectiveIntent.sessionId = resolvedSessionId
            let response = try await client.create(effectiveIntent)
            guard current(expected), admitted() else { return .stale }
            return await handleCommandResponse(
                response,
                expectedEpoch: expected,
                expectedInstanceId: intent.instanceId,
                expectedGeneration: intent.generation,
                expectedScope: intent.scope,
                expectedTarget: expectedTarget,
                isCreate: true,
                firstInputText: intent.firstInputText,
                latchKey: latchKey,
                latched: LatchedCommand(
                    scope: intent.scope,
                    target: expectedTarget,
                    commandId: intent.commandId,
                    clientId: intent.clientId,
                    sessionId: resolvedSessionId,
                    expectedExecutionId: nil
                )
            )
        } catch ZCodeFailure.transportTimeout {
            guard current(expected), admitted() else { return .stale }
            let latched = LatchedCommand(
                scope: intent.scope,
                target: expectedTarget,
                commandId: intent.commandId,
                clientId: intent.clientId,
                sessionId: resolvedSessionId,
                expectedExecutionId: nil
            )
            pendingCommands[latchKey] = latched
            return await reconcileAfterTimeout(
                latchKey: latchKey,
                latched: latched,
                expectedEpoch: expected,
                expectedInstanceId: intent.instanceId,
                expectedGeneration: intent.generation,
                isCreate: true,
                firstInputText: intent.firstInputText,
                admitted: admitted
            )
        } catch {
            return await failCommand(error, epoch: expected)
        }
    }
    func send(_ intent: ZCodeSendIntent?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let intent, !opening, let snapshot, enabled, !suspended, state == .ready,
              intent.selection.localEpoch == epoch, intent.selection.instanceId == snapshot.instanceId,
              intent.selection.generation == snapshot.generation,
              snapshot.scopes.contains(intent.selection.address.scope),
              rows.contains(where: { $0.address == intent.selection.address }),
              !intent.commandId.isEmpty, !intent.clientId.isEmpty,
              !intent.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              admitted() else { return .stale }
        guard selected == intent.selection.address else {
            let reply = ZCodeCommandReply(
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                status: .unconfirmed,
                selected: selected,
                commandId: intent.commandId
            )
            lastCommandReply = reply
            lastCommandResult = .unconfirmed
            return .unconfirmed
        }
        let latchKey = sessionLatchKey(intent.selection.address)
        if let latched = pendingCommands[latchKey] {
            let reconcileIntent = ZCodeQueryIntent(
                scope: intent.selection.address.scope,
                target: intent.selection.address,
                commandId: latched.commandId,
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                localEpoch: intent.selection.localEpoch
            )
            return await query(reconcileIntent, admitted: admitted)
        }
        opening = true
        sequence &+= 1
        let expected = epoch
        let latched = LatchedCommand(
            scope: intent.selection.address.scope,
            target: intent.selection.address,
            commandId: intent.commandId,
            clientId: intent.clientId,
            sessionId: intent.selection.address.taskId,
            expectedExecutionId: nil
        )
        defer { opening = false }
        do {
            let response = try await client.send(intent)
            guard current(expected), admitted() else { return .stale }
            return await handleCommandResponse(
                response,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                expectedScope: intent.selection.address.scope,
                expectedTarget: intent.selection.address,
                isCreate: false,
                firstInputText: nil,
                latchKey: latchKey,
                latched: latched
            )
        } catch ZCodeFailure.transportTimeout {
            guard current(expected), admitted() else { return .stale }
            pendingCommands[latchKey] = latched
            return await reconcileAfterTimeout(
                latchKey: latchKey,
                latched: latched,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                isCreate: false,
                firstInputText: nil,
                admitted: admitted
            )
        } catch {
            return await failCommand(error, epoch: expected)
        }
    }
    func stop(_ intent: ZCodeStopIntent?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let intent, !opening, let snapshot, enabled, !suspended, state == .ready,
              intent.selection.localEpoch == epoch, intent.selection.instanceId == snapshot.instanceId,
              intent.selection.generation == snapshot.generation,
              snapshot.scopes.contains(intent.selection.address.scope),
              rows.contains(where: { $0.address == intent.selection.address }),
              !intent.commandId.isEmpty, !intent.clientId.isEmpty,
              !intent.expectedExecutionId.isEmpty,
              admitted() else { return .stale }
        guard selected == intent.selection.address else {
            let reply = ZCodeCommandReply(
                instanceId: snapshot.instanceId,
                generation: snapshot.generation,
                status: .unconfirmed,
                selected: selected,
                commandId: intent.commandId
            )
            lastCommandReply = reply
            lastCommandResult = .unconfirmed
            return .unconfirmed
        }
        let latchKey = sessionLatchKey(intent.selection.address)
        if let latched = pendingCommands[latchKey] {
            let reconcileIntent = ZCodeQueryIntent(
                scope: intent.selection.address.scope,
                target: intent.selection.address,
                commandId: latched.commandId,
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                localEpoch: intent.selection.localEpoch
            )
            return await query(reconcileIntent, admitted: admitted)
        }
        opening = true
        sequence &+= 1
        let expected = epoch
        let latched = LatchedCommand(
            scope: intent.selection.address.scope,
            target: intent.selection.address,
            commandId: intent.commandId,
            clientId: intent.clientId,
            sessionId: intent.selection.address.taskId,
            expectedExecutionId: intent.expectedExecutionId
        )
        defer { opening = false }
        do {
            let response = try await client.stop(intent)
            guard current(expected), admitted() else { return .stale }
            return await handleCommandResponse(
                response,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                expectedScope: intent.selection.address.scope,
                expectedTarget: intent.selection.address,
                isCreate: false,
                firstInputText: nil,
                latchKey: latchKey,
                latched: latched
            )
        } catch ZCodeFailure.transportTimeout {
            guard current(expected), admitted() else { return .stale }
            pendingCommands[latchKey] = latched
            return await reconcileAfterTimeout(
                latchKey: latchKey,
                latched: latched,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                isCreate: false,
                firstInputText: nil,
                admitted: admitted
            )
        } catch {
            return await failCommand(error, epoch: expected)
        }
    }
    func resolve(_ intent: ZCodeResolveIntent?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let intent, !opening, let snapshot, enabled, !suspended, state == .ready,
              intent.selection.localEpoch == epoch, intent.selection.instanceId == snapshot.instanceId,
              intent.selection.generation == snapshot.generation,
              snapshot.scopes.contains(intent.selection.address.scope),
              let row = rows.first(where: { $0.address == intent.selection.address }),
              !intent.commandId.isEmpty, !intent.clientId.isEmpty,
              !intent.interactionId.isEmpty, admitted() else { return .stale }
        guard selected == intent.selection.address else {
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .unconfirmed,
                selected: selected,
                commandId: intent.commandId
            )
            lastCommandReply = reply
            lastCommandResult = .unconfirmed
            return .unconfirmed
        }
        let latchKey = sessionLatchKey(intent.selection.address)
        if let latched = pendingCommands[latchKey] {
            let reconcileIntent = ZCodeQueryIntent(
                scope: intent.selection.address.scope,
                target: intent.selection.address,
                commandId: latched.commandId,
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                localEpoch: intent.selection.localEpoch
            )
            return await query(reconcileIntent, admitted: admitted)
        }
        if intent.interactionType == .question && (intent.decision == .approve || intent.decision == .allowAlways || intent.optionId == "allowOnce" || intent.optionId == "allowAlways") {
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .interactionTypeMismatch,
                selected: selected,
                commandId: intent.commandId,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: "guard.questionCannotApproveAsPermission"
            )
            lastCommandReply = reply
            lastCommandResult = .interactionTypeMismatch
            return .interactionTypeMismatch
        }
        if let prior = resolvedCommands[intent.commandId], prior.address == intent.selection.address {
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .resolved,
                selected: selected,
                commandId: intent.commandId,
                duplicate: true,
                interactionId: prior.interactionId,
                interactionType: prior.interactionType,
                reasonCode: "proto.duplicateCommandId"
            )
            lastCommandReply = reply
            lastCommandResult = .resolved
            return .resolved
        }
        guard let pending = row.pendingInteraction, pending.interactionId == intent.interactionId else {
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .noop,
                selected: selected,
                commandId: intent.commandId,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: "proto.alreadyResolved"
            )
            lastCommandReply = reply
            lastCommandResult = .noop
            return .noop
        }
        guard pending.interactionType == intent.interactionType else {
            let reason = (pending.interactionType == .question && intent.interactionType == .permission)
                ? "guard.questionCannotApproveAsPermission"
                : "guard.interactionTypeMismatch"
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .interactionTypeMismatch,
                selected: selected,
                commandId: intent.commandId,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: reason
            )
            lastCommandReply = reply
            lastCommandResult = .interactionTypeMismatch
            return .interactionTypeMismatch
        }
        opening = true
        sequence &+= 1
        let expected = epoch
        let latched = LatchedCommand(
            scope: intent.selection.address.scope,
            target: intent.selection.address,
            commandId: intent.commandId,
            clientId: intent.clientId,
            sessionId: intent.selection.address.taskId,
            expectedExecutionId: nil
        )
        defer { opening = false }
        do {
            let response = try await client.resolve(intent)
            guard current(expected), admitted() else { return .stale }
            let result = await handleCommandResponse(
                response,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                expectedScope: intent.selection.address.scope,
                expectedTarget: intent.selection.address,
                isCreate: false,
                firstInputText: nil,
                latchKey: latchKey,
                latched: latched
            )
            if result == .resolved {
                resolvedCommands[intent.commandId] = (
                    address: intent.selection.address,
                    interactionId: intent.interactionId,
                    interactionType: intent.interactionType
                )
            }
            return result
        } catch ZCodeFailure.transportTimeout {
            guard current(expected), admitted() else { return .stale }
            pendingCommands[latchKey] = latched
            return await reconcileAfterTimeout(
                latchKey: latchKey,
                latched: latched,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                isCreate: false,
                firstInputText: nil,
                admitted: admitted
            )
        } catch {
            return await failCommand(error, epoch: expected)
        }
    }
    func switchModel(_ intent: ZCodeSwitchModelIntent?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let intent, !opening, let snapshot, enabled, !suspended, state == .ready,
              intent.selection.localEpoch == epoch, intent.selection.instanceId == snapshot.instanceId,
              intent.selection.generation == snapshot.generation,
              snapshot.scopes.contains(intent.selection.address.scope),
              let row = rows.first(where: { $0.address == intent.selection.address }),
              !intent.commandId.isEmpty, !intent.clientId.isEmpty,
              !intent.provider.isEmpty, !intent.model.isEmpty, !intent.thought.isEmpty,
              admitted() else { return .stale }
        guard selected == intent.selection.address else {
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .unconfirmed,
                selected: selected,
                commandId: intent.commandId
            )
            lastCommandReply = reply
            lastCommandResult = .unconfirmed
            return .unconfirmed
        }
        let latchKey = sessionLatchKey(intent.selection.address)
        if let latched = pendingCommands[latchKey] {
            let reconcileIntent = ZCodeQueryIntent(
                scope: intent.selection.address.scope,
                target: intent.selection.address,
                commandId: latched.commandId,
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                localEpoch: intent.selection.localEpoch
            )
            return await query(reconcileIntent, admitted: admitted)
        }
        if let modelConfig = row.modelConfig,
           !modelConfig.supportsThought(intent.thought, provider: intent.provider, model: intent.model) {
            let reply = ZCodeCommandReply(
                instanceId: intent.selection.instanceId,
                generation: intent.selection.generation,
                status: .unsupportedThought,
                selected: selected,
                commandId: intent.commandId,
                modelConfig: modelConfig,
                reasonCode: "guard.unsupportedThoughtLevel"
            )
            lastCommandReply = reply
            lastCommandResult = .unsupportedThought
            return .unsupportedThought
        }
        opening = true
        sequence &+= 1
        let expected = epoch
        let latched = LatchedCommand(
            scope: intent.selection.address.scope,
            target: intent.selection.address,
            commandId: intent.commandId,
            clientId: intent.clientId,
            sessionId: intent.selection.address.taskId,
            expectedExecutionId: nil
        )
        defer { opening = false }
        do {
            var response = try await client.switchModel(intent)
            guard current(expected), admitted() else { return .stale }
            if response.status == .modelSwitched || response.status == .confirmed {
                guard let readback = response.modelConfig,
                      readback.valid,
                      readback.provider == intent.provider,
                      readback.model == intent.model,
                      readback.thought == intent.thought else {
                    response = ZCodeCommandReply(
                        instanceId: response.instanceId,
                        generation: response.generation,
                        status: .unconfirmed,
                        selected: response.selected,
                        commandId: response.commandId ?? intent.commandId,
                        duplicate: response.duplicate,
                        modelConfig: response.modelConfig,
                        reasonCode: response.reasonCode ?? "guard.modelConfigReadbackMismatch"
                    )
                    return await handleCommandResponse(
                        response,
                        expectedEpoch: expected,
                        expectedInstanceId: intent.selection.instanceId,
                        expectedGeneration: intent.selection.generation,
                        expectedScope: intent.selection.address.scope,
                        expectedTarget: intent.selection.address,
                        isCreate: false,
                        firstInputText: nil,
                        latchKey: latchKey,
                        latched: latched
                    )
                }
                response = ZCodeCommandReply(
                    instanceId: response.instanceId,
                    generation: response.generation,
                    status: .modelSwitched,
                    selected: response.selected,
                    commandId: response.commandId ?? intent.commandId,
                    duplicate: response.duplicate,
                    modelConfig: readback,
                    reasonCode: response.reasonCode
                )
            }
            return await handleCommandResponse(
                response,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                expectedScope: intent.selection.address.scope,
                expectedTarget: intent.selection.address,
                isCreate: false,
                firstInputText: nil,
                latchKey: latchKey,
                latched: latched
            )
        } catch ZCodeFailure.transportTimeout {
            guard current(expected), admitted() else { return .stale }
            pendingCommands[latchKey] = latched
            return await reconcileAfterTimeout(
                latchKey: latchKey,
                latched: latched,
                expectedEpoch: expected,
                expectedInstanceId: intent.selection.instanceId,
                expectedGeneration: intent.selection.generation,
                isCreate: false,
                firstInputText: nil,
                admitted: admitted
            )
        } catch {
            return await failCommand(error, epoch: expected)
        }
    }
    func query(_ intent: ZCodeQueryIntent?, admitted: @escaping @MainActor () -> Bool) async -> ZCodeCommandResult {
        guard let intent, !opening, let snapshot, enabled, !suspended, state == .ready,
              intent.localEpoch == epoch, intent.instanceId == snapshot.instanceId,
              intent.generation == snapshot.generation,
              intent.scope.valid, snapshot.scopes.contains(intent.scope),
              !intent.commandId.isEmpty, admitted() else { return .stale }
        opening = true
        sequence &+= 1
        let expected = epoch
        let latchKey = intent.target.map(sessionLatchKey) ?? createLatchKey(intent.scope)
        let latched = pendingCommands[latchKey] ?? LatchedCommand(
            scope: intent.scope,
            target: intent.target,
            commandId: intent.commandId,
            clientId: "micro-input",
            sessionId: intent.target?.taskId,
            expectedExecutionId: nil
        )
        defer { opening = false }
        do {
            let response = try await client.query(intent)
            guard current(expected), admitted() else { return .stale }
            return await handleCommandResponse(
                response,
                expectedEpoch: expected,
                expectedInstanceId: intent.instanceId,
                expectedGeneration: intent.generation,
                expectedScope: intent.scope,
                expectedTarget: intent.target ?? latched.target,
                isCreate: pendingCommands[createLatchKey(intent.scope)] != nil,
                firstInputText: nil,
                latchKey: latchKey,
                latched: latched
            )
        } catch ZCodeFailure.transportTimeout {
            guard current(expected), admitted() else { return .stale }
            pendingCommands[latchKey] = latched
            let reply = ZCodeCommandReply(
                instanceId: intent.instanceId,
                generation: intent.generation,
                status: .unknown,
                selected: selected,
                commandId: intent.commandId
            )
            lastCommandReply = reply
            lastCommandResult = .unknown
            return .unknown
        } catch {
            return await failCommand(error, epoch: expected)
        }
    }
    private func reconcileAfterTimeout(
        latchKey: String,
        latched: LatchedCommand,
        expectedEpoch: UInt64,
        expectedInstanceId: String,
        expectedGeneration: UInt64,
        isCreate: Bool,
        firstInputText: String?,
        admitted: @escaping @MainActor () -> Bool
    ) async -> ZCodeCommandResult {
        let queryIntent = ZCodeQueryIntent(
            scope: latched.scope,
            target: isCreate ? nil : latched.target,
            commandId: latched.commandId,
            instanceId: expectedInstanceId,
            generation: expectedGeneration,
            localEpoch: expectedEpoch
        )
        do {
            let response = try await client.query(queryIntent)
            guard current(expectedEpoch), admitted() else { return .stale }
            return await handleCommandResponse(
                response,
                expectedEpoch: expectedEpoch,
                expectedInstanceId: expectedInstanceId,
                expectedGeneration: expectedGeneration,
                expectedScope: latched.scope,
                expectedTarget: latched.target,
                isCreate: isCreate,
                firstInputText: firstInputText,
                latchKey: latchKey,
                latched: latched
            )
        } catch ZCodeFailure.transportTimeout {
            guard current(expectedEpoch), admitted() else { return .stale }
            let reply = ZCodeCommandReply(
                instanceId: expectedInstanceId,
                generation: expectedGeneration,
                status: .unknown,
                selected: selected,
                commandId: latched.commandId
            )
            lastCommandReply = reply
            lastCommandResult = .unknown
            return .unknown
        } catch {
            return await failCommand(error, epoch: expectedEpoch)
        }
    }
    private func handleCommandResponse(
        _ response: ZCodeCommandReply,
        expectedEpoch: UInt64,
        expectedInstanceId: String,
        expectedGeneration: UInt64,
        expectedScope: ZCodeScope,
        expectedTarget: ZCodeTaskAddress?,
        isCreate: Bool,
        firstInputText: String?,
        latchKey: String,
        latched: LatchedCommand
    ) async -> ZCodeCommandResult {
        guard response.instanceId == expectedInstanceId, response.generation == expectedGeneration else {
            await fail(ZCodeFailure.stale, epoch: expectedEpoch)
            guard current(expectedEpoch) else { return .stale }
            lastCommandReply = response
            lastCommandResult = .stale
            return .stale
        }
        var resolvedStatus = response.status
        if let delivery = response.delivery {
            switch delivery {
            case .guide:
                resolvedStatus = .steered
            case .queue:
                resolvedStatus = .queued
            case .startNow:
                if resolvedStatus != .confirmed {
                    resolvedStatus = .started
                }
            }
        }
        if isCreate && resolvedStatus == .confirmed {
            guard let created = response.selected,
                  created.valid,
                  created.scope == expectedScope,
                  expectedTarget == nil || created == expectedTarget else {
                resolvedStatus = .unconfirmed
                let adjusted = ZCodeCommandReply(
                    instanceId: response.instanceId,
                    generation: response.generation,
                    status: .unconfirmed,
                    selected: response.selected,
                    commandId: response.commandId ?? latched.commandId,
                    duplicate: response.duplicate,
                    delivery: response.delivery,
                    executionId: response.executionId,
                    reasonCode: response.reasonCode
                )
                pendingCommands.removeValue(forKey: latchKey)
                lastCommandReply = adjusted
                lastCommandResult = .unconfirmed
                return .unconfirmed
            }
            selected = created
            if !rows.contains(where: { $0.address == created }) {
                let initialLiveStatus = (firstInputText?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false)
                    ? "running" : "idle"
                rows.append(
                    ZCodeTaskRow(
                        address: created,
                        title: "新建任务",
                        liveStatus: initialLiveStatus,
                        executionId: response.executionId
                    )
                )
            }
        } else if [.started, .queued, .steered, .stopped, .resolved, .modelSwitched, .noop].contains(resolvedStatus),
                  let expectedTarget {
            if let returnedSelected = response.selected, returnedSelected != expectedTarget {
                resolvedStatus = .unconfirmed
            } else {
                selected = expectedTarget
                if let idx = rows.firstIndex(where: { $0.address == expectedTarget }) {
                    let existing = rows[idx]
                    switch resolvedStatus {
                    case .started, .queued, .steered:
                        rows[idx] = ZCodeTaskRow(
                            address: existing.address,
                            title: existing.title,
                            liveStatus: "running",
                            executionId: response.executionId ?? existing.executionId,
                            pendingInteraction: existing.pendingInteraction,
                            modelConfig: existing.modelConfig
                        )
                    case .stopped:
                        rows[idx] = ZCodeTaskRow(
                            address: existing.address,
                            title: existing.title,
                            liveStatus: "idle",
                            executionId: nil,
                            pendingInteraction: existing.pendingInteraction,
                            modelConfig: existing.modelConfig
                        )
                    case .resolved:
                        rows[idx] = ZCodeTaskRow(
                            address: existing.address,
                            title: existing.title,
                            liveStatus: existing.liveStatus == "waiting" ? "running" : existing.liveStatus,
                            executionId: existing.executionId,
                            pendingInteraction: nil,
                            modelConfig: existing.modelConfig
                        )
                    case .modelSwitched:
                        rows[idx] = ZCodeTaskRow(
                            address: existing.address,
                            title: existing.title,
                            liveStatus: existing.liveStatus,
                            executionId: existing.executionId,
                            pendingInteraction: existing.pendingInteraction,
                            modelConfig: response.modelConfig ?? existing.modelConfig
                        )
                    default:
                        break
                    }
                }
            }
        }
        if resolvedStatus == .unknown {
            pendingCommands[latchKey] = latched
        } else {
            pendingCommands.removeValue(forKey: latchKey)
            if let expectedTarget {
                pendingCommands.removeValue(forKey: sessionLatchKey(expectedTarget))
            }
            pendingCommands.removeValue(forKey: createLatchKey(expectedScope))
        }
        if [.stale, .denied, .unavailable].contains(resolvedStatus) {
            await fail(
                resolvedStatus == .denied ? ZCodeFailure.denied : resolvedStatus == .stale ? .stale : .unavailable,
                epoch: expectedEpoch
            )
            guard current(expectedEpoch) else { return resolvedStatus }
        }
        let finalReply = ZCodeCommandReply(
            instanceId: response.instanceId,
            generation: response.generation,
            status: resolvedStatus,
            selected: response.selected ?? selected,
            commandId: response.commandId ?? latched.commandId,
            duplicate: response.duplicate,
            delivery: response.delivery,
            executionId: response.executionId,
            interactionId: response.interactionId,
            interactionType: response.interactionType,
            modelConfig: response.modelConfig,
            reasonCode: response.reasonCode
        )
        lastCommandReply = finalReply
        lastCommandResult = resolvedStatus
        return resolvedStatus
    }
    private func failCommand(_ error: Error, epoch expected: UInt64) async -> ZCodeCommandResult {
        await fail(error, epoch: expected)
        guard current(expected) else { return .stale }
        let result: ZCodeCommandResult = error as? ZCodeFailure == .denied
            ? .denied
            : error as? ZCodeFailure == .stale ? .stale : .unavailable
        lastCommandResult = result
        return result
    }
}
