import Foundation

@MainActor
final class CindySessionController: ObservableObject {
    @Published private(set) var enabled: Bool
    @Published private(set) var state: CindyConnectionState = .disabled
    @Published private(set) var instanceId: String?
    @Published private(set) var generation: UInt64?
    @Published private(set) var ownerEpoch: String?
    @Published private(set) var scopes: [CindyScope] = []
    @Published private(set) var rows: [CindyTaskRow] = []
    @Published private(set) var delivered: CindyTaskAddress?
    @Published private(set) var selected: CindyTaskAddress?
    @Published private(set) var lastDeepLink: String?
    @Published private(set) var lastResult: CindyOpenResult?
    @Published private(set) var lastCommandReply: CindyCommandReply?
    @Published private(set) var pendingUnknownCommand: CindyQueryIntent?
    @Published private(set) var attentionRoundVisitedCount: Int = 0

    var onOpenConfirmed: ((CindyTaskRow) -> Void)?
    var onCommandCompleted: ((CindyCommandReply, CindyTaskRow?) -> Void)?

    private let client: any CindySessionServing
    private let pollNanoseconds: UInt64
    private var pollTask: Task<Void, Never>?
    private var localEpoch: UInt64 = 0
    private var suspended = false
    private var latchedCommandsByScope: [CindyScope: CindyQueryIntent] = [:]
    private var completedCommandReplies: [String: CindyCommandReply] = [:]
    private var attentionVisitedSignatures: [CindyTaskAddress: String] = [:]

    init(
        client: any CindySessionServing = CindySessionClient(),
        enabled: Bool = false,
        pollNanoseconds: UInt64 = 1_500_000_000
    ) {
        self.client = client
        self.enabled = enabled
        self.pollNanoseconds = pollNanoseconds
        if enabled {
            startPolling()
        }
    }

    deinit {
        pollTask?.cancel()
    }

    func setEnabled(_ value: Bool) {
        guard enabled != value else {
            if value, case .unavailable = state, !suspended {
                startPolling()
            }
            return
        }
        enabled = value
        localEpoch &+= 1
        if value {
            if !suspended {
                startPolling()
            }
        } else {
            stopAndClear(nextState: .disabled)
        }
    }

    func suspend() {
        guard !suspended else { return }
        suspended = true
        localEpoch &+= 1
        pollTask?.cancel()
        pollTask = nil
        let client = self.client
        Task {
            await client.close()
        }
    }

    func resume() {
        guard suspended else { return }
        suspended = false
        guard enabled else { return }
        startPolling()
    }

    func shutdown() async {
        suspended = true
        enabled = false
        localEpoch &+= 1
        pollTask?.cancel()
        pollTask = nil
        instanceId = nil
        generation = nil
        ownerEpoch = nil
        scopes = []
        rows = []
        delivered = nil
        selected = nil
        lastDeepLink = nil
        lastResult = nil
        lastCommandReply = nil
        pendingUnknownCommand = nil
        latchedCommandsByScope.removeAll()
        completedCommandReplies.removeAll()
        attentionVisitedSignatures.removeAll()
        attentionRoundVisitedCount = 0
        state = .disabled
        await client.close()
    }

    func resetAttentionRound() {
        attentionVisitedSignatures.removeAll()
        attentionRoundVisitedCount = 0
    }

    func nextAttentionCandidate(allowWrap: Bool = false) -> CindyTaskRow? {
        guard enabled, !suspended, state == .ready else {
            return nil
        }
        let eligible = rows.filter { row in
            scopes.contains(CindyScope(workspacePath: row.address.workspacePath)) &&
            row.isEligibleForAttentionRound
        }
        let sorted = eligible.enumerated().sorted { lhs, rhs in
            if lhs.element.attentionPriorityRank != rhs.element.attentionPriorityRank {
                return lhs.element.attentionPriorityRank < rhs.element.attentionPriorityRank
            }
            return lhs.offset < rhs.offset
        }.map(\.element)
        guard !sorted.isEmpty else {
            return nil
        }
        let candidateSet = Set(sorted.map(\.address))
        attentionVisitedSignatures = attentionVisitedSignatures.filter { candidateSet.contains($0.key) }
        if let next = sorted.first(where: { attentionVisitedSignatures[$0.address] != $0.roundWatermarkSignature }) {
            return next
        }
        if allowWrap {
            attentionVisitedSignatures.removeAll()
            attentionRoundVisitedCount = 0
            return sorted.first
        }
        return nil
    }

