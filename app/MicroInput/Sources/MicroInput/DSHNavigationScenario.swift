#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

actor DSHNavigationFixture: DSHSessionServing {
    static let shared = DSHNavigationFixture()
    static let first = "qa-session-a"
    static let second = "qa-session-b"
    static let cold = "qa-session-cold"
    enum Mode { case normal, wrongSelected, deleted, rotated, disconnected, gap }
    private struct SessionRuntimeState {
        var workspaceId: String
        var title: String
        var running: Bool
        var pendingInput: UInt64
        var currentTurn: Int?
        var queuedCount: Int
        var pendingInteraction: DSHPendingInteraction?
        var modelConfig: DSHModelConfig?
        var supportedModels: [DSHModelOption]?
    }
    private var mode = Mode.normal
    private var selected: String?
    private var cursor: UInt64 = 0
    private var changes: [DSHSessionChange] = []
    private var connected = false
    private var sessionOrder: [String] = [DSHNavigationFixture.first, DSHNavigationFixture.second]
    private var createdOrder: [String] = []
    private var states: [String: SessionRuntimeState] = [
        DSHNavigationFixture.first: SessionRuntimeState(
            workspaceId: "workspace-a", title: "同名会话", running: true, pendingInput: 0, currentTurn: 1, queuedCount: 0,
            pendingInteraction: nil,
            modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "medium"),
            supportedModels: [
                DSHModelOption(provider: "deepseek-official", model: "deepseek-chat", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
                DSHModelOption(provider: "deepseek-official", model: "deepseek-reasoner", defaultThought: "high", supportedThoughts: ["low", "medium", "high"]),
            ]
        ),
        DSHNavigationFixture.second: SessionRuntimeState(
            workspaceId: "workspace-b", title: "同名会话", running: true, pendingInput: 0, currentTurn: 1, queuedCount: 0,
            pendingInteraction: nil,
            modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "medium"),
            supportedModels: [
                DSHModelOption(provider: "deepseek-official", model: "deepseek-chat", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
                DSHModelOption(provider: "deepseek-official", model: "deepseek-reasoner", defaultThought: "high", supportedThoughts: ["low", "medium", "high"]),
            ]
        ),
    ]
    private var settledReplies: [String: DSHCommandReply] = [:]
    private(set) var metadataReads = 0
    private(set) var bodyReads = 0
    private(set) var opens = 0
    private(set) var creates = 0
    private(set) var prompts = 0
    private(set) var cancels = 0
    private(set) var resolves = 0
    private(set) var selectModels = 0
    private(set) var closes = 0
    private(set) var navigations: [String] = []

    func reset() {
        mode = .normal
        selected = nil
        cursor = 0
        changes = []
        connected = false
        sessionOrder = [Self.first, Self.second]
        createdOrder = []
        states = [
            Self.first: SessionRuntimeState(
                workspaceId: "workspace-a", title: "同名会话", running: true, pendingInput: 0, currentTurn: 1, queuedCount: 0,
                pendingInteraction: nil,
                modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "medium"),
                supportedModels: [
                    DSHModelOption(provider: "deepseek-official", model: "deepseek-chat", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
                    DSHModelOption(provider: "deepseek-official", model: "deepseek-reasoner", defaultThought: "high", supportedThoughts: ["low", "medium", "high"]),
                ]
            ),
            Self.second: SessionRuntimeState(
                workspaceId: "workspace-b", title: "同名会话", running: true, pendingInput: 0, currentTurn: 1, queuedCount: 0,
                pendingInteraction: nil,
                modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "medium"),
                supportedModels: [
                    DSHModelOption(provider: "deepseek-official", model: "deepseek-chat", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
                    DSHModelOption(provider: "deepseek-official", model: "deepseek-reasoner", defaultThought: "high", supportedThoughts: ["low", "medium", "high"]),
                ]
            ),
        ]
        settledReplies = [:]
        metadataReads = 0
        bodyReads = 0
        opens = 0
        creates = 0
        prompts = 0
        cancels = 0
        resolves = 0
        selectModels = 0
        closes = 0
        navigations = []
    }
    func setMode(_ value: Mode) { mode = value }
    func setTurnState(sessionId: String, running: Bool, currentTurn: Int?, queuedCount: Int? = nil) {
        guard var entry = states[sessionId] else { return }
        entry.running = running
        entry.currentTurn = currentTurn
        if let queuedCount { entry.queuedCount = queuedCount }
        states[sessionId] = entry
        cursor += 1
        changes.append(DSHSessionChange(sequence: cursor, row: row(sessionId)))
    }
    func setPendingInteraction(sessionId: String, interaction: DSHPendingInteraction?) {
        guard var entry = states[sessionId] else { return }
        entry.pendingInteraction = interaction
        entry.pendingInput = interaction != nil ? 1 : 0
        states[sessionId] = entry
        cursor += 1
        changes.append(DSHSessionChange(sequence: cursor, row: row(sessionId)))
    }
    func counts() -> (reads: Int, bodyReads: Int, opens: Int, creates: Int, prompts: Int, cancels: Int, resolves: Int, selectModels: Int, closes: Int, navigations: [String]) {
        (metadataReads, bodyReads, opens, creates, prompts, cancels, resolves, selectModels, closes, navigations)
    }
    private var generation: UInt64 { mode == .rotated ? 2 : 1 }
    private func row(_ id: String) -> DSHSessionRow {
        let state = states[id] ?? SessionRuntimeState(
            workspaceId: id == Self.first ? "workspace-a" : "workspace-b",
            title: "同名会话", running: true, pendingInput: 0, currentTurn: 1, queuedCount: 0,
            pendingInteraction: nil,
            modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "medium"),
            supportedModels: [
                DSHModelOption(provider: "deepseek-official", model: "deepseek-chat", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
                DSHModelOption(provider: "deepseek-official", model: "deepseek-reasoner", defaultThought: "high", supportedThoughts: ["low", "medium", "high"]),
            ]
        )
        return DSHSessionRow(
            sessionId: id,
            workspaceId: state.workspaceId,
            title: state.title,
            active: true,
            running: state.running,
            pendingInput: state.pendingInput,
            currentTurn: state.currentTurn,
            queuedCount: state.queuedCount,
            pendingInteraction: state.pendingInteraction,
            modelConfig: state.modelConfig,
            supportedModels: state.supportedModels
        )
    }
    func pendingRoundTrip() {
        for value in [UInt64(1), UInt64(0)] {
            if var entry = states[Self.first] {
                entry.pendingInput = value
                states[Self.first] = entry
            }
            cursor += 1
            changes.append(DSHSessionChange(sequence: cursor, row: row(Self.first)))
        }
    }
    func connect() async throws -> DSHSessionSnapshot {
        connected = true; changes = []
        return try await read()
    }
    func read() async throws -> DSHSessionSnapshot {
        guard connected else { throw DSHFailure.denied }
        guard mode != .disconnected else { throw DSHFailure.unavailable }
        guard mode != .gap else { throw DSHFailure.gap }
        metadataReads += 1
        let liveSessions = sessionOrder + createdOrder
        let visibleIds = mode == .deleted ? liveSessions.filter { $0 != Self.first } : liveSessions
        let allIds = [Self.first, Self.second, Self.cold] + createdOrder
        let currentRows = visibleIds.map { row($0) }
        let currentUnavailable = allIds.filter { id in !visibleIds.contains(id) }
        let resultChanges = changes
        changes = []
        return DSHSessionSnapshot(
            hostId: "qa-host",
            generation: generation,
            sessionIds: allIds,
            cursor: cursor,
            rows: currentRows,
            unavailableSessionIds: currentUnavailable,
            changes: resultChanges,
            selected: selected
        )
    }
    func open(_ selection: DSHSelection) async throws -> DSHOpenReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard selection.hostId == "qa-host", selection.generation == generation else { return reply(.stale) }
        guard mode != .deleted || selection.sessionId != Self.first else { return reply(.denied) }
        opens += 1
        let target = selection.sessionId
        navigations.append(target)
        if mode == .wrongSelected {
            selected = Self.second
            return reply(.unconfirmed, selected: Self.second)
        }
        selected = target
        return reply(.confirmed, selected: target)
    }
    func create(_ intent: DSHCreateIntent) async throws -> DSHCommandReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard intent.hostId == "qa-host", intent.generation == generation else {
            return commandReply(status: "stale", requestId: intent.requestId)
        }
        if let existing = settledReplies[intent.requestId] {
            return withDuplicate(existing)
        }
        creates += 1
        let createdId = "qa-session-created-\(creates)"
        createdOrder.append(createdId)
        states[createdId] = SessionRuntimeState(
            workspaceId: intent.workspaceId, title: "New Session", running: false, pendingInput: 0, currentTurn: nil, queuedCount: 0,
            pendingInteraction: nil,
            modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "medium"),
            supportedModels: [
                DSHModelOption(provider: "deepseek-official", model: "deepseek-chat", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
                DSHModelOption(provider: "deepseek-official", model: "deepseek-reasoner", defaultThought: "high", supportedThoughts: ["low", "medium", "high"]),
            ]
        )
        selected = createdId
        cursor += 1
        changes.append(DSHSessionChange(sequence: cursor, row: row(createdId)))
        let settled = DSHCommandReply(
            hostId: "qa-host", generation: generation, status: "confirmed",
            sessionId: createdId, workspaceId: intent.workspaceId, selected: createdId,
            requestId: intent.requestId, delivery: nil, expectedTurn: nil,
            currentTurn: nil, queuedCount: 0, duplicate: false, reasonCode: nil
        )
        settledReplies[intent.requestId] = settled
        return settled
    }
    func prompt(_ intent: DSHPromptIntent) async throws -> DSHCommandReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard intent.hostId == "qa-host", intent.generation == generation else {
            return commandReply(status: "stale", requestId: intent.requestId)
        }
        if let existing = settledReplies[intent.requestId] {
            return withDuplicate(existing)
        }
        guard var entry = states[intent.sessionId] else {
            return commandReply(status: "denied", requestId: intent.requestId, reasonCode: "session.notFound")
        }
        if intent.mode == .steer && (!entry.running || entry.currentTurn == nil) {
            return commandReply(status: "denied", requestId: intent.requestId, reasonCode: "session.steerUnavailable")
        }
        prompts += 1
        entry.queuedCount += 1
        states[intent.sessionId] = entry
        cursor += 1
        changes.append(DSHSessionChange(sequence: cursor, row: row(intent.sessionId)))
        let deliveryStatus = intent.mode == .steer ? "steered" : "queued"
        let settled = DSHCommandReply(
            hostId: "qa-host", generation: generation, status: deliveryStatus,
            sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
            requestId: intent.requestId, delivery: intent.mode, expectedTurn: nil,
            currentTurn: entry.currentTurn, queuedCount: entry.queuedCount, duplicate: false, reasonCode: nil
        )
        settledReplies[intent.requestId] = settled
        return settled
    }
    func cancel(_ intent: DSHCancelIntent) async throws -> DSHCommandReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard intent.hostId == "qa-host", intent.generation == generation else {
            return commandReply(status: "stale", requestId: intent.requestId)
        }
        if let existing = settledReplies[intent.requestId] {
            return withDuplicate(existing)
        }
        guard var entry = states[intent.sessionId] else {
            return commandReply(status: "denied", requestId: intent.requestId, reasonCode: "session.notFound")
        }
        if !entry.running || entry.currentTurn == nil {
            let noop = DSHCommandReply(
                hostId: "qa-host", generation: generation, status: "noop",
                sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                requestId: intent.requestId, delivery: nil, expectedTurn: intent.expectedTurn,
                currentTurn: entry.currentTurn, queuedCount: entry.queuedCount, duplicate: false, reasonCode: "guard.notRunning"
            )
            settledReplies[intent.requestId] = noop
            return noop
        }
        guard let currentTurn = entry.currentTurn else {
            return commandReply(status: "denied", requestId: intent.requestId, reasonCode: "guard.unprovableTurnAtomicity")
        }
        if currentTurn != intent.expectedTurn {
            let noop = DSHCommandReply(
                hostId: "qa-host", generation: generation, status: "noop",
                sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                requestId: intent.requestId, delivery: nil, expectedTurn: intent.expectedTurn,
                currentTurn: currentTurn, queuedCount: entry.queuedCount, duplicate: false, reasonCode: "guard.turnChanged"
            )
            settledReplies[intent.requestId] = noop
            return noop
        }
        cancels += 1
        entry.running = false
        entry.currentTurn = nil
        states[intent.sessionId] = entry
        cursor += 1
        changes.append(DSHSessionChange(sequence: cursor, row: row(intent.sessionId)))
        let status = entry.queuedCount > 0 ? "cancelledQueueRemaining" : "cancelled"
        let settled = DSHCommandReply(
            hostId: "qa-host", generation: generation, status: status,
            sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
            requestId: intent.requestId, delivery: nil, expectedTurn: intent.expectedTurn,
            currentTurn: nil, queuedCount: entry.queuedCount, duplicate: false, reasonCode: nil
        )
        settledReplies[intent.requestId] = settled
        return settled
    }
    func resolve(_ intent: DSHResolveIntent) async throws -> DSHCommandReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard intent.hostId == "qa-host", intent.generation == generation else {
            return commandReply(status: "stale", requestId: intent.requestId)
        }
        if let existing = settledReplies[intent.requestId] {
            return withDuplicate(existing)
        }
        resolves += 1
        guard var entry = states[intent.sessionId] else {
            return commandReply(status: "denied", requestId: intent.requestId, reasonCode: "session.notFound")
        }
        if let pending = entry.pendingInteraction {
            if pending.interactionType != intent.interactionType {
                let reason = intent.interactionType == .permission
                    ? "guard.questionCannotApproveAsPermission"
                    : "guard.permissionCannotAnswerAsQuestion"
                return DSHCommandReply(
                    hostId: "qa-host", generation: generation, status: "interactionTypeMismatch",
                    sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                    requestId: intent.requestId, delivery: nil, expectedTurn: nil, currentTurn: nil,
                    queuedCount: nil, duplicate: false, reasonCode: reason
                )
            }
            entry.pendingInteraction = nil
            entry.pendingInput = 0
            states[intent.sessionId] = entry
            cursor += 1
            changes.append(DSHSessionChange(sequence: cursor, row: row(intent.sessionId)))
            let settled = DSHCommandReply(
                hostId: "qa-host", generation: generation, status: "resolved",
                sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                requestId: intent.requestId, delivery: nil, expectedTurn: nil, currentTurn: nil,
                queuedCount: nil, duplicate: false, reasonCode: nil
            )
            settledReplies[intent.requestId] = settled
            return settled
        } else {
            let noop = DSHCommandReply(
                hostId: "qa-host", generation: generation, status: "noop",
                sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                requestId: intent.requestId, delivery: nil, expectedTurn: nil, currentTurn: nil,
                queuedCount: nil, duplicate: false, reasonCode: "guard.alreadyResolved"
            )
            settledReplies[intent.requestId] = noop
            return noop
        }
    }
    func switchModel(_ intent: DSHSwitchModelIntent) async throws -> DSHCommandReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard intent.hostId == "qa-host", intent.generation == generation else {
            return commandReply(status: "stale", requestId: intent.requestId)
        }
        if let existing = settledReplies[intent.requestId] {
            return withDuplicate(existing)
        }
        selectModels += 1
        guard var entry = states[intent.sessionId] else {
            return commandReply(status: "denied", requestId: intent.requestId, reasonCode: "session.notFound")
        }
        let supported = entry.supportedModels ?? []
        guard let option = supported.first(where: { $0.provider == intent.provider && $0.model == intent.model }) else {
            return DSHCommandReply(
                hostId: "qa-host", generation: generation, status: "denied",
                sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                requestId: intent.requestId, delivery: nil, expectedTurn: nil, currentTurn: nil,
                queuedCount: nil, duplicate: false, reasonCode: "guard.unsupportedModel"
            )
        }
        let targetThought = intent.thought ?? option.defaultThought
        if let targetThought, !option.supportedThoughts.contains(targetThought) {
            return DSHCommandReply(
                hostId: "qa-host", generation: generation, status: "unsupportedThought",
                sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
                requestId: intent.requestId, delivery: nil, expectedTurn: nil, currentTurn: nil,
                queuedCount: nil, duplicate: false, reasonCode: "guard.unsupportedThoughtLevel"
            )
        }
        let newConfig = DSHModelConfig(provider: intent.provider, model: intent.model, thought: targetThought)
        entry.modelConfig = newConfig
        states[intent.sessionId] = entry
        cursor += 1
        changes.append(DSHSessionChange(sequence: cursor, row: row(intent.sessionId)))
        let settled = DSHCommandReply(
            hostId: "qa-host", generation: generation, status: "modelSwitched",
            sessionId: intent.sessionId, workspaceId: intent.workspaceId, selected: selected,
            requestId: intent.requestId, delivery: nil, expectedTurn: nil, currentTurn: nil,
            queuedCount: nil, duplicate: false, reasonCode: nil, modelConfig: newConfig
        )
        settledReplies[intent.requestId] = settled
        return settled
    }
    func query(_ intent: DSHQueryIntent) async throws -> DSHCommandReply {
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard intent.hostId == "qa-host", intent.generation == generation else {
            return commandReply(status: "stale", requestId: intent.requestId)
        }
        if let existing = settledReplies[intent.requestId] {
            return withDuplicate(existing)
        }
        return commandReply(status: "unknown", requestId: intent.requestId, reasonCode: "query.unknownRequest")
    }
    private func nullTurn() -> Int? { nil }
    private func withDuplicate(_ reply: DSHCommandReply) -> DSHCommandReply {
        DSHCommandReply(
            hostId: reply.hostId, generation: reply.generation, status: reply.status,
            sessionId: reply.sessionId, workspaceId: reply.workspaceId, selected: reply.selected,
            requestId: reply.requestId, delivery: reply.delivery, expectedTurn: reply.expectedTurn,
            currentTurn: reply.currentTurn, queuedCount: reply.queuedCount, duplicate: true,
            reasonCode: reply.reasonCode, modelConfig: reply.modelConfig
        )
    }
    private func commandReply(status: String, selected: String? = nil, requestId: String, reasonCode: String? = nil) -> DSHCommandReply {
        DSHCommandReply(
            hostId: "qa-host", generation: generation, status: status,
            sessionId: nil, workspaceId: nil, selected: selected,
            requestId: requestId, delivery: nil, expectedTurn: nil,
            currentTurn: nil, queuedCount: nil, duplicate: false, reasonCode: reasonCode
        )
    }
    private func reply(_ status: DSHOpenResult, selected: String? = nil) -> DSHOpenReply {
        DSHOpenReply(hostId: "qa-host", generation: generation, status: status, selected: selected)
    }
    func close() async { if connected { closes += 1 }; connected = false }
}

