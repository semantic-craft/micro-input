#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

actor ZCodeNavigationFixture: ZCodeSessionServing {
    static let shared = ZCodeNavigationFixture()
    static let first = ZCodeTaskAddress(workspacePath: "/qa/workspace-a", taskId: "same-id")
    static let second = ZCodeTaskAddress(workspacePath: "/qa/workspace-b", taskId: "same-id")
    enum Mode { case normal, wrongSelected, deleted, rotated, disconnected }
    struct SettledEntry {
        let status: ZCodeCommandResult
        let selected: ZCodeTaskAddress?
        let delivery: ZCodeDelivery?
        let executionId: String?
        let interactionId: String?
        let interactionType: ZCodeInteractionType?
        let modelConfig: ZCodeModelConfig?
        let reasonCode: String?
    }
    private static let defaultModelConfig = ZCodeModelConfig(
        provider: "glm",
        model: "glm-5.1",
        thought: "medium",
        supportedThoughts: ["off", "low", "medium", "high"],
        availableModels: [
            ZCodeModelOption(provider: "glm", model: "glm-5.1", defaultThought: "medium", supportedThoughts: ["off", "low", "medium", "high"]),
            ZCodeModelOption(provider: "zhipu", model: "glm-4-flash", defaultThought: "off", supportedThoughts: ["off"])
        ]
    )
    private var mode = Mode.normal
    private var selected: ZCodeTaskAddress?
    private var dynamicRows: [ZCodeTaskRow] = []
    private var liveStatusByAddress: [ZCodeTaskAddress: String] = [:]
    private var executionByAddress: [ZCodeTaskAddress: String] = [:]
    private var pendingInteractionByAddress: [ZCodeTaskAddress: ZCodePendingInteraction] = [:]
    private var modelConfigByAddress: [ZCodeTaskAddress: ZCodeModelConfig] = [:]
    private var forceQueueForGuide = false
    private var timeoutNextSend = false
    private var holdReconcileUnknown = false
    private var corruptNextModelReadback = false
    private var settledByKey: [String: SettledEntry] = [:]
    private var heldByKey: [String: SettledEntry] = [:]
    private(set) var metadataReads = 0
    private(set) var opens = 0
    private(set) var creates = 0
    private(set) var sends = 0
    private(set) var stops = 0
    private(set) var resolves = 0
    private(set) var switchModels = 0
    private(set) var queries = 0
    private(set) var navigations: [ZCodeTaskAddress] = []
    private(set) var sentByAddress: [ZCodeTaskAddress: [String]] = [:]
    private(set) var stoppedExecutions: [String] = []
    private(set) var resolvedInteractions: [String] = []
    private(set) var commandIdsSeen: [String] = []
    private var generation: UInt64 { mode == .rotated ? 2 : 1 }

    func setMode(_ mode: Mode) { self.mode = mode }
    func resetAll() {
        mode = .normal
        selected = nil
        dynamicRows.removeAll()
        liveStatusByAddress.removeAll()
        executionByAddress.removeAll()
        pendingInteractionByAddress.removeAll()
        modelConfigByAddress.removeAll()
        forceQueueForGuide = false
        timeoutNextSend = false
        holdReconcileUnknown = false
        corruptNextModelReadback = false
        settledByKey.removeAll()
        heldByKey.removeAll()
        metadataReads = 0
        opens = 0
        creates = 0
        sends = 0
        stops = 0
        resolves = 0
        switchModels = 0
        queries = 0
        navigations.removeAll()
        sentByAddress.removeAll()
        stoppedExecutions.removeAll()
        resolvedInteractions.removeAll()
        commandIdsSeen.removeAll()
    }
    func configureRunning(_ address: ZCodeTaskAddress, executionId: String?) {
        liveStatusByAddress[address] = executionId == nil ? "idle" : "running"
        if let executionId {
            executionByAddress[address] = executionId
        } else {
            executionByAddress.removeValue(forKey: address)
        }
    }
    func configureInteraction(_ address: ZCodeTaskAddress, interaction: ZCodePendingInteraction?) {
        if let interaction {
            pendingInteractionByAddress[address] = interaction
            liveStatusByAddress[address] = "waiting"
        } else {
            pendingInteractionByAddress.removeValue(forKey: address)
        }
    }
    func setCorruptNextModelReadback(_ value: Bool) { corruptNextModelReadback = value }
    func setForceQueueForGuide(_ value: Bool) { forceQueueForGuide = value }
    func setTimeoutNextSend(_ value: Bool, holdUnknown: Bool) {
        timeoutNextSend = value
        holdReconcileUnknown = holdUnknown
    }
    func settleHeldCommands() {
        for (key, value) in heldByKey {
            settledByKey[key] = value
        }
        heldByKey.removeAll()
        holdReconcileUnknown = false
    }
    func counts() -> (
        reads: Int,
        opens: Int,
        creates: Int,
        sends: Int,
        stops: Int,
        resolves: Int,
        switchModels: Int,
        queries: Int,
        navigations: [ZCodeTaskAddress],
        sentByAddress: [ZCodeTaskAddress: [String]],
        stoppedExecutions: [String],
        resolvedInteractions: [String],
        commandIdsSeen: [String],
        activeExecutionForSecond: String?
    ) {
        (
            metadataReads,
            opens,
            creates,
            sends,
            stops,
            resolves,
            switchModels,
            queries,
            navigations,
            sentByAddress,
            stoppedExecutions,
            resolvedInteractions,
            commandIdsSeen,
            executionByAddress[Self.second]
        )
    }
    func connect() async throws -> ZCodeSessionSnapshot { try await read() }
    func read() async throws -> ZCodeSessionSnapshot {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        metadataReads += 1
        let baseAddresses = mode == .deleted ? [Self.second] : [Self.first, Self.second]
        var allRows = baseAddresses.map { addr in
            ZCodeTaskRow(
                address: addr,
                title: "同名任务",
                liveStatus: liveStatusByAddress[addr] ?? "idle",
                executionId: executionByAddress[addr],
                pendingInteraction: pendingInteractionByAddress[addr],
                modelConfig: modelConfigByAddress[addr] ?? Self.defaultModelConfig
            )
        }
        for extra in dynamicRows where !allRows.contains(where: { $0.address == extra.address }) {
            allRows.append(
                ZCodeTaskRow(
                    address: extra.address,
                    title: extra.title,
                    liveStatus: liveStatusByAddress[extra.address] ?? extra.liveStatus,
                    executionId: executionByAddress[extra.address] ?? extra.executionId,
                    pendingInteraction: pendingInteractionByAddress[extra.address],
                    modelConfig: modelConfigByAddress[extra.address] ?? Self.defaultModelConfig
                )
            )
        }
        return ZCodeSessionSnapshot(
            instanceId: "qa-zcode-instance",
            generation: generation,
            scopes: [Self.first.scope, Self.second.scope],
            rows: allRows,
            selected: allRows.contains(where: { $0.address == selected }) ? selected : nil
        )
    }
    func open(_ selection: ZCodeSelection) async throws -> ZCodeOpenReply {
        opens += 1
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard selection.instanceId == "qa-zcode-instance", selection.generation == generation else {
            return reply(.stale)
        }
        let validAddresses = [Self.first, Self.second] + dynamicRows.map(\.address)
        guard validAddresses.contains(selection.address) else { return reply(.denied) }
        guard mode != .deleted || selection.address != Self.first else { return reply(.stale) }
        if mode == .wrongSelected { return reply(.confirmed, selected: Self.second) }
        selected = selection.address
        navigations.append(selection.address)
        return reply(.confirmed, selected: selected)
    }
    func create(_ intent: ZCodeCreateIntent) async throws -> ZCodeCommandReply {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard intent.instanceId == "qa-zcode-instance", intent.generation == generation else {
            return commandReply(.stale, commandId: intent.commandId)
        }
        guard [Self.first.scope, Self.second.scope].contains(intent.scope) else {
            return commandReply(.denied, commandId: intent.commandId)
        }
        let sessionId = intent.sessionId ?? "qa-created-\(intent.commandId)"
        let key = "\(sessionId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let existing = settledByKey[key] {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: existing.status,
                selected: existing.selected,
                commandId: intent.commandId,
                duplicate: true,
                delivery: existing.delivery,
                executionId: existing.executionId,
                reasonCode: existing.reasonCode
            )
        }
        creates += 1
        let created = ZCodeTaskAddress(
            workspacePath: intent.scope.workspacePath,
            workspaceIdentity: intent.scope.workspaceIdentity,
            sourceServerId: intent.scope.sourceServerId,
            remoteSessionId: intent.scope.remoteSessionId,
            taskId: sessionId
        )
        let running = intent.firstInputText?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == false
        let execId = running ? "exec-create-\(creates)" : nil
        liveStatusByAddress[created] = running ? "running" : "idle"
        if let execId { executionByAddress[created] = execId }
        dynamicRows.append(ZCodeTaskRow(address: created, title: "新建任务", liveStatus: running ? "running" : "idle", executionId: execId))
        selected = created
        navigations.append(created)
        let entry = SettledEntry(status: .confirmed, selected: created, delivery: running ? .startNow : nil, executionId: execId, interactionId: nil, interactionType: nil, modelConfig: nil, reasonCode: nil)
        settledByKey[key] = entry
        return ZCodeCommandReply(
            instanceId: "qa-zcode-instance",
            generation: generation,
            status: .confirmed,
            selected: created,
            commandId: intent.commandId,
            duplicate: false,
            delivery: entry.delivery,
            executionId: execId
        )
    }
    func send(_ intent: ZCodeSendIntent) async throws -> ZCodeCommandReply {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard intent.selection.instanceId == "qa-zcode-instance", intent.selection.generation == generation else {
            return commandReply(.stale, commandId: intent.commandId)
        }
        let address = intent.selection.address
        guard selected == address else {
            return commandReply(.unconfirmed, selected: selected, commandId: intent.commandId)
        }
        let key = "\(address.taskId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let existing = settledByKey[key] {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: existing.status,
                selected: existing.selected,
                commandId: intent.commandId,
                duplicate: true,
                delivery: existing.delivery,
                executionId: existing.executionId,
                interactionId: existing.interactionId,
                interactionType: existing.interactionType,
                modelConfig: existing.modelConfig,
                reasonCode: existing.reasonCode
            )
        }
        sends += 1
        sentByAddress[address, default: []].append(intent.text)
        let isRunning = (liveStatusByAddress[address] ?? "idle") == "running"
        let delivery: ZCodeDelivery
        let status: ZCodeCommandResult
        if !isRunning {
            delivery = .startNow
            status = .started
            liveStatusByAddress[address] = "running"
            if executionByAddress[address] == nil {
                executionByAddress[address] = "exec-\(sends)"
            }
        } else if intent.requestedDelivery == .guide && !forceQueueForGuide {
            delivery = .guide
            status = .steered
        } else {
            delivery = .queue
            status = .queued
        }
        let execId = executionByAddress[address]
        let entry = SettledEntry(status: status, selected: address, delivery: delivery, executionId: execId, interactionId: nil, interactionType: nil, modelConfig: nil, reasonCode: nil)
        if timeoutNextSend {
            timeoutNextSend = false
            if holdReconcileUnknown {
                heldByKey[key] = entry
            } else {
                settledByKey[key] = entry
            }
            throw ZCodeFailure.transportTimeout
        }
        settledByKey[key] = entry
        return ZCodeCommandReply(
            instanceId: "qa-zcode-instance",
            generation: generation,
            status: status,
            selected: address,
            commandId: intent.commandId,
            duplicate: false,
            delivery: delivery,
            executionId: execId
        )
    }
    func stop(_ intent: ZCodeStopIntent) async throws -> ZCodeCommandReply {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard intent.selection.instanceId == "qa-zcode-instance", intent.selection.generation == generation else {
            return commandReply(.stale, commandId: intent.commandId)
        }
        let address = intent.selection.address
        guard selected == address else {
            return commandReply(.unconfirmed, selected: selected, commandId: intent.commandId)
        }
        let key = "\(address.taskId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let existing = settledByKey[key] {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: existing.status,
                selected: existing.selected,
                commandId: intent.commandId,
                duplicate: true,
                delivery: existing.delivery,
                executionId: existing.executionId,
                interactionId: existing.interactionId,
                interactionType: existing.interactionType,
                modelConfig: existing.modelConfig,
                reasonCode: existing.reasonCode
            )
        }
        stops += 1
        let activeExec = executionByAddress[address]
        let isRunning = (liveStatusByAddress[address] ?? "idle") == "running"
        if !isRunning || activeExec == nil || activeExec != intent.expectedExecutionId {
            let entry = SettledEntry(
                status: .noop,
                selected: address,
                delivery: nil,
                executionId: activeExec,
                interactionId: nil,
                interactionType: nil,
                modelConfig: nil,
                reasonCode: "guard.stopTargetChanged"
            )
            settledByKey[key] = entry
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .noop,
                selected: address,
                commandId: intent.commandId,
                duplicate: false,
                executionId: activeExec,
                reasonCode: "guard.stopTargetChanged"
            )
        }
        stoppedExecutions.append(intent.expectedExecutionId)
        liveStatusByAddress[address] = "idle"
        executionByAddress.removeValue(forKey: address)
        let entry = SettledEntry(status: .stopped, selected: address, delivery: nil, executionId: intent.expectedExecutionId, interactionId: nil, interactionType: nil, modelConfig: nil, reasonCode: nil)
        settledByKey[key] = entry
        return ZCodeCommandReply(
            instanceId: "qa-zcode-instance",
            generation: generation,
            status: .stopped,
            selected: address,
            commandId: intent.commandId,
            duplicate: false,
            executionId: intent.expectedExecutionId
        )
    }
    func resolve(_ intent: ZCodeResolveIntent) async throws -> ZCodeCommandReply {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard intent.selection.instanceId == "qa-zcode-instance", intent.selection.generation == generation else {
            return commandReply(.stale, commandId: intent.commandId)
        }
        let address = intent.selection.address
        guard selected == address else {
            return commandReply(.unconfirmed, selected: selected, commandId: intent.commandId)
        }
        let key = "\(address.taskId)::\(intent.commandId)"
        commandIdsSeen.append(intent.commandId)
        if let existing = settledByKey[key] {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: existing.status,
                selected: existing.selected,
                commandId: intent.commandId,
                duplicate: true,
                interactionId: existing.interactionId,
                interactionType: existing.interactionType,
                reasonCode: existing.reasonCode
            )
        }
        guard let pending = pendingInteractionByAddress[address], pending.interactionId == intent.interactionId else {
            let entry = SettledEntry(
                status: .noop,
                selected: address,
                delivery: nil,
                executionId: nil,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                modelConfig: nil,
                reasonCode: "proto.alreadyResolved"
            )
            settledByKey[key] = entry
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .noop,
                selected: address,
                commandId: intent.commandId,
                duplicate: false,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: "proto.alreadyResolved"
            )
        }
        guard pending.interactionType == intent.interactionType,
              !(intent.interactionType == .question && (intent.decision == .approve || intent.decision == .allowAlways || intent.optionId == "allowOnce" || intent.optionId == "allowAlways")) else {
            let reason = (pending.interactionType == .question) ? "guard.questionCannotApproveAsPermission" : "guard.interactionTypeMismatch"
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .interactionTypeMismatch,
                selected: address,
                commandId: intent.commandId,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: reason
            )
        }
        resolves += 1
        resolvedInteractions.append(intent.interactionId)
        pendingInteractionByAddress.removeValue(forKey: address)
        liveStatusByAddress[address] = "running"
        let entry = SettledEntry(
            status: .resolved,
            selected: address,
            delivery: nil,
            executionId: nil,
            interactionId: intent.interactionId,
            interactionType: intent.interactionType,
            modelConfig: nil,
            reasonCode: nil
        )
        settledByKey[key] = entry
        return ZCodeCommandReply(
            instanceId: "qa-zcode-instance",
            generation: generation,
            status: .resolved,
            selected: address,
            commandId: intent.commandId,
            duplicate: false,
            interactionId: intent.interactionId,
            interactionType: intent.interactionType
        )
    }
    func switchModel(_ intent: ZCodeSwitchModelIntent) async throws -> ZCodeCommandReply {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard intent.selection.instanceId == "qa-zcode-instance", intent.selection.generation == generation else {
            return commandReply(.stale, commandId: intent.commandId)
        }
        let address = intent.selection.address
        guard selected == address else {
            return commandReply(.unconfirmed, selected: selected, commandId: intent.commandId)
        }
        let currentConfig = modelConfigByAddress[address] ?? Self.defaultModelConfig
        guard currentConfig.supportsThought(intent.thought, provider: intent.provider, model: intent.model) else {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .unsupportedThought,
                selected: address,
                commandId: intent.commandId,
                modelConfig: currentConfig,
                reasonCode: "guard.unsupportedThoughtLevel"
            )
        }
        switchModels += 1
        if corruptNextModelReadback {
            corruptNextModelReadback = false
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .modelSwitched,
                selected: address,
                commandId: intent.commandId,
                modelConfig: ZCodeModelConfig(
                    provider: intent.provider,
                    model: intent.model,
                    thought: "mismatched-\(intent.thought)",
                    supportedThoughts: currentConfig.supportedThoughts,
                    availableModels: currentConfig.availableModels
                )
            )
        }
        let targetSupported = currentConfig.availableModels?.first(where: { $0.provider == intent.provider && $0.model == intent.model })?.supportedThoughts ?? currentConfig.supportedThoughts
        let updated = ZCodeModelConfig(
            provider: intent.provider,
            model: intent.model,
            thought: intent.thought,
            supportedThoughts: targetSupported,
            availableModels: currentConfig.availableModels
        )
        modelConfigByAddress[address] = updated
        let key = "\(address.taskId)::\(intent.commandId)"
        let entry = SettledEntry(
            status: .modelSwitched,
            selected: address,
            delivery: nil,
            executionId: nil,
            interactionId: nil,
            interactionType: nil,
            modelConfig: updated,
            reasonCode: nil
        )
        settledByKey[key] = entry
        return ZCodeCommandReply(
            instanceId: "qa-zcode-instance",
            generation: generation,
            status: .modelSwitched,
            selected: address,
            commandId: intent.commandId,
            duplicate: false,
            modelConfig: updated
        )
    }
    func query(_ intent: ZCodeQueryIntent) async throws -> ZCodeCommandReply {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard intent.instanceId == "qa-zcode-instance", intent.generation == generation else {
            return commandReply(.stale, commandId: intent.commandId)
        }
        queries += 1
        let sessionId = intent.target?.taskId ?? "qa-created-\(intent.commandId)"
        let key = "\(sessionId)::\(intent.commandId)"
        if holdReconcileUnknown && heldByKey[key] != nil {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .unknown,
                selected: selected,
                commandId: intent.commandId
            )
        }
        guard let existing = settledByKey[key] else {
            return ZCodeCommandReply(
                instanceId: "qa-zcode-instance",
                generation: generation,
                status: .unknown,
                selected: selected,
                commandId: intent.commandId
            )
        }
        return ZCodeCommandReply(
            instanceId: "qa-zcode-instance",
            generation: generation,
            status: existing.status,
            selected: existing.selected,
            commandId: intent.commandId,
            duplicate: true,
            delivery: existing.delivery,
            executionId: existing.executionId,
            interactionId: existing.interactionId,
            interactionType: existing.interactionType,
            modelConfig: existing.modelConfig,
            reasonCode: existing.reasonCode
        )
    }
    private func reply(_ status: ZCodeOpenResult, selected: ZCodeTaskAddress? = nil) -> ZCodeOpenReply {
        ZCodeOpenReply(instanceId: "qa-zcode-instance", generation: generation, status: status, selected: selected)
    }
    private func commandReply(_ status: ZCodeCommandResult, selected: ZCodeTaskAddress? = nil, commandId: String? = nil) -> ZCodeCommandReply {
        ZCodeCommandReply(instanceId: "qa-zcode-instance", generation: generation, status: status, selected: selected, commandId: commandId)
    }
    func close() async {}
}