    func advanceAttentionRound(
        allowWrap: Bool = false,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        let cmdId = "round-\(UUID().uuidString)"
        guard enabled, !suspended, state == .ready,
              let instanceId, let generation, let ownerEpoch else {
            let reply = CindyCommandReply(
                commandId: cmdId,
                instanceId: self.instanceId ?? "",
                generation: self.generation ?? 0,
                ownerEpoch: self.ownerEpoch ?? "",
                status: .stale,
                reasonCode: "round.notReady"
            )
            lastCommandReply = reply
            return reply
        }
        guard admitted?() ?? true else {
            let reply = CindyCommandReply(
                commandId: cmdId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                reasonCode: "guard.notAdmitted"
            )
            lastCommandReply = reply
            return reply
        }
        guard let candidate = nextAttentionCandidate(allowWrap: allowWrap) else {
            let reply = CindyCommandReply(
                commandId: cmdId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .noop,
                selected: selected,
                reasonCode: "round.exhausted"
            )
            lastCommandReply = reply
            return reply
        }
        let openResult = await openSelection(
            CindySelection(
                address: candidate.address,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                localEpoch: localEpoch
            ),
            admitted: admitted
        )
        let mappedStatus: CindyCommandResult
        switch openResult {
        case .confirmed:
            attentionVisitedSignatures[candidate.address] = candidate.roundWatermarkSignature
            attentionRoundVisitedCount = attentionVisitedSignatures.count
            mappedStatus = .confirmed
        case .delivered:
            attentionVisitedSignatures[candidate.address] = candidate.roundWatermarkSignature
            attentionRoundVisitedCount = attentionVisitedSignatures.count
            mappedStatus = .delivered
        case .unconfirmed:
            mappedStatus = .unconfirmed
        case .stale:
            mappedStatus = .stale
        case .denied:
            mappedStatus = .denied
        case .unavailable:
            mappedStatus = .unavailable
        }
        let reply = CindyCommandReply(
            commandId: cmdId,
            instanceId: self.instanceId ?? instanceId,
            generation: self.generation ?? generation,
            ownerEpoch: self.ownerEpoch ?? ownerEpoch,
            status: mappedStatus,
            delivered: delivered,
            selected: selected,
            deepLink: lastDeepLink
        )
        lastCommandReply = reply
        if mappedStatus == .confirmed {
            onCommandCompleted?(reply, candidate)
        }
        return reply
    }

    func execute(
        _ command: CindyQueuedCommand,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        switch command {
        case .create(let scope, let text, let commandId):
            return await create(
                scope: scope,
                text: text,
                commandId: commandId ?? UUID().uuidString,
                admitted: admitted
            )
        case .send(let address, let text, let deliveryMode, let commandId):
            return await send(
                address: address,
                text: text,
                deliveryMode: deliveryMode,
                commandId: commandId ?? UUID().uuidString,
                admitted: admitted
            )
        case .stop(let address, let expectedRunId, let commandId):
            return await stop(
                address: address,
                expectedRunId: expectedRunId,
                commandId: commandId ?? UUID().uuidString,
                admitted: admitted
            )
        case .resolve(let address, let interactionId, let interactionType, let decision, let optionId, let commandId):
            return await resolve(
                address: address,
                interactionId: interactionId,
                interactionType: interactionType,
                decision: decision,
                optionId: optionId,
                commandId: commandId ?? UUID().uuidString,
                admitted: admitted
            )
        case .query(let scope, let target, let commandId):
            return await query(
                scope: scope,
                target: target,
                commandId: commandId,
                admitted: admitted
            )
        case .nextAttention:
            return await advanceAttentionRound(allowWrap: false, admitted: admitted)
        }
    }