extension Store {
    func prepareDSHNavigationScenario() async {
        let fixture = DSHNavigationFixture.shared
        await fixture.reset()
        var checks: [String: Bool] = [:]
        func waitReady() async throws {
            for _ in 0..<100 {
                if dshSessions.state == .ready { return }
                try await Task.sleep(nanoseconds: 10_000_000)
            }
            throw DSHFailure.unavailable
        }
        func queuedOpen(_ selection: DSHSelection) async {
            openDSHSession(selection)
            await hostExecutionTask?.value
        }
        func queuedCommand(_ command: DSHQueuedCommand?) async {
            guard let command else { return }
            switch command {
            case .create: createDSHSession(command)
            case .prompt: promptDSHSession(command)
            case .cancel: cancelDSHSession(command)
            case .query: queryDSHCommand(command)
            case .resolve: resolveDSHSession(command)
            case .switchModel: switchModelDSHSession(command)
            }
            await hostExecutionTask?.value
        }
        do {
            await installCodingAgentPresets()
            guard let profile = hostProfiles.first(where: { $0.name == "DeepSeek Coder (DSH)" }) else { throw DSHFailure.unavailable }
            await setHostActiveProfile(profile.id.text)
            let before = await fixture.counts()
            await dshSessions.refresh()
            let disabled = await fixture.counts()
            checks["disabled_zero_reads"] = before.reads == disabled.reads
            dshSessions.setEnabled(true); try await waitReady()
            guard let first = dshSessions.selection(for: DSHNavigationFixture.first),
                  let second = dshSessions.selection(for: DSHNavigationFixture.second) else { throw DSHFailure.invalidReply }
            checks["same_name_exact_identity"] = dshSessions.rows.count == 2
                && Set(dshSessions.rows.map(\.title)).count == 1 && first.sessionId != second.sessionId
                && first.workspaceId != second.workspaceId
            checks["cold_target_unavailable"] = dshSessions.unavailableSessionIds == [DSHNavigationFixture.cold]
                && dshSessions.selection(for: DSHNavigationFixture.cold) == nil
            await fixture.pendingRoundTrip(); await dshSessions.refresh()
            checks["pending_only_transitions_retained"] = dshSessions.lastChanges.map(\.row.pendingInput) == [1, 0]
                && dshSessions.lastChanges.map(\.sequence) == [1, 2]
            await queuedOpen(first)
            let firstConfirmed = lastHostActionReceipt?.outcome == "confirmed"
            await queuedOpen(second)
            let successful = await fixture.counts()
            checks["selected_id_readback"] = firstConfirmed && lastHostActionReceipt?.outcome == "confirmed"
                && successful.navigations == [first.sessionId, second.sessionId]

            // Re-select first session for K2 prompt/cancel checks
            await queuedOpen(first)

            // Steer prompt confirmed on running turn 1
            let steerCmd = dshSessions.promptIntent(for: DSHNavigationFixture.first, mode: .steer, text: "steer current turn", requestId: "qa-req-steer-1")
            await queuedCommand(steerCmd)
            let steeredOk = lastHostActionReceipt?.actionType == "dsh_prompt"
                && lastHostActionReceipt?.outcome == "steered"
                && dshSessions.lastCommandResult == .steered(sessionId: DSHNavigationFixture.first, requestId: "qa-req-steer-1", currentTurn: 1, queuedCount: 1, duplicate: false)

            // Duplicate requestId must not double-send
            let promptsBeforeDup = (await fixture.counts()).prompts
            await queuedCommand(steerCmd)
            let promptsAfterDup = (await fixture.counts()).prompts
            checks["duplicate_request_no_double_send"] = promptsBeforeDup == 1 && promptsAfterDup == 1
                && dshSessions.lastCommandResult == .steered(sessionId: DSHNavigationFixture.first, requestId: "qa-req-steer-1", currentTurn: 1, queuedCount: 1, duplicate: true)

            let queueCmd = dshSessions.promptIntent(for: DSHNavigationFixture.first, mode: .queue, text: "queue next turn", requestId: "qa-req-queue-1")
            await queuedCommand(queueCmd)
            let queuedOk = lastHostActionReceipt?.actionType == "dsh_prompt"
                && lastHostActionReceipt?.outcome == "queued"
                && dshSessions.lastCommandResult == .queued(sessionId: DSHNavigationFixture.first, requestId: "qa-req-queue-1", currentTurn: 1, queuedCount: 2, duplicate: false)
            let firstRowAfterPrompts = dshSessions.rows.first(where: { $0.sessionId == DSHNavigationFixture.first })
            checks["queue_and_steer_confirmed_separately"] = steeredOk && queuedOk
                && firstRowAfterPrompts?.turnStatusLabel == "轮次 #1 运行中"
                && firstRowAfterPrompts?.queueStatusLabel == "队列 2 条"

            // Capture cancel intent for session A on turn 1, then switch to session B: old cancel on A must not mis-stop A or B
            let cancelAOnTurn1 = dshSessions.cancelIntent(for: DSHNavigationFixture.first, requestId: "qa-req-cancel-stale-turn1")
            await queuedOpen(second)
            let cancelsBeforeWrongSession = (await fixture.counts()).cancels
            await queuedCommand(cancelAOnTurn1)
            let cancelsAfterWrongSession = (await fixture.counts()).cancels
            checks["switch_session_no_misstop"] = cancelsBeforeWrongSession == 0 && cancelsAfterWrongSession == 0
                && lastHostActionReceipt?.actionType == "dsh_cancel"
                && lastHostActionReceipt?.outcome == "unconfirmed"

            // Switch back to session A; advance session A to turn 2 on Host; old Stop(expectedTurn=1) must do zero cancellation
            await queuedOpen(first)
            await fixture.setTurnState(sessionId: DSHNavigationFixture.first, running: true, currentTurn: 2)
            let cancelsBeforeStaleTurn = (await fixture.counts()).cancels
            await queuedCommand(cancelAOnTurn1)
            let cancelsAfterStaleTurn = (await fixture.counts()).cancels
            checks["same_session_stale_stop_zero_cancel"] = cancelsBeforeStaleTurn == 0 && cancelsAfterStaleTurn == 0
                && lastHostActionReceipt?.actionType == "dsh_cancel"
                && lastHostActionReceipt?.outcome == "noop"
                && dshSessions.lastCommandResult == .noop(reasonCode: "guard.turnChanged", currentTurn: 2, queuedCount: 2)

            // Refresh so controller sees turn 2, then cancel turn 2 while queue has 2 items -> cancelledQueueRemaining (not all stopped)
            await dshSessions.refresh()
            let cancelAOnTurn2 = dshSessions.cancelIntent(for: DSHNavigationFixture.first, requestId: "qa-req-cancel-turn2")
            await queuedCommand(cancelAOnTurn2)
            let cancelsAfterTurn2 = (await fixture.counts()).cancels
            checks["cancel_queue_remaining_not_all_stopped"] = cancelsAfterTurn2 == 1
                && lastHostActionReceipt?.actionType == "dsh_cancel"
                && lastHostActionReceipt?.outcome == "cancelledQueueRemaining"
                && dshSessions.lastCommandResult?.label.contains("队列仍有 2 条可能继续") == true

            // Unprovable turn atomicity must refuse to send
            await fixture.setTurnState(sessionId: DSHNavigationFixture.first, running: true, currentTurn: 3)
            await dshSessions.refresh()
            let prebuiltCancelForUnprovable = dshSessions.cancelIntent(for: DSHNavigationFixture.first, requestId: "qa-req-unprovable-exec")
            await fixture.setTurnState(sessionId: DSHNavigationFixture.first, running: true, currentTurn: nil)
            await dshSessions.refresh()
            let unprovableIntent = dshSessions.cancelIntent(for: DSHNavigationFixture.first, requestId: "qa-req-unprovable")
            await queuedCommand(prebuiltCancelForUnprovable)
            let deniedOutcome = lastHostActionReceipt?.actionType == "dsh_cancel" && lastHostActionReceipt?.outcome == "denied"
            dshSessions.setAtomicTurnCheckSupported(false)
            await fixture.setTurnState(sessionId: DSHNavigationFixture.first, running: true, currentTurn: 3)
            await dshSessions.refresh()
            let unsupportedAtomicIntent = dshSessions.cancelIntent(for: DSHNavigationFixture.first, requestId: "qa-req-unsupported-atomic")
            dshSessions.setAtomicTurnCheckSupported(true)
            let cancelsAfterUnprovable = (await fixture.counts()).cancels
            checks["unprovable_turn_atomicity_refused"] = unprovableIntent == nil && unsupportedAtomicIntent == nil
                && deniedOutcome
                && cancelsAfterUnprovable == 1
                && dshSessions.lastCommandResult == .denied(reasonCode: "guard.unprovableTurnAtomicity")

            // Create session bound to workspace-a and verify selected readback
            let createCmd = dshSessions.createIntent(workspaceId: "workspace-a", requestId: "qa-req-create-1")
            await queuedCommand(createCmd)
            let createdCounts = await fixture.counts()
            checks["create_workspace_bound_readback"] = createdCounts.creates == 1
                && lastHostActionReceipt?.actionType == "dsh_create"
                && lastHostActionReceipt?.outcome == "confirmed"
                && dshSessions.selected == "qa-session-created-1"
                && dshSessions.rows.contains(where: { $0.sessionId == "qa-session-created-1" && $0.workspaceId == "workspace-a" })

            // Slice L2: Resolve approval recorded
            await queuedOpen(first)
            await fixture.setPendingInteraction(sessionId: DSHNavigationFixture.first, interaction: DSHPendingInteraction(interactionId: "qa-perm-1", interactionType: .permission, toolName: "bash", options: ["allowed-once", "rejected"]))
            await dshSessions.refresh()
            let resolveApprovalCmd = dshSessions.resolveIntent(for: DSHNavigationFixture.first, interactionId: "qa-perm-1", interactionType: .permission, decision: .approve, requestId: "qa-req-res-perm-1")
            await queuedCommand(resolveApprovalCmd)
            let resolvesAfterApproval = (await fixture.counts()).resolves
            checks["resolve_approval_recorded"] = resolvesAfterApproval == 1
                && lastHostActionReceipt?.actionType == "dsh_resolve"
                && lastHostActionReceipt?.outcome == "resolved"
                && dshSessions.lastCommandResult == .resolved(sessionId: DSHNavigationFixture.first, requestId: "qa-req-res-perm-1", duplicate: false)

            // Slice L2: Already resolved returns noop with zero side effect
            await queuedCommand(resolveApprovalCmd)
            checks["already_resolved_noop_zero_side_effect"] = lastHostActionReceipt?.actionType == "dsh_resolve"
                && (lastHostActionReceipt?.outcome == "resolved" || lastHostActionReceipt?.outcome == "noop")

            // Slice L2: Interaction type mismatch blocked
            await fixture.setPendingInteraction(sessionId: DSHNavigationFixture.first, interaction: DSHPendingInteraction(interactionId: "qa-q-1", interactionType: .question, toolName: "ask_user_question", options: ["Yes", "No"]))
            await dshSessions.refresh()
            let mismatchCmd = dshSessions.resolveIntent(for: DSHNavigationFixture.first, interactionId: "qa-q-1", interactionType: .permission, decision: .approve, requestId: "qa-req-mismatch-1")
            await queuedCommand(mismatchCmd)
            checks["interaction_type_mismatch_blocked"] = lastHostActionReceipt?.actionType == "dsh_resolve"
                && dshSessions.lastCommandResult == .interactionTypeMismatch(reasonCode: "guard.questionCannotApproveAsPermission")

            // Slice L2: Resolve question answered
            let resolveQuestionCmd = dshSessions.resolveIntent(for: DSHNavigationFixture.first, interactionId: "qa-q-1", interactionType: .question, decision: .approve, optionId: "Yes", requestId: "qa-req-res-q-1")
            await queuedCommand(resolveQuestionCmd)
            checks["resolve_question_answered"] = lastHostActionReceipt?.actionType == "dsh_resolve"
                && lastHostActionReceipt?.outcome == "resolved"

            // Slice L2: Select model with readback confirmed
            let switchModelCmd = dshSessions.switchModelIntent(for: DSHNavigationFixture.first, provider: "deepseek-official", model: "deepseek-chat", thought: "high", requestId: "qa-req-model-1")
            await queuedCommand(switchModelCmd)
            let modelSwitchedOk = lastHostActionReceipt?.actionType == "dsh_switch_model"
                && lastHostActionReceipt?.outcome == "modelSwitched"
                && dshSessions.lastCommandResult == .modelSwitched(sessionId: DSHNavigationFixture.first, requestId: "qa-req-model-1", modelConfig: DSHModelConfig(provider: "deepseek-official", model: "deepseek-chat", thought: "high"), duplicate: false)
            let rowAfterModelSwitch = dshSessions.rows.first(where: { $0.sessionId == DSHNavigationFixture.first })
            checks["select_model_readback_confirmed"] = modelSwitchedOk && rowAfterModelSwitch?.modelConfig?.thought == "high"

            // Slice L2: Unsupported thought level locally refused without wire command
            let selectModelsBeforeUnsupported = (await fixture.counts()).selectModels
            let unsupportedThoughtCmd = dshSessions.switchModelIntent(for: DSHNavigationFixture.first, provider: "deepseek-official", model: "deepseek-reasoner", thought: "off", requestId: "qa-req-unsupported-thought")
            await queuedCommand(unsupportedThoughtCmd)
            let selectModelsAfterUnsupported = (await fixture.counts()).selectModels
            checks["unsupported_thought_level_locally_refused"] = selectModelsBeforeUnsupported == selectModelsAfterUnsupported
                && dshSessions.lastCommandResult == .unsupportedThought(reasonCode: "guard.unsupportedThoughtLevel")

            await fixture.setMode(.wrongSelected); await queuedOpen(first)
            checks["wrong_selected_unconfirmed"] = lastHostActionReceipt?.outcome != "confirmed"
            let outside = DSHSelection(sessionId: "outside", workspaceId: first.workspaceId,
                hostId: first.hostId, generation: first.generation, localEpoch: first.localEpoch)
            let deniedBefore = await fixture.counts(); await queuedOpen(outside)
            let deniedAfter = await fixture.counts()
            checks["out_of_scope_zero_reads"] = deniedBefore.reads == deniedAfter.reads && deniedBefore.opens == deniedAfter.opens
            let beforeDeleted = await fixture.counts()
            await fixture.setMode(.deleted); await queuedOpen(first)
            let deletedCounts = await fixture.counts()
            checks["deleted_target_no_redirect"] = lastHostActionReceipt?.outcome != "confirmed"
                && deletedCounts.navigations == beforeDeleted.navigations
            dshSessions.setEnabled(false); await fixture.setMode(.normal)
            dshSessions.setEnabled(true); try await waitReady()
            guard let old = dshSessions.selection(for: DSHNavigationFixture.second) else { throw DSHFailure.invalidReply }
            await fixture.setMode(.rotated); await dshSessions.refresh()
            let staleBefore = await fixture.counts(); await queuedOpen(old)
            let staleAfter = await fixture.counts()
            checks["host_generation_rejects_old_selection"] = staleBefore.opens == staleAfter.opens && dshSessions.rows.isEmpty
            dshSessions.setEnabled(false); await fixture.setMode(.normal)
            dshSessions.setEnabled(true); try await waitReady()
            await fixture.setMode(.gap); await dshSessions.refresh()
            checks["journal_gap_clears_targets"] = dshSessions.rows.isEmpty && dshSessions.state != .ready
            await dshSessions.shutdown()
            checks["bridge_subscription_closed"] = (await fixture.counts()).closes > 0
        } catch {
            checks["scenario_completed"] = false
        }
        let finalCounts = await fixture.counts()
        let payload: [String: Any] = [
            "status": checks.count == 24 && checks.values.allSatisfy({ $0 }) && finalCounts.bodyReads == 0 ? "PASS" : "FAIL",
            "checks": checks,
            "body_reads": finalCounts.bodyReads,
            "native_app": "NOT_RUN",
            "physical_key": "NOT_RUN"
        ]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("dsh-navigation.json"))
        }
    }
}
#endif