extension Store {
    func prepareZCodeNavigationScenario() async {
        let fixture = ZCodeNavigationFixture.shared
        await fixture.resetAll()
        var checks: [String: Bool] = [:]
        func waitReady() async throws {
            for _ in 0..<200 {
                if zcodeSessions.state == .ready { return }
                try await Task.sleep(for: .milliseconds(5))
            }
            throw ZCodeFailure.unavailable
        }
        func queuedOpen(_ selection: ZCodeSelection) async {
            openZCodeSession(selection)
            await hostExecutionTask?.value
        }
        func queuedCreate(_ intent: ZCodeCreateIntent) async {
            createZCodeSession(intent)
            await hostExecutionTask?.value
        }
        func queuedSend(_ intent: ZCodeSendIntent) async {
            sendZCodeText(intent)
            await hostExecutionTask?.value
        }
        func queuedStop(_ intent: ZCodeStopIntent) async {
            stopZCodeSession(intent)
            await hostExecutionTask?.value
        }
        func queuedResolve(_ intent: ZCodeResolveIntent) async {
            resolveZCodeInteraction(intent)
            await hostExecutionTask?.value
        }
        func queuedSwitchModel(_ intent: ZCodeSwitchModelIntent) async {
            switchZCodeModel(intent)
            await hostExecutionTask?.value
        }
        func queuedQuery(_ intent: ZCodeQueryIntent) async {
            queryZCodeCommand(intent)
            await hostExecutionTask?.value
        }
        do {
            await installCodingAgentPresets()
            guard let profile = hostProfiles.first(where: { $0.name == "ZCode" }) else { throw ZCodeFailure.unavailable }
            await setHostActiveProfile(profile.id.text)
            let disabledBefore = await fixture.counts()
            await zcodeSessions.refresh()
            let disabledAfter = await fixture.counts()
            checks["disabled_zero_reads"] = disabledAfter.reads == disabledBefore.reads
            zcodeSessions.setEnabled(true)
            try await waitReady()
            guard let first = zcodeSessions.selection(for: ZCodeNavigationFixture.first),
                  let second = zcodeSessions.selection(for: ZCodeNavigationFixture.second) else { throw ZCodeFailure.invalidReply }
            checks["same_name_two_workspaces"] = zcodeSessions.rows.count == 2
                && Set(zcodeSessions.rows.map(\.title)).count == 1 && first.address != second.address
            await queuedOpen(first)
            let firstConfirmed = lastHostActionReceipt?.outcome == "confirmed"
            await queuedOpen(second)
            let successful = await fixture.counts()
            checks["selected_readback_matches"] = firstConfirmed && lastHostActionReceipt?.outcome == "confirmed"
                && successful.navigations == [first.address, second.address]

            // 1. Two sessions only affect the selected one (second is selected; sending to first is unconfirmed)
            let unselectedSend = ZCodeSendIntent(
                selection: first,
                commandId: "cmd-unselected-send",
                clientId: "qa-client",
                text: "must-not-deliver"
            )
            await queuedSend(unselectedSend)
            let afterUnselectedOutcome = lastHostActionReceipt?.outcome
            guard let selectedSend = zcodeSessions.sendIntent(
                for: second.address,
                text: "run-on-second",
                commandId: "cmd-selected-send",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedSend(selectedSend)
            let afterIsolation = await fixture.counts()
            checks["selected_only_isolation"] = afterUnselectedOutcome == "unconfirmed"
                && lastHostActionReceipt?.outcome == "started"
                && (afterIsolation.sentByAddress[first.address] ?? []).isEmpty
                && afterIsolation.sentByAddress[second.address] == ["run-on-second"]

            // 2. Duplicate command deduplication (same commandId does not double-send)
            await queuedSend(selectedSend)
            let afterDedup = await fixture.counts()
            checks["duplicate_command_dedup"] = lastHostActionReceipt?.outcome == "started"
                && zcodeSessions.lastCommandReply?.duplicate == true
                && afterDedup.sends == afterIsolation.sends
                && afterDedup.sentByAddress[second.address]?.count == 1

            // 3. In-flight sending on running session: guide -> steered, queue -> queued, zero new sessions created
            await fixture.configureRunning(second.address, executionId: "exec-running-1")
            await zcodeSessions.refresh()
            guard let guideSend = zcodeSessions.sendIntent(
                for: second.address,
                text: "steer-now",
                requestedDelivery: .guide,
                commandId: "cmd-guide-1",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedSend(guideSend)
            let steeredOutcome = lastHostActionReceipt?.outcome
            await fixture.setForceQueueForGuide(true)
            guard let queuedWhenGuideUnavailable = zcodeSessions.sendIntent(
                for: second.address,
                text: "fallback-to-queue",
                requestedDelivery: .guide,
                commandId: "cmd-queue-1",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedSend(queuedWhenGuideUnavailable)
            let queuedOutcome = lastHostActionReceipt?.outcome
            await fixture.setForceQueueForGuide(false)
            let afterInflight = await fixture.counts()
            checks["inflight_steering_vs_queue"] = steeredOutcome == "steered"
                && queuedOutcome == "queued"
                && afterInflight.creates == 0
                && afterInflight.sentByAddress[second.address]?.count == 3

            // 4. Late stop carrying old executionId does not stop new execution
            await fixture.configureRunning(second.address, executionId: "exec-running-2")
            await zcodeSessions.refresh()
            guard let staleStop = zcodeSessions.stopIntent(
                for: second.address,
                expectedExecutionId: "exec-running-1",
                commandId: "cmd-stop-stale",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedStop(staleStop)
            let staleStopOutcome = lastHostActionReceipt?.outcome
            let staleStopReason = zcodeSessions.lastCommandReply?.reasonCode
            let afterStaleStop = await fixture.counts()
            guard let matchingStop = zcodeSessions.stopIntent(
                for: second.address,
                expectedExecutionId: "exec-running-2",
                commandId: "cmd-stop-current",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedStop(matchingStop)
            let matchingStopOutcome = lastHostActionReceipt?.outcome
            let afterMatchingStop = await fixture.counts()
            checks["late_stop_noop"] = staleStopOutcome == "noop"
                && staleStopReason == "guard.stopTargetChanged"
                && afterStaleStop.activeExecutionForSecond == "exec-running-2"
                && matchingStopOutcome == "stopped"
                && afterMatchingStop.activeExecutionForSecond == nil
                && afterMatchingStop.stoppedExecutions == ["exec-running-2"]

            // 5. Timeout / unknown outcome does not replay with a new commandId
            await fixture.setTimeoutNextSend(true, holdUnknown: true)
            guard let timeoutSend = zcodeSessions.sendIntent(
                for: second.address,
                text: "timeout-message",
                commandId: "cmd-timeout-original",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedSend(timeoutSend)
            let initialTimeoutOutcome = lastHostActionReceipt?.outcome
            let latchedId = zcodeSessions.latchedCommand(forAddress: second.address)?.commandId
            // Attempt to create a new sendIntent with a DIFFERENT commandId while latched:
            guard let retryIntent = zcodeSessions.sendIntent(
                for: second.address,
                text: "timeout-message",
                commandId: "cmd-should-not-be-used",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            let preservedOriginalId = retryIntent.commandId == "cmd-timeout-original"
            await fixture.settleHeldCommands()
            await queuedSend(retryIntent)
            let reconciledOutcome = lastHostActionReceipt?.outcome
            let afterTimeoutReconcile = await fixture.counts()
            checks["timeout_same_id_no_replay"] = initialTimeoutOutcome == "unknown"
                && latchedId == "cmd-timeout-original"
                && preservedOriginalId
                && reconciledOutcome == "started"
                && zcodeSessions.latchedCommand(forAddress: second.address) == nil
                && !afterTimeoutReconcile.commandIdsSeen.contains("cmd-should-not-be-used")
                && afterTimeoutReconcile.sentByAddress[second.address]?.filter({ $0 == "timeout-message" }).count == 1

            // 6. Workspace-bound createSession and new ID readback
            guard let createIntent = zcodeSessions.createIntent(
                for: ZCodeNavigationFixture.first.scope,
                commandId: "cmd-create-1",
                clientId: "qa-client",
                sessionId: "qa-created-session-1",
                firstInputText: "bootstrap-workspace-a"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedCreate(createIntent)
            let createOutcome = lastHostActionReceipt?.outcome
            let expectedCreatedAddress = ZCodeTaskAddress(
                workspacePath: ZCodeNavigationFixture.first.workspacePath,
                taskId: "qa-created-session-1"
            )
            let afterCreate = await fixture.counts()
            checks["create_workspace_readback"] = createOutcome == "confirmed"
                && zcodeSessions.selected == expectedCreatedAddress
                && zcodeSessions.rows.contains(where: { $0.address == expectedCreatedAddress })
                && afterCreate.creates == 1

            // Reset to second selected for Slice L1 interaction & model switching checks
            await queuedOpen(second)

            // 7. Question is never approved as permission; valid question answer succeeds
            await fixture.configureInteraction(
                second.address,
                interaction: ZCodePendingInteraction(
                    interactionId: "int-q-1",
                    interactionType: .question,
                    toolName: "AskUserQuestion",
                    options: ["Option A", "Option B"]
                )
            )
            await zcodeSessions.refresh()
            guard let misapproveQuestionAsPerm = zcodeSessions.resolveIntent(
                for: second.address,
                interactionId: "int-q-1",
                interactionType: .permission,
                decision: .approve,
                commandId: "cmd-misapprove-q1",
                clientId: "qa-client"
            ),
            let misapproveQuestionWithAllowOnce = zcodeSessions.resolveIntent(
                for: second.address,
                interactionId: "int-q-1",
                interactionType: .question,
                decision: .approve,
                optionId: "allowOnce",
                commandId: "cmd-misapprove-q2",
                clientId: "qa-client"
            ),
            let validQuestionAnswer = zcodeSessions.resolveIntent(
                for: second.address,
                interactionId: "int-q-1",
                interactionType: .question,
                decision: .accept,
                optionId: "Option A",
                commandId: "cmd-answer-q1",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedResolve(misapproveQuestionAsPerm)
            let misapproveOutcome1 = lastHostActionReceipt?.outcome
            let misapproveReason1 = zcodeSessions.lastCommandReply?.reasonCode
            await queuedResolve(misapproveQuestionWithAllowOnce)
            let misapproveOutcome2 = lastHostActionReceipt?.outcome
            let misapproveReason2 = zcodeSessions.lastCommandReply?.reasonCode
            let afterMisapprovals = await fixture.counts()
            await queuedResolve(validQuestionAnswer)
            let validQuestionOutcome = lastHostActionReceipt?.outcome
            let afterValidQuestion = await fixture.counts()
            checks["question_never_approved_as_permission"] = misapproveOutcome1 == "interactionTypeMismatch"
                && misapproveReason1 == "guard.questionCannotApproveAsPermission"
                && misapproveOutcome2 == "interactionTypeMismatch"
                && misapproveReason2 == "guard.questionCannotApproveAsPermission"
                && afterMisapprovals.resolves == 0
                && validQuestionOutcome == "resolved"
                && afterValidQuestion.resolves == 1

            // 8. Expired / duplicate interaction requests produce zero mis-approvals; permission & plan resolve cleanly
            await fixture.configureInteraction(
                second.address,
                interaction: ZCodePendingInteraction(
                    interactionId: "int-perm-2",
                    interactionType: .permission,
                    toolName: "Bash",
                    options: ["allowOnce", "allowAlways", "deny"]
                )
            )
            await zcodeSessions.refresh()
            guard let expiredPermIntent = zcodeSessions.resolveIntent(
                for: second.address,
                interactionId: "int-perm-old",
                interactionType: .permission,
                decision: .approve,
                commandId: "cmd-expired-perm",
                clientId: "qa-client"
            ),
            let validPermIntent = zcodeSessions.resolveIntent(
                for: second.address,
                interactionId: "int-perm-2",
                interactionType: .permission,
                decision: .approve,
                commandId: "cmd-approve-perm2",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedResolve(expiredPermIntent)
            let expiredOutcome = lastHostActionReceipt?.outcome
            let expiredReason = zcodeSessions.lastCommandReply?.reasonCode
            await queuedResolve(validQuestionAnswer)
            let dupOutcome = lastHostActionReceipt?.outcome
            let dupFlag = zcodeSessions.lastCommandReply?.duplicate
            await queuedResolve(validPermIntent)
            let permOutcome = lastHostActionReceipt?.outcome
            await fixture.configureInteraction(
                second.address,
                interaction: ZCodePendingInteraction(
                    interactionId: "int-plan-1",
                    interactionType: .plan,
                    toolName: "ExitPlanMode"
                )
            )
            await zcodeSessions.refresh()
            guard let validPlanIntent = zcodeSessions.resolveIntent(
                for: second.address,
                interactionId: "int-plan-1",
                interactionType: .plan,
                decision: .approve,
                commandId: "cmd-approve-plan1",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedResolve(validPlanIntent)
            let planOutcome = lastHostActionReceipt?.outcome
            let afterInteractions = await fixture.counts()
            checks["expired_and_duplicate_interaction_zero_misapproval"] = expiredOutcome == "noop"
                && expiredReason == "proto.alreadyResolved"
                && dupOutcome == "resolved"
                && dupFlag == true
                && permOutcome == "resolved"
                && planOutcome == "resolved"
                && afterInteractions.resolves == 3
                && afterInteractions.resolvedInteractions == ["int-q-1", "int-perm-2", "int-plan-1"]

            // 9. Unsupported thought level is not sent; model & thought readback matches
            guard let unsupportedSwitch = zcodeSessions.switchModelIntent(
                for: second.address,
                provider: "glm",
                model: "glm-5.1",
                thought: "xhigh",
                commandId: "cmd-switch-unsupported",
                clientId: "qa-client"
            ),
            let validSwitch = zcodeSessions.switchModelIntent(
                for: second.address,
                provider: "glm",
                model: "glm-5.1",
                thought: "high",
                commandId: "cmd-switch-high",
                clientId: "qa-client"
            ),
            let corruptedSwitch = zcodeSessions.switchModelIntent(
                for: second.address,
                provider: "glm",
                model: "glm-5.1",
                thought: "low",
                commandId: "cmd-switch-corrupted",
                clientId: "qa-client"
            ) else { throw ZCodeFailure.invalidReply }
            await queuedSwitchModel(unsupportedSwitch)
            let unsupportedOutcome = lastHostActionReceipt?.outcome
            let unsupportedReason = zcodeSessions.lastCommandReply?.reasonCode
            let afterUnsupportedSwitch = await fixture.counts()
            checks["unsupported_thought_not_sent"] = unsupportedOutcome == "unsupportedThought"
                && unsupportedReason == "guard.unsupportedThoughtLevel"
                && afterUnsupportedSwitch.switchModels == 0

            await queuedSwitchModel(validSwitch)
            let validSwitchOutcome = lastHostActionReceipt?.outcome
            let switchedRowConfig = zcodeSessions.rows.first(where: { $0.address == second.address })?.modelConfig
            await fixture.setCorruptNextModelReadback(true)
            await queuedSwitchModel(corruptedSwitch)
            let corruptedOutcome = lastHostActionReceipt?.outcome
            let corruptedReason = zcodeSessions.lastCommandReply?.reasonCode
            let preservedRowConfig = zcodeSessions.rows.first(where: { $0.address == second.address })?.modelConfig
            let afterModelSwitches = await fixture.counts()
            checks["model_and_thought_readback_consistent"] = validSwitchOutcome == "modelSwitched"
                && switchedRowConfig?.provider == "glm"
                && switchedRowConfig?.model == "glm-5.1"
                && switchedRowConfig?.thought == "high"
                && corruptedOutcome == "unconfirmed"
                && corruptedReason == "guard.modelConfigReadbackMismatch"
                && preservedRowConfig?.thought == "high"
                && afterModelSwitches.switchModels == 2

            // Reset to second selected for remaining navigation boundary checks
            await queuedOpen(second)
            let navBaseline = await fixture.counts()
            await fixture.setMode(.wrongSelected)
            await queuedOpen(first)
            checks["wrong_selected_unconfirmed"] = lastHostActionReceipt?.outcome != "confirmed"
            let outsider = ZCodeSelection(address: ZCodeTaskAddress(workspacePath: "/qa/not-authorized", taskId: "same-id"),
                instanceId: first.instanceId, generation: first.generation, localEpoch: first.localEpoch)
            let beforeDenied = await fixture.counts()
            await queuedOpen(outsider)
            let afterDenied = await fixture.counts()
            checks["out_of_scope_zero_reads"] = beforeDenied.reads == afterDenied.reads && beforeDenied.opens == afterDenied.opens
            await fixture.setMode(.deleted)
            await queuedOpen(first)
            let deleted = await fixture.counts()
            checks["deleted_target_no_redirect"] = lastHostActionReceipt?.outcome != "confirmed"
                && deleted.navigations == navBaseline.navigations
            zcodeSessions.setEnabled(false)
            await fixture.setMode(.normal)
            zcodeSessions.setEnabled(true)
            try await waitReady()
            guard let preRotation = zcodeSessions.selection(for: ZCodeNavigationFixture.second) else { throw ZCodeFailure.invalidReply }
            await fixture.setMode(.rotated)
            await zcodeSessions.refresh()
            let beforeOld = await fixture.counts()
            await queuedOpen(preRotation)
            let afterOld = await fixture.counts()
            checks["generation_change_blocks_old_selection"] = beforeOld.opens == afterOld.opens
                && afterOld.navigations == navBaseline.navigations
            zcodeSessions.setEnabled(false)
            await fixture.setMode(.normal)
            zcodeSessions.setEnabled(true)
            try await waitReady()
            await fixture.setMode(.disconnected)
            await zcodeSessions.refresh()
            checks["disconnect_clears_targets"] = zcodeSessions.rows.isEmpty && zcodeSessions.state != .ready
            zcodeSessions.setEnabled(false)
        } catch {
            checks["scenario_completed"] = false
        }
        let payload: [String: Any] = ["status": checks.count == 18 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "checks": checks, "body_reads": 0, "native_app": "NOT_RUN", "physical_key": "NOT_RUN"]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("zcode-navigation.json"))
        }
    }
}
#endif