    func create(
        scope: CindyScope,
        text: String? = nil,
        commandId: String = UUID().uuidString,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        guard enabled, !suspended, state == .ready,
              let instanceId, let generation, let ownerEpoch,
              admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: self.instanceId ?? "",
                    generation: self.generation ?? 0,
                    ownerEpoch: self.ownerEpoch ?? "",
                    status: .stale,
                    reasonCode: "guard.notReady"
                )
            )
        }
        guard scopes.contains(scope), scope.isValid else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .denied,
                    reasonCode: "guard.scopeOutOfBounds"
                )
            )
        }
        let cmdKey = "\(scope.workspacePath)::\(commandId)"
        if let cached = completedCommandReplies[cmdKey] {
            var dup = cached
            dup.duplicate = true
            lastCommandReply = dup
            return dup
        }
        if let latched = latchedCommandsByScope[scope], latched.commandId != commandId {
            return recordReply(
                CindyCommandReply(
                    commandId: latched.commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .unknown,
                    reasonCode: "guard.unknownOutcomeRequiresReconcile"
                )
            )
        }
        let epoch = localEpoch
        let intent = CindyCreateIntent(
            commandId: commandId,
            scope: scope,
            text: text,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            localEpoch: epoch
        )
        let response = await client.create(intent)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    reasonCode: "guard.epochChanged"
                )
            )
        }
        return handleCommandResponse(
            response,
            scope: scope,
            target: response.selected ?? response.delivered,
            commandKey: cmdKey
        )
    }

    func send(
        address: CindyTaskAddress,
        text: String,
        deliveryMode: CindyDeliveryMode = .auto,
        commandId: String = UUID().uuidString,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        let scope = CindyScope(workspacePath: address.workspacePath)
        let cmdKey = "\(scope.workspacePath)::\(commandId)"
        if let cached = completedCommandReplies[cmdKey] {
            var dup = cached
            dup.duplicate = true
            lastCommandReply = dup
            return dup
        }
        guard let validated = validateTargetCommand(
            address: address,
            scope: scope,
            commandId: commandId,
            admitted: admitted
        ) else {
            return lastCommandReply!
        }
        let (selection, row) = validated
        if deliveryMode == .steer, row.phase != "running" {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: selection.instanceId,
                    generation: selection.generation,
                    ownerEpoch: selection.ownerEpoch,
                    status: .denied,
                    selected: selected,
                    reasonCode: "guard.steerRequiresRunningTurn"
                )
            )
        }
        let epoch = localEpoch
        let intent = CindySendIntent(
            commandId: commandId,
            selection: selection,
            text: text,
            deliveryMode: deliveryMode
        )
        let response = await client.send(intent)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: selection.instanceId,
                    generation: selection.generation,
                    ownerEpoch: selection.ownerEpoch,
                    status: .stale,
                    reasonCode: "guard.epochChanged"
                )
            )
        }
        return handleCommandResponse(response, scope: scope, target: address, commandKey: cmdKey)
    }

    func stop(
        address: CindyTaskAddress,
        expectedRunId: String,
        commandId: String = UUID().uuidString,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        let scope = CindyScope(workspacePath: address.workspacePath)
        let cmdKey = "\(scope.workspacePath)::\(commandId)"
        if let cached = completedCommandReplies[cmdKey] {
            var dup = cached
            dup.duplicate = true
            lastCommandReply = dup
            return dup
        }
        guard let validated = validateTargetCommand(
            address: address,
            scope: scope,
            commandId: commandId,
            admitted: admitted
        ) else {
            return lastCommandReply!
        }
        let (selection, row) = validated
        if row.phase != "running" || (row.runId != nil && row.runId != expectedRunId) {
            let noopReply = CindyCommandReply(
                commandId: commandId,
                instanceId: selection.instanceId,
                generation: selection.generation,
                ownerEpoch: selection.ownerEpoch,
                status: .noop,
                selected: address,
                runId: row.runId,
                reasonCode: row.phase != "running" ? "guard.notRunning" : "guard.stopTargetChanged"
            )
            completedCommandReplies[cmdKey] = noopReply
            return recordReply(noopReply)
        }
        let epoch = localEpoch
        let intent = CindyStopIntent(
            commandId: commandId,
            selection: selection,
            expectedRunId: expectedRunId
        )
        let response = await client.stop(intent)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: selection.instanceId,
                    generation: selection.generation,
                    ownerEpoch: selection.ownerEpoch,
                    status: .stale,
                    reasonCode: "guard.epochChanged"
                )
            )
        }
        return handleCommandResponse(response, scope: scope, target: address, commandKey: cmdKey)
    }

    func resolve(
        address: CindyTaskAddress,
        interactionId: String,
        interactionType: CindyInteractionType,
        decision: CindyInteractionDecision,
        optionId: String? = nil,
        commandId: String = UUID().uuidString,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        let scope = CindyScope(workspacePath: address.workspacePath)
        let cmdKey = "\(scope.workspacePath)::\(commandId)"
        if let cached = completedCommandReplies[cmdKey] {
            var dup = cached
            dup.duplicate = true
            lastCommandReply = dup
            return dup
        }
        if interactionType == .question,
           (decision == .approve || decision == .allowForSession),
           (optionId == nil || optionId?.isEmpty == true) {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId ?? "",
                    generation: generation ?? 0,
                    ownerEpoch: ownerEpoch ?? "",
                    status: .interactionTypeMismatch,
                    selected: selected,
                    interactionId: interactionId,
                    interactionType: interactionType,
                    reasonCode: "guard.questionCannotApproveAsPermission"
                )
            )
        }
        guard let validated = validateTargetCommand(
            address: address,
            scope: scope,
            commandId: commandId,
            admitted: admitted
        ) else {
            return lastCommandReply!
        }
        let (selection, row) = validated
        if let pending = row.pendingInteraction {
            if pending.interactionId != interactionId {
                let noopReply = CindyCommandReply(
                    commandId: commandId,
                    instanceId: selection.instanceId,
                    generation: selection.generation,
                    ownerEpoch: selection.ownerEpoch,
                    status: .noop,
                    selected: address,
                    interactionId: interactionId,
                    interactionType: interactionType,
                    reasonCode: "guard.interactionAlreadyResolved"
                )
                completedCommandReplies[cmdKey] = noopReply
                return recordReply(noopReply)
            }
            if pending.interactionType != interactionType {
                return recordReply(
                    CindyCommandReply(
                        commandId: commandId,
                        instanceId: selection.instanceId,
                        generation: selection.generation,
                        ownerEpoch: selection.ownerEpoch,
                        status: .interactionTypeMismatch,
                        selected: address,
                        interactionId: pending.interactionId,
                        interactionType: pending.interactionType,
                        reasonCode: "guard.interactionTypeMismatch"
                    )
                )
            }
        }
        let epoch = localEpoch
        let intent = CindyResolveIntent(
            commandId: commandId,
            selection: selection,
            interactionId: interactionId,
            interactionType: interactionType,
            decision: decision,
            optionId: optionId
        )
        let response = await client.resolve(intent)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: selection.instanceId,
                    generation: selection.generation,
                    ownerEpoch: selection.ownerEpoch,
                    status: .stale,
                    reasonCode: "guard.epochChanged"
                )
            )
        }
        return handleCommandResponse(response, scope: scope, target: address, commandKey: cmdKey)
    }

    func query(
        scope: CindyScope,
        target: CindyTaskAddress? = nil,
        commandId: String,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyCommandReply {
        guard enabled, !suspended, state == .ready,
              let instanceId, let generation, let ownerEpoch,
              admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: self.instanceId ?? "",
                    generation: self.generation ?? 0,
                    ownerEpoch: self.ownerEpoch ?? "",
                    status: .stale,
                    reasonCode: "guard.notReady"
                )
            )
        }
        let epoch = localEpoch
        let intent = CindyQueryIntent(
            commandId: commandId,
            scope: scope,
            target: target,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            localEpoch: epoch
        )
        let response = await client.query(intent)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            return recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    reasonCode: "guard.epochChanged"
                )
            )
        }
        let cmdKey = "\(scope.workspacePath)::\(commandId)"
        return handleCommandResponse(response, scope: scope, target: target, commandKey: cmdKey)
    }

    private func validateTargetCommand(
        address: CindyTaskAddress,
        scope: CindyScope,
        commandId: String,
        admitted: (@MainActor () -> Bool)?
    ) -> (CindySelection, CindyTaskRow)? {
        guard enabled, !suspended, state == .ready,
              let instanceId, let generation, let ownerEpoch,
              admitted?() ?? true else {
            _ = recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: self.instanceId ?? "",
                    generation: self.generation ?? 0,
                    ownerEpoch: self.ownerEpoch ?? "",
                    status: .stale,
                    reasonCode: "guard.notReady"
                )
            )
            return nil
        }
        guard scopes.contains(scope), address.isValid else {
            _ = recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .denied,
                    reasonCode: "guard.scopeOutOfBounds"
                )
            )
            return nil
        }
        if let latched = latchedCommandsByScope[scope], latched.commandId != commandId {
            _ = recordReply(
                CindyCommandReply(
                    commandId: latched.commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .unknown,
                    reasonCode: "guard.unknownOutcomeRequiresReconcile"
                )
            )
            return nil
        }
        guard let row = rows.first(where: { $0.address == address }) else {
            _ = recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    reasonCode: "guard.targetMissing"
                )
            )
            return nil
        }
        guard selected == address else {
            _ = recordReply(
                CindyCommandReply(
                    commandId: commandId,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .unconfirmed,
                    selected: selected,
                    reasonCode: "guard.selectionNotCommitted"
                )
            )
            return nil
        }
        let sel = CindySelection(
            address: address,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            localEpoch: localEpoch
        )
        return (sel, row)
    }

    private func handleCommandResponse(
        _ response: CindyCommandReply,
        scope: CindyScope,
        target: CindyTaskAddress?,
        commandKey: String
    ) -> CindyCommandReply {
        if response.status == .unknown {
            if let instanceId, let generation, let ownerEpoch {
                let queryIntent = CindyQueryIntent(
                    commandId: response.commandId,
                    scope: scope,
                    target: target,
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    localEpoch: localEpoch
                )
                latchedCommandsByScope[scope] = queryIntent
                pendingUnknownCommand = queryIntent
            }
            return recordReply(response)
        }
        if response.instanceId != instanceId ||
            response.generation != generation ||
            response.ownerEpoch != ownerEpoch {
            invalidate(with: .stale)
            return recordReply(
                CindyCommandReply(
                    commandId: response.commandId,
                    instanceId: response.instanceId,
                    generation: response.generation,
                    ownerEpoch: response.ownerEpoch,
                    status: .stale,
                    reasonCode: response.reasonCode ?? "guard.epochRotated"
                )
            )
        }
        if latchedCommandsByScope[scope]?.commandId == response.commandId {
            latchedCommandsByScope.removeValue(forKey: scope)
            pendingUnknownCommand = latchedCommandsByScope.values.first
        }
        switch response.status {
        case .confirmed:
            guard let createdOrTarget = response.selected ?? target,
                  response.delivered == createdOrTarget,
                  response.selected == createdOrTarget else {
                var unconfirmed = response
                unconfirmed.status = .unconfirmed
                return recordReply(unconfirmed)
            }
            delivered = createdOrTarget
            selected = createdOrTarget
            lastDeepLink = response.deepLink ?? createdOrTarget.deepLink
            lastResult = .confirmed
            if !rows.contains(where: { $0.address == createdOrTarget }) {
                rows.append(
                    CindyTaskRow(
                        address: createdOrTarget,
                        title: createdOrTarget.sessionId,
                        phase: response.runId != nil ? "running" : "idle",
                        attention: false,
                        pinned: false,
                        runId: response.runId,
                        unread: false
                    )
                )
            }
            completedCommandReplies[commandKey] = response
        case .delivered:
            delivered = response.delivered
            selected = nil
            lastDeepLink = response.deepLink
            lastResult = .delivered
            completedCommandReplies[commandKey] = response
        case .started, .steered, .queued:
            if let target, let idx = rows.firstIndex(where: { $0.address == target }) {
                rows[idx].phase = "running"
                if let runId = response.runId {
                    rows[idx].runId = runId
                }
                rows[idx].queuedCount = response.queuedCount
            }
            completedCommandReplies[commandKey] = response
        case .stopped:
            if let target, let idx = rows.firstIndex(where: { $0.address == target }) {
                rows[idx].phase = "idle"
                rows[idx].runId = nil
            }
            completedCommandReplies[commandKey] = response
        case .resolved:
            if let target, let idx = rows.firstIndex(where: { $0.address == target }) {
                rows[idx].pendingInteraction = nil
                rows[idx].attention = false
                if rows[idx].phase == "needs-interaction" {
                    rows[idx].phase = "running"
                }
            }
            completedCommandReplies[commandKey] = response
        case .noop:
            completedCommandReplies[commandKey] = response
        case .stale:
            invalidate(with: .stale)
        case .denied, .interactionTypeMismatch, .unconfirmed:
            break
        case .unavailable:
            invalidate(with: .unavailable)
        case .unknown:
            break
        }
        let recorded = recordReply(response)
        let matchingRow = (response.selected ?? target).flatMap { addr in
            rows.first(where: { $0.address == addr })
        }
        onCommandCompleted?(recorded, matchingRow)
        return recorded
    }

    private func recordReply(_ reply: CindyCommandReply) -> CindyCommandReply {
        lastCommandReply = reply
        return reply
    }

    func refreshNow() async {
        guard enabled, !suspended else { return }
        let epoch = localEpoch
        let result: Result<CindySessionSnapshot, CindyFailure>
        if state == .ready {
            result = await client.read()
        } else {
            state = .connecting
            result = await client.connect()
        }
        guard enabled, !suspended, epoch == localEpoch else { return }
        apply(result)
    }

    func selection(for address: CindyTaskAddress) -> CindySelection? {
        guard enabled,
              !suspended,
              state == .ready,
              let instanceId,
              let generation,
              let ownerEpoch,
              rows.contains(where: { $0.address == address }) else {
            return nil
        }
        return CindySelection(
            address: address,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            localEpoch: localEpoch
        )
    }

    func open(_ address: CindyTaskAddress) async -> CindyOpenResult {
        guard let selection = selection(for: address) else {
            lastResult = .stale
            return .stale
        }
        return await openSelection(selection)
    }

    func open(_ selection: CindySelection?, admitted: @escaping @MainActor () -> Bool) async -> CindyOpenResult {
        guard let selection, admitted() else {
            lastResult = .stale
            return .stale
        }
        return await openSelection(selection, admitted: admitted)
    }

    func openSelection(
        _ selection: CindySelection,
        admitted: (@MainActor () -> Bool)? = nil
    ) async -> CindyOpenResult {
        guard enabled,
              !suspended,
              state == .ready,
              selection.localEpoch == localEpoch,
              selection.instanceId == instanceId,
              selection.generation == generation,
              selection.ownerEpoch == ownerEpoch,
              scopes.contains(CindyScope(workspacePath: selection.address.workspacePath)),
              let row = rows.first(where: { $0.address == selection.address }),
              admitted?() ?? true else {
            if enabled, !suspended, state == .ready {
                invalidate(with: .stale)
            }
            lastResult = .stale
            return .stale
        }

        let epoch = localEpoch
        let response = await client.open(selection)
        guard enabled, !suspended, epoch == localEpoch, admitted?() ?? true else {
            lastResult = .stale
            return .stale
        }

        if response.instanceId != instanceId ||
            response.generation != generation ||
            response.ownerEpoch != ownerEpoch {
            invalidate(with: .stale)
            lastResult = .stale
            return .stale
        }

        switch response.status {
        case .confirmed:
            guard response.delivered == selection.address,
                  response.selected == selection.address,
                  response.deepLink == selection.address.deepLink else {
                delivered = response.delivered
                selected = nil
                lastDeepLink = response.deepLink
                lastResult = .unconfirmed
                return .unconfirmed
            }
            delivered = selection.address
            selected = selection.address
            lastDeepLink = response.deepLink
            lastResult = .confirmed
            onOpenConfirmed?(row)
            return .confirmed
        case .delivered:
            delivered = selection.address
            selected = nil
            lastDeepLink = response.deepLink
            lastResult = .delivered
            return .delivered
        case .unconfirmed:
            delivered = response.delivered
            selected = nil
            lastDeepLink = response.deepLink
            lastResult = .unconfirmed
            return .unconfirmed
        case .stale:
            invalidate(with: .stale)
            lastResult = .stale
            return .stale
        case .denied:
            invalidate(with: .denied)
            lastResult = .denied
            return .denied
        case .unavailable:
            invalidate(with: .unavailable)
            lastResult = .unavailable
            return .unavailable
        }
    }

    private func startPolling() {
        pollTask?.cancel()
        localEpoch &+= 1
        let epoch = localEpoch
        let interval = pollNanoseconds
        state = .connecting
        pollTask = Task { [weak self] in
            guard let self else { return }
            await self.pollStep(expectedEpoch: epoch, initial: true)
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: interval)
                if Task.isCancelled { break }
                await self.pollStep(expectedEpoch: epoch, initial: false)
            }
        }
    }

    private func pollStep(expectedEpoch: UInt64, initial: Bool) async {
        guard enabled, !suspended, expectedEpoch == localEpoch else { return }
        let result: Result<CindySessionSnapshot, CindyFailure>
        if initial || state != .ready {
            result = await client.connect()
        } else {
            result = await client.read()
        }
        guard enabled, !suspended, expectedEpoch == localEpoch else { return }
        apply(result)
    }

    private func apply(_ result: Result<CindySessionSnapshot, CindyFailure>) {
        switch result {
        case .success(let snapshot):
            let identityChanged =
                instanceId != nil &&
                (instanceId != snapshot.instanceId ||
                 generation != snapshot.generation ||
                 ownerEpoch != snapshot.ownerEpoch)
            if identityChanged {
                localEpoch &+= 1
                latchedCommandsByScope.removeAll()
                completedCommandReplies.removeAll()
                attentionVisitedSignatures.removeAll()
                pendingUnknownCommand = nil
                attentionRoundVisitedCount = 0
            }
            instanceId = snapshot.instanceId
            generation = snapshot.generation
            ownerEpoch = snapshot.ownerEpoch
            scopes = snapshot.scopes
            rows = snapshot.rows
            if let snapshotSelected = snapshot.selected,
               snapshot.rows.contains(where: { $0.address == snapshotSelected }) {
                selected = snapshotSelected
            } else {
                selected = nil
            }
            if let currentDelivered = delivered,
               !snapshot.rows.contains(where: { $0.address == currentDelivered }) {
                delivered = nil
            }
            state = .ready
        case .failure(let failure):
            invalidate(with: failure)
        }
    }

    private func invalidate(with failure: CindyFailure) {
        localEpoch &+= 1
        instanceId = nil
        generation = nil
        ownerEpoch = nil
        scopes = []
        rows = []
        delivered = nil
        selected = nil
        lastDeepLink = nil
        pendingUnknownCommand = nil
        latchedCommandsByScope.removeAll()
        completedCommandReplies.removeAll()
        attentionVisitedSignatures.removeAll()
        attentionRoundVisitedCount = 0
        state = .unavailable(failure)
    }

    private func stopAndClear(nextState: CindyConnectionState) {
        localEpoch &+= 1
        pollTask?.cancel()
        pollTask = nil
        instanceId = nil
        generation = nil
        ownerEpoch = nil
        scopes = []
        rows = []
        delivered = nil
        selected = nil
        lastDeepLink = nil
        lastResult = nil
        lastCommandReply = nil
        pendingUnknownCommand = nil
        latchedCommandsByScope.removeAll()
        completedCommandReplies.removeAll()
        attentionVisitedSignatures.removeAll()
        attentionRoundVisitedCount = 0
        state = nextState
        let client = self.client
        Task {
            await client.close()
        }
    }
}
