#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

enum CindyOpenOverride: Sendable {
    case normal
    case deliveredOnly
    case wrongSelected(CindyTaskAddress)
    case missingSelected
    case rotateGeneration(UInt64)
    case rotateOwner(String, UInt64)
}

actor CindyNavigationFixture: CindySessionServing {
    static let shared = CindyNavigationFixture()

    private var instanceId = "cindy-instance-1"
    private var generation: UInt64 = 1
    private var ownerEpoch = "owner-account-1:1"
    private var scopes: [CindyScope] = [
        CindyScope(workspacePath: "/Users/example/Projects/alpha"),
        CindyScope(workspacePath: "/Users/example/Projects/beta"),
    ]
    private var allRows: [CindyTaskRow] = [
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/alpha",
                sessionId: "sess-alpha-1"
            ),
            title: "Daily Standup",
            phase: "running",
            attention: false,
            pinned: true,
            runId: "run-alpha-1",
            unread: false,
            eventWatermark: 10
        ),
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/alpha",
                sessionId: "sess-alpha-2"
            ),
            title: "Daily Standup",
            phase: "needs-interaction",
            attention: true,
            pinned: false,
            runId: "run-alpha-2",
            unread: false,
            eventWatermark: 20,
            pendingInteraction: CindyPendingInteraction(
                interactionId: "perm-alpha-2",
                interactionType: .permission,
                toolName: "bash"
            )
        ),
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/beta",
                sessionId: "sess-beta-1"
            ),
            title: "Daily Standup",
            phase: "idle",
            attention: false,
            pinned: false,
            runId: nil,
            unread: nil,
            eventWatermark: 5
        ),
        CindyTaskRow(
            address: CindyTaskAddress(
                workspacePath: "/Users/example/Projects/secret",
                sessionId: "sess-secret-9"
            ),
            title: "Secret Workspace Task",
            phase: "running",
            attention: true,
            pinned: false,
            runId: "run-secret-9",
            unread: true,
            eventWatermark: 90
        ),
    ]
    private var delivered: CindyTaskAddress?
    private var selected: CindyTaskAddress? = CindyTaskAddress(
        workspacePath: "/Users/example/Projects/alpha",
        sessionId: "sess-alpha-1"
    )
    private var openOverride: CindyOpenOverride = .normal
    private var authorizedMetadataReads = 0
    private var unauthorizedMetadataReads = 0
    private var openHistory: [CindyTaskAddress] = []
    private var createCount = 0
    private var sendCount = 0
    private var stopCount = 0
    private var resolveCount = 0
    private var timeoutNextCommand = false
    private var recordedCommands: [String: CindyCommandReply] = [:]

    func reset() {
        instanceId = "cindy-instance-1"
        generation = 1
        ownerEpoch = "owner-account-1:1"
        scopes = [
            CindyScope(workspacePath: "/Users/example/Projects/alpha"),
            CindyScope(workspacePath: "/Users/example/Projects/beta"),
        ]
        allRows = [
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/alpha",
                    sessionId: "sess-alpha-1"
                ),
                title: "Daily Standup",
                phase: "running",
                attention: false,
                pinned: true,
                runId: "run-alpha-1",
                unread: false,
                eventWatermark: 10
            ),
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/alpha",
                    sessionId: "sess-alpha-2"
                ),
                title: "Daily Standup",
                phase: "needs-interaction",
                attention: true,
                pinned: false,
                runId: "run-alpha-2",
                unread: false,
                eventWatermark: 20,
                pendingInteraction: CindyPendingInteraction(
                    interactionId: "perm-alpha-2",
                    interactionType: .permission,
                    toolName: "bash"
                )
            ),
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/beta",
                    sessionId: "sess-beta-1"
                ),
                title: "Daily Standup",
                phase: "idle",
                attention: false,
                pinned: false,
                runId: nil,
                unread: nil,
                eventWatermark: 5
            ),
            CindyTaskRow(
                address: CindyTaskAddress(
                    workspacePath: "/Users/example/Projects/secret",
                    sessionId: "sess-secret-9"
                ),
                title: "Secret Workspace Task",
                phase: "running",
                attention: true,
                pinned: false,
                runId: "run-secret-9",
                unread: true,
                eventWatermark: 90
            ),
        ]
        delivered = nil
        selected = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/alpha",
            sessionId: "sess-alpha-1"
        )
        openOverride = .normal
        authorizedMetadataReads = 0
        unauthorizedMetadataReads = 0
        openHistory = []
        createCount = 0
        sendCount = 0
        stopCount = 0
        resolveCount = 0
        timeoutNextCommand = false
        recordedCommands.removeAll()
    }

    func setOpenOverride(_ overrideMode: CindyOpenOverride) {
        openOverride = overrideMode
    }

    func setTimeoutNextCommand(_ enabled: Bool) {
        timeoutNextCommand = enabled
    }

    func upsertTask(_ row: CindyTaskRow) {
        if let idx = allRows.firstIndex(where: { $0.address == row.address }) {
            allRows[idx] = row
        } else {
            allRows.append(row)
        }
    }

    func removeTask(_ address: CindyTaskAddress) {
        allRows.removeAll { $0.address == address }
        if delivered == address {
            delivered = nil
        }
        if selected == address {
            selected = nil
        }
    }

    func stats() -> (authorizedReads: Int, unauthorizedReads: Int, openHistory: [CindyTaskAddress]) {
        (authorizedMetadataReads, unauthorizedMetadataReads, openHistory)
    }

    func commandCounts() -> (creates: Int, sends: Int, stops: Int, resolves: Int) {
        (createCount, sendCount, stopCount, resolveCount)
    }

    func connect() async -> Result<CindySessionSnapshot, CindyFailure> {
        await read()
    }

    func read() async -> Result<CindySessionSnapshot, CindyFailure> {
        authorizedMetadataReads += 1
        let allowedPaths = Set(scopes.map(\.workspacePath))
        let scopedRows = allRows.filter { allowedPaths.contains($0.address.workspacePath) }
        let scopedSelected = selected.flatMap { current in
            scopedRows.contains(where: { $0.address == current }) ? current : nil
        }
        return .success(
            CindySessionSnapshot(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                scopes: scopes,
                rows: scopedRows,
                selected: scopedSelected
            )
        )
    }

    func open(_ selection: CindySelection) async -> CindyOpenReply {
        let allowedPaths = Set(scopes.map(\.workspacePath))
        guard allowedPaths.contains(selection.address.workspacePath) else {
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .denied,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }

        switch openOverride {
        case .rotateGeneration(let nextGeneration):
            generation = nextGeneration
            openOverride = .normal
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        case .rotateOwner(let nextOwnerEpoch, let nextGeneration):
            ownerEpoch = nextOwnerEpoch
            generation = nextGeneration
            openOverride = .normal
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        case .wrongSelected(let wrongAddress):
            openOverride = .normal
            delivered = selection.address
            selected = wrongAddress
            openHistory.append(selection.address)
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .confirmed,
                delivered: selection.address,
                selected: wrongAddress,
                deepLink: selection.address.deepLink
            )
        case .missingSelected:
            openOverride = .normal
            delivered = selection.address
            selected = nil
            openHistory.append(selection.address)
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .unconfirmed,
                delivered: selection.address,
                selected: nil,
                deepLink: selection.address.deepLink
            )
        case .deliveredOnly:
            openOverride = .normal
            guard selection.instanceId == instanceId,
                  selection.generation == generation,
                  selection.ownerEpoch == ownerEpoch else {
                return CindyOpenReply(
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    delivered: nil,
                    selected: nil,
                    deepLink: nil
                )
            }
            authorizedMetadataReads += 1
            guard allRows.contains(where: { $0.address == selection.address }) else {
                return CindyOpenReply(
                    instanceId: instanceId,
                    generation: generation,
                    ownerEpoch: ownerEpoch,
                    status: .stale,
                    delivered: nil,
                    selected: nil,
                    deepLink: nil
                )
            }
            delivered = selection.address
            selected = nil
            openHistory.append(selection.address)
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .delivered,
                delivered: selection.address,
                selected: nil,
                deepLink: selection.address.deepLink
            )
        case .normal:
            break
        }

        guard selection.instanceId == instanceId,
              selection.generation == generation,
              selection.ownerEpoch == ownerEpoch else {
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }

        authorizedMetadataReads += 1
        guard allRows.contains(where: { $0.address == selection.address }) else {
            return CindyOpenReply(
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                delivered: nil,
                selected: nil,
                deepLink: nil
            )
        }

        delivered = selection.address
        selected = selection.address
        openHistory.append(selection.address)
        return CindyOpenReply(
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            status: .confirmed,
            delivered: selection.address,
            selected: selection.address,
            deepLink: selection.address.deepLink
        )
    }

    func create(_ intent: CindyCreateIntent) async -> CindyCommandReply {
        let key = "\(intent.scope.workspacePath)::\(intent.commandId)"
        if let recorded = recordedCommands[key] {
            var dup = recorded
            dup.duplicate = true
            return dup
        }
        guard scopes.contains(intent.scope) else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .denied,
                reasonCode: "guard.scopeOutOfBounds"
            )
        }
        createCount += 1
        let created = CindyTaskAddress(
            workspacePath: intent.scope.workspacePath,
            sessionId: "sess-created-\(createCount)"
        )
        let runId = intent.text != nil ? "run-created-\(createCount)" : nil
        let newRow = CindyTaskRow(
            address: created,
            title: intent.text ?? "New Task \(createCount)",
            phase: intent.text != nil ? "running" : "idle",
            attention: false,
            pinned: false,
            runId: runId,
            unread: false,
            eventWatermark: UInt64(100 + createCount)
        )
        allRows.append(newRow)
        delivered = created
        selected = created
        let reply = CindyCommandReply(
            commandId: intent.commandId,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            status: .confirmed,
            delivered: created,
            selected: created,
            deepLink: created.deepLink,
            runId: runId
        )
        recordedCommands[key] = reply
        return reply
    }

    func send(_ intent: CindySendIntent) async -> CindyCommandReply {
        let address = intent.selection.address
        let key = "\(address.workspacePath)::\(intent.commandId)"
        if let recorded = recordedCommands[key] {
            var dup = recorded
            dup.duplicate = true
            return dup
        }
        guard selected == address else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .unconfirmed,
                selected: selected,
                reasonCode: "guard.selectionNotCommitted"
            )
        }
        guard let idx = allRows.firstIndex(where: { $0.address == address }) else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                reasonCode: "guard.targetMissing"
            )
        }
        if intent.deliveryMode == .steer, allRows[idx].phase != "running" {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .denied,
                selected: address,
                reasonCode: "guard.steerRequiresRunningTurn"
            )
        }
        sendCount += 1
        let reply: CindyCommandReply
        if allRows[idx].phase != "running" {
            let runId = "run-send-\(sendCount)"
            allRows[idx].phase = "running"
            allRows[idx].runId = runId
            allRows[idx].eventWatermark = (allRows[idx].eventWatermark ?? 0) + 1
            reply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .started,
                selected: address,
                delivery: .auto,
                runId: runId,
                queuedCount: allRows[idx].queuedCount
            )
        } else if intent.deliveryMode == .steer {
            allRows[idx].eventWatermark = (allRows[idx].eventWatermark ?? 0) + 1
            reply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .steered,
                selected: address,
                delivery: .steer,
                runId: allRows[idx].runId,
                queuedCount: allRows[idx].queuedCount
            )
        } else {
            allRows[idx].queuedCount += 1
            allRows[idx].eventWatermark = (allRows[idx].eventWatermark ?? 0) + 1
            reply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .queued,
                selected: address,
                delivery: .queue,
                runId: allRows[idx].runId,
                queuedCount: allRows[idx].queuedCount
            )
        }
        recordedCommands[key] = reply
        if timeoutNextCommand {
            timeoutNextCommand = false
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .unknown,
                reasonCode: "bridge.transportTimeout"
            )
        }
        return reply
    }

    func stop(_ intent: CindyStopIntent) async -> CindyCommandReply {
        let address = intent.selection.address
        let key = "\(address.workspacePath)::\(intent.commandId)"
        if let recorded = recordedCommands[key] {
            var dup = recorded
            dup.duplicate = true
            return dup
        }
        guard selected == address else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .unconfirmed,
                selected: selected,
                reasonCode: "guard.selectionNotCommitted"
            )
        }
        guard let idx = allRows.firstIndex(where: { $0.address == address }) else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                reasonCode: "guard.targetMissing"
            )
        }
        guard allRows[idx].phase == "running",
              let activeRunId = allRows[idx].runId,
              activeRunId == intent.expectedRunId else {
            let noopReply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .noop,
                selected: address,
                runId: allRows[idx].runId,
                reasonCode: allRows[idx].phase != "running" ? "guard.notRunning" : "guard.stopTargetChanged"
            )
            recordedCommands[key] = noopReply
            return noopReply
        }
        stopCount += 1
        allRows[idx].phase = "idle"
        allRows[idx].runId = nil
        allRows[idx].eventWatermark = (allRows[idx].eventWatermark ?? 0) + 1
        let reply = CindyCommandReply(
            commandId: intent.commandId,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            status: .stopped,
            selected: address,
            runId: activeRunId
        )
        recordedCommands[key] = reply
        return reply
    }

    func resolve(_ intent: CindyResolveIntent) async -> CindyCommandReply {
        let address = intent.selection.address
        let key = "\(address.workspacePath)::\(intent.commandId)"
        if let recorded = recordedCommands[key] {
            var dup = recorded
            dup.duplicate = true
            return dup
        }
        if intent.interactionType == .question,
           (intent.decision == .approve || intent.decision == .allowForSession),
           (intent.optionId == nil || intent.optionId?.isEmpty == true) {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .interactionTypeMismatch,
                selected: selected,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: "guard.questionCannotApproveAsPermission"
            )
        }
        guard selected == address else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .unconfirmed,
                selected: selected,
                reasonCode: "guard.selectionNotCommitted"
            )
        }
        guard let idx = allRows.firstIndex(where: { $0.address == address }) else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .stale,
                reasonCode: "guard.targetMissing"
            )
        }
        guard let pending = allRows[idx].pendingInteraction,
              pending.interactionId == intent.interactionId else {
            let noopReply = CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .noop,
                selected: address,
                interactionId: intent.interactionId,
                interactionType: intent.interactionType,
                reasonCode: "guard.interactionAlreadyResolved"
            )
            recordedCommands[key] = noopReply
            return noopReply
        }
        guard pending.interactionType == intent.interactionType else {
            return CindyCommandReply(
                commandId: intent.commandId,
                instanceId: instanceId,
                generation: generation,
                ownerEpoch: ownerEpoch,
                status: .interactionTypeMismatch,
                selected: address,
                interactionId: pending.interactionId,
                interactionType: pending.interactionType,
                reasonCode: "guard.interactionTypeMismatch"
            )
        }
        resolveCount += 1
        allRows[idx].pendingInteraction = nil
        allRows[idx].attention = false
        allRows[idx].phase = (intent.decision == .reject || intent.decision == .cancel) ? "idle" : "running"
        allRows[idx].eventWatermark = (allRows[idx].eventWatermark ?? 0) + 1
        let reply = CindyCommandReply(
            commandId: intent.commandId,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            status: .resolved,
            selected: address,
            interactionId: pending.interactionId,
            interactionType: pending.interactionType
        )
        recordedCommands[key] = reply
        return reply
    }

    func query(_ intent: CindyQueryIntent) async -> CindyCommandReply {
        let key = "\(intent.scope.workspacePath)::\(intent.commandId)"
        if let recorded = recordedCommands[key] {
            var dup = recorded
            dup.duplicate = true
            return dup
        }
        return CindyCommandReply(
            commandId: intent.commandId,
            instanceId: instanceId,
            generation: generation,
            ownerEpoch: ownerEpoch,
            status: .noop,
            reasonCode: "query.notFound"
        )
    }

    func close() async {}
}

struct CindyScenarioCheckReceipt: Codable {
    var name: String
    var status: String
    var detail: String
}

struct CindyNavigationScenarioReceipt: Codable {
    var scenario: String
    var status: String
    var unauthorized_metadata_reads: Int
    var authorized_metadata_reads: Int
    var local_agent_api_relaxed: Bool
    var checks: [CindyScenarioCheckReceipt]
    var native_app: String
    var physical_key: String
}

extension Store {
    func prepareCindyNavigationScenario() async {
        let fixture = CindyNavigationFixture.shared
        await fixture.reset()

        func queuedOpen(_ selection: CindySelection) async {
            _ = openCindySession(selection)
            await hostExecutionTask?.value
        }

        func queuedCommand(_ command: CindyQueuedCommand) async {
            _ = executeCindyCommand(command)
            await hostExecutionTask?.value
        }

        var checks: [CindyScenarioCheckReceipt] = []

        await installCodingAgentPresets()
        if let profile = hostProfiles.first(where: { $0.name == "Cindy" }) {
            await setHostActiveProfile(profile.id.text)
        }

        let initialStats = await fixture.stats()
        let disabledZeroReadsPass =
            !cindySessions.enabled &&
            cindySessions.state == .disabled &&
            initialStats.authorizedReads == 0 &&
            initialStats.unauthorizedReads == 0
        checks.append(
            CindyScenarioCheckReceipt(
                name: "disabled_zero_reads",
                status: disabledZeroReadsPass ? "PASS" : "FAIL",
                detail: "enabled=\(cindySessions.enabled) authorized_reads=\(initialStats.authorizedReads)"
            )
        )

        cindySessions.setEnabled(true)
        await cindySessions.refreshNow()

        let alpha1 = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/alpha",
            sessionId: "sess-alpha-1"
        )
        let alpha2 = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/alpha",
            sessionId: "sess-alpha-2"
        )
        let beta1 = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/beta",
            sessionId: "sess-beta-1"
        )
        let secret = CindyTaskAddress(
            workspacePath: "/Users/example/Projects/secret",
            sessionId: "sess-secret-9"
        )

        let scopedCatalogPass =
            cindySessions.state == .ready &&
            cindySessions.rows.map(\.address) == [alpha1, alpha2, beta1] &&
            !cindySessions.rows.contains(where: { $0.address == secret })
        checks.append(
            CindyScenarioCheckReceipt(
                name: "scoped_catalog_only",
                status: scopedCatalogPass ? "PASS" : "FAIL",
                detail: "rows=\(cindySessions.rows.map(\.id).joined(separator: ","))"
            )
        )

        await fixture.setOpenOverride(.deliveredOnly)
        if let deliveredSelection = cindySessions.selection(for: alpha2) {
            await queuedOpen(deliveredSelection)
        }
        let deliveredOnlyPass =
            cindySessions.lastResult == .delivered &&
            lastHostActionReceipt?.outcome == "delivered" &&
            cindySessions.delivered == alpha2 &&
            cindySessions.selected == nil &&
            cindySessions.lastDeepLink == "cindy://session/sess-alpha-2" &&
            navigationHistory.current != .session(adapter: "cindy", session: "sess-alpha-2")

        if let alpha2Selection = cindySessions.selection(for: alpha2) {
            await queuedOpen(alpha2Selection)
        }
        let confirmedAfterDeliveredPass =
            deliveredOnlyPass &&
            cindySessions.lastResult == .confirmed &&
            lastHostActionReceipt?.outcome == "confirmed" &&
            cindySessions.delivered == alpha2 &&
            cindySessions.selected == alpha2 &&
            cindySessions.lastDeepLink == "cindy://session/sess-alpha-2" &&
            navigationHistory.current == .session(adapter: "cindy", session: "sess-alpha-2")
        checks.append(
            CindyScenarioCheckReceipt(
                name: "delivered_vs_confirmed_separated",
                status: confirmedAfterDeliveredPass ? "PASS" : "FAIL",
                detail: "delivered=\(cindySessions.delivered?.id ?? "nil") selected=\(cindySessions.selected?.id ?? "nil") deepLink=\(cindySessions.lastDeepLink ?? "nil")"
            )
        )

        let sameNamePass =
            cindySessions.lastResult == .confirmed &&
            cindySessions.selected == alpha2 &&
            cindySessions.selected != alpha1 &&
            cindySessions.selected != beta1
        checks.append(
            CindyScenarioCheckReceipt(
                name: "same_name_exact_identity",
                status: sameNamePass ? "PASS" : "FAIL",
                detail: "selected=\(cindySessions.selected?.id ?? "nil")"
            )
        )

        if let beta1Selection = cindySessions.selection(for: beta1) {
            await queuedOpen(beta1Selection)
        }
        let readbackPass =
            cindySessions.lastResult == .confirmed &&
            cindySessions.selected == beta1 &&
            navigationHistory.current == .session(adapter: "cindy", session: "sess-beta-1")
        checks.append(
            CindyScenarioCheckReceipt(
                name: "selected_readback_matches",
                status: readbackPass ? "PASS" : "FAIL",
                detail: "selected=\(cindySessions.selected?.id ?? "nil")"
            )
        )

        await fixture.setOpenOverride(.wrongSelected(alpha1))
        if let mismatchSelection = cindySessions.selection(for: beta1) {
            await queuedOpen(mismatchSelection)
        }
        let unconfirmedPass =
            cindySessions.lastResult == .unconfirmed &&
            lastHostActionReceipt?.outcome == "unconfirmed" &&
            cindySessions.delivered == beta1 &&
            cindySessions.selected == nil
        checks.append(
            CindyScenarioCheckReceipt(
                name: "wrong_selected_unconfirmed",
                status: unconfirmedPass ? "PASS" : "FAIL",
                detail: "result=\(cindySessions.lastResult?.rawValue ?? "nil") selected=\(cindySessions.selected?.id ?? "nil")"
            )
        )

        let readsBeforeOutOfScope = await fixture.stats().authorizedReads
        let forgedReply = await fixture.open(
            CindySelection(
                address: secret,
                instanceId: "cindy-instance-1",
                generation: 1,
                ownerEpoch: "owner-account-1:1",
                localEpoch: 1
            )
        )
        let readsAfterOutOfScope = await fixture.stats()
        let outOfScopeZeroReadsPass =
            forgedReply.status == .denied &&
            readsAfterOutOfScope.authorizedReads == readsBeforeOutOfScope &&
            readsAfterOutOfScope.unauthorizedReads == 0
        checks.append(
            CindyScenarioCheckReceipt(
                name: "out_of_scope_zero_reads",
                status: outOfScopeZeroReadsPass ? "PASS" : "FAIL",
                detail: "status=\(forgedReply.status.rawValue) delta_reads=\(readsAfterOutOfScope.authorizedReads - readsBeforeOutOfScope)"
            )
        )

        await cindySessions.refreshNow()
        if let staleDeletedSelection = cindySessions.selection(for: alpha2) {
            await fixture.removeTask(alpha2)
            let historyBeforeDeletedOpen = await fixture.stats().openHistory.count
            await queuedOpen(staleDeletedSelection)
            let historyAfterDeletedOpen = await fixture.stats().openHistory.count
            let deletedNoRedirectPass =
                cindySessions.lastResult == .stale &&
                lastHostActionReceipt?.outcome == "stale" &&
                cindySessions.state == .unavailable(.stale) &&
                cindySessions.selected == nil &&
                cindySessions.rows.isEmpty &&
                historyAfterDeletedOpen == historyBeforeDeletedOpen
            checks.append(
                CindyScenarioCheckReceipt(
                    name: "deleted_target_no_redirect",
                    status: deletedNoRedirectPass ? "PASS" : "FAIL",
                    detail: "result=\(cindySessions.lastResult?.rawValue ?? "nil") extra_opens=\(historyAfterDeletedOpen - historyBeforeDeletedOpen)"
                )
            )
        }

        await cindySessions.refreshNow()
        if let preRotationSelection = cindySessions.selection(for: alpha1) {
            await fixture.setOpenOverride(.rotateOwner("owner-account-2:2", 2))
            await queuedOpen(preRotationSelection)
            let rotationPass =
                cindySessions.lastResult == .stale &&
                lastHostActionReceipt?.outcome == "stale" &&
                cindySessions.state == .unavailable(.stale) &&
                cindySessions.rows.isEmpty &&
                cindySessions.selected == nil &&
                cindySessions.delivered == nil
            checks.append(
                CindyScenarioCheckReceipt(
                    name: "account_or_generation_change_blocks_old_selection",
                    status: rotationPass ? "PASS" : "FAIL",
                    detail: "state=\(cindySessions.state.label) rows=\(cindySessions.rows.count)"
                )
            )
        }

        // Restore alpha2 with a pending permission interaction and refresh.
        await fixture.upsertTask(
            CindyTaskRow(
                address: alpha2,
                title: "Daily Standup",
                phase: "needs-interaction",
                attention: true,
                pinned: false,
                runId: "run-alpha-2",
                unread: false,
                eventWatermark: 20,
                pendingInteraction: CindyPendingInteraction(
                    interactionId: "perm-alpha-2",
                    interactionType: .permission,
                    toolName: "bash"
                )
            )
        )
        await cindySessions.refreshNow()
        if let currentAlpha1 = cindySessions.selection(for: alpha1) {
            await queuedOpen(currentAlpha1)
        }

        // Slice M Check 10: stop bound to (target, expectedRunId), stale runId is noop with zero aborts.
        let stopsBefore = await fixture.commandCounts().stops
        await queuedCommand(.stop(address: alpha1, expectedRunId: "run-stale-0", commandId: "cmd-stop-stale"))
        let staleStopReply = cindySessions.lastCommandReply
        let stopsAfterStale = await fixture.commandCounts().stops
        await queuedCommand(.stop(address: alpha1, expectedRunId: "run-alpha-1", commandId: "cmd-stop-exact"))
        let exactStopReply = cindySessions.lastCommandReply
        let stopsAfterExact = await fixture.commandCounts().stops
        await queuedCommand(.stop(address: alpha1, expectedRunId: "run-alpha-1", commandId: "cmd-stop-after-idle"))
        let afterIdleStopReply = cindySessions.lastCommandReply
        let stopsAfterIdle = await fixture.commandCounts().stops
        let stopBoundPass =
            staleStopReply?.status == .noop &&
            staleStopReply?.reasonCode == "guard.stopTargetChanged" &&
            stopsAfterStale == stopsBefore &&
            exactStopReply?.status == .stopped &&
            stopsAfterExact == stopsBefore + 1 &&
            afterIdleStopReply?.status == .noop &&
            afterIdleStopReply?.reasonCode == "guard.notRunning" &&
            stopsAfterIdle == stopsBefore + 1
        checks.append(
            CindyScenarioCheckReceipt(
                name: "stop_bound_to_run_and_stale_noop",
                status: stopBoundPass ? "PASS" : "FAIL",
                detail: "stale=\(staleStopReply?.status.rawValue ?? "nil") exact=\(exactStopReply?.status.rawValue ?? "nil") stops=\(stopsAfterIdle - stopsBefore)"
            )
        )

        // Slice M Check 11: resolve bound to (target, interactionId, interactionType) and question guard.
        if let selAlpha2 = cindySessions.selection(for: alpha2) {
            await queuedOpen(selAlpha2)
        }
        let resolvesBefore = await fixture.commandCounts().resolves
        await queuedCommand(
            .resolve(
                address: alpha2,
                interactionId: "perm-alpha-2",
                interactionType: .question,
                decision: .approve,
                optionId: nil,
                commandId: "cmd-resolve-q-guard"
            )
        )
        let questionGuardReply = cindySessions.lastCommandReply
        await queuedCommand(
            .resolve(
                address: alpha2,
                interactionId: "perm-expired-0",
                interactionType: .permission,
                decision: .approve,
                optionId: nil,
                commandId: "cmd-resolve-expired"
            )
        )
        let expiredResolveReply = cindySessions.lastCommandReply
        let resolvesAfterGuards = await fixture.commandCounts().resolves
        await queuedCommand(
            .resolve(
                address: alpha2,
                interactionId: "perm-alpha-2",
                interactionType: .permission,
                decision: .approve,
                optionId: nil,
                commandId: "cmd-resolve-valid"
            )
        )
        let validResolveReply = cindySessions.lastCommandReply
        await queuedCommand(
            .resolve(
                address: alpha2,
                interactionId: "perm-alpha-2",
                interactionType: .permission,
                decision: .approve,
                optionId: nil,
                commandId: "cmd-resolve-after"
            )
        )
        let afterResolveReply = cindySessions.lastCommandReply
        let resolvesAfterAll = await fixture.commandCounts().resolves
        let resolveBoundPass =
            questionGuardReply?.status == .interactionTypeMismatch &&
            questionGuardReply?.reasonCode == "guard.questionCannotApproveAsPermission" &&
            expiredResolveReply?.status == .noop &&
            expiredResolveReply?.reasonCode == "guard.interactionAlreadyResolved" &&
            resolvesAfterGuards == resolvesBefore &&
            validResolveReply?.status == .resolved &&
            afterResolveReply?.status == .noop &&
            resolvesAfterAll == resolvesBefore + 1
        checks.append(
            CindyScenarioCheckReceipt(
                name: "resolve_bound_and_question_guard",
                status: resolveBoundPass ? "PASS" : "FAIL",
                detail: "q_guard=\(questionGuardReply?.status.rawValue ?? "nil") valid=\(validResolveReply?.status.rawValue ?? "nil") resolves=\(resolvesAfterAll - resolvesBefore)"
            )
        )

        // Slice M Check 12: workspace create, target prompt/steering/queue, and timeout query reconciliation.
        let betaScope = CindyScope(workspacePath: "/Users/example/Projects/beta")
        await queuedCommand(.create(scope: betaScope, text: nil, commandId: "cmd-create-beta"))
        let createReply = cindySessions.lastCommandReply
        let createdAddr = createReply?.selected
        if let selBeta1 = cindySessions.selection(for: beta1) {
            await queuedOpen(selBeta1)
        }
        let sendsBefore = await fixture.commandCounts().sends
        await queuedCommand(.send(address: beta1, text: "Steer while idle", deliveryMode: .steer, commandId: "cmd-steer-idle"))
        let steerIdleReply = cindySessions.lastCommandReply
        await queuedCommand(.send(address: beta1, text: "Start turn", deliveryMode: .auto, commandId: "cmd-send-start"))
        let startSendReply = cindySessions.lastCommandReply
        await queuedCommand(.send(address: beta1, text: "Steer running turn", deliveryMode: .steer, commandId: "cmd-send-steer"))
        let steerRunningReply = cindySessions.lastCommandReply
        await fixture.setTimeoutNextCommand(true)
        await queuedCommand(.send(address: beta1, text: "Queue follow-up", deliveryMode: .queue, commandId: "cmd-send-timeout"))
        let timeoutSendReply = cindySessions.lastCommandReply
        await queuedCommand(.send(address: beta1, text: "Blocked while unknown", deliveryMode: .queue, commandId: "cmd-send-blocked"))
        let blockedByUnknownReply = cindySessions.lastCommandReply
        await queuedCommand(.query(scope: betaScope, target: beta1, commandId: "cmd-send-timeout"))
        let reconciledReply = cindySessions.lastCommandReply
        let sendsAfter = await fixture.commandCounts().sends
        let createAndSendPass =
            createReply?.status == .confirmed &&
            createdAddr?.workspacePath == betaScope.workspacePath &&
            steerIdleReply?.status == .denied &&
            startSendReply?.status == .started &&
            steerRunningReply?.status == .steered &&
            timeoutSendReply?.status == .unknown &&
            blockedByUnknownReply?.status == .unknown &&
            blockedByUnknownReply?.reasonCode == "guard.unknownOutcomeRequiresReconcile" &&
            reconciledReply?.status == .queued &&
            reconciledReply?.duplicate == true &&
            cindySessions.pendingUnknownCommand == nil &&
            sendsAfter == sendsBefore + 3
        checks.append(
            CindyScenarioCheckReceipt(
                name: "workspace_create_and_target_prompt_steering",
                status: createAndSendPass ? "PASS" : "FAIL",
                detail: "create=\(createReply?.status.rawValue ?? "nil") start=\(startSendReply?.status.rawValue ?? "nil") steer=\(steerRunningReply?.status.rawValue ?? "nil") reconcile=\(reconciledReply?.status.rawValue ?? "nil")"
            )
        )

        // Slice M Check 13: native unread tri-state (completed != unread) and non-repeating pending round.
        let completedReadAddr = CindyTaskAddress(workspacePath: "/Users/example/Projects/alpha", sessionId: "sess-completed-read")
        let completedUnknownAddr = CindyTaskAddress(workspacePath: "/Users/example/Projects/alpha", sessionId: "sess-completed-unknown")
        let completedUnreadAddr = CindyTaskAddress(workspacePath: "/Users/example/Projects/alpha", sessionId: "sess-completed-unread")
        let errorUnreadAddr = CindyTaskAddress(workspacePath: "/Users/example/Projects/beta", sessionId: "sess-error-unread")
        await fixture.upsertTask(
            CindyTaskRow(
                address: completedReadAddr,
                title: "Completed Read",
                phase: "completed",
                attention: false,
                pinned: false,
                unread: false,
                eventWatermark: 30
            )
        )
        await fixture.upsertTask(
            CindyTaskRow(
                address: completedUnknownAddr,
                title: "Completed Unknown Unread",
                phase: "completed",
                attention: false,
                pinned: false,
                unread: nil,
                eventWatermark: 31
            )
        )
        await fixture.upsertTask(
            CindyTaskRow(
                address: completedUnreadAddr,
                title: "Completed Unread",
                phase: "completed",
                attention: true,
                pinned: false,
                unread: true,
                eventWatermark: 32
            )
        )
        await fixture.upsertTask(
            CindyTaskRow(
                address: errorUnreadAddr,
                title: "Error Unread",
                phase: "error",
                attention: true,
                pinned: false,
                unread: true,
                eventWatermark: 33
            )
        )
        await cindySessions.refreshNow()
        cindySessions.resetAttentionRound()
        await queuedCommand(.nextAttention)
        let firstRoundPick = cindySessions.lastCommandReply?.selected
        await queuedCommand(.nextAttention)
        let secondRoundPick = cindySessions.lastCommandReply?.selected
        await queuedCommand(.nextAttention)
        let thirdRoundExhausted = cindySessions.lastCommandReply
        // Advance watermark on completedUnreadAddr so it re-enters the round immediately.
        await fixture.upsertTask(
            CindyTaskRow(
                address: completedUnreadAddr,
                title: "Completed Unread",
                phase: "completed",
                attention: true,
                pinned: false,
                unread: true,
                eventWatermark: 34
            )
        )
        await cindySessions.refreshNow()
        await queuedCommand(.nextAttention)
        let reenteredPick = cindySessions.lastCommandReply?.selected
        let unreadAndRoundPass =
            firstRoundPick == errorUnreadAddr &&
            secondRoundPick == completedUnreadAddr &&
            firstRoundPick != completedReadAddr &&
            firstRoundPick != completedUnknownAddr &&
            secondRoundPick != completedReadAddr &&
            secondRoundPick != completedUnknownAddr &&
            thirdRoundExhausted?.status == .noop &&
            thirdRoundExhausted?.reasonCode == "round.exhausted" &&
            reenteredPick == completedUnreadAddr
        checks.append(
            CindyScenarioCheckReceipt(
                name: "unread_tristate_and_pending_round",
                status: unreadAndRoundPass ? "PASS" : "FAIL",
                detail: "first=\(firstRoundPick?.sessionId ?? "nil") second=\(secondRoundPick?.sessionId ?? "nil") exhausted=\(thirdRoundExhausted?.status.rawValue ?? "nil") reentered=\(reenteredPick?.sessionId ?? "nil")"
            )
        )

        let finalStats = await fixture.stats()
        let allPass = checks.count == 13 && checks.allSatisfy { $0.status == "PASS" } && finalStats.unauthorizedReads == 0
        let receipt = CindyNavigationScenarioReceipt(
            scenario: "cindy-navigation",
            status: allPass ? "PASS" : "FAIL",
            unauthorized_metadata_reads: finalStats.unauthorizedReads,
            authorized_metadata_reads: finalStats.authorizedReads,
            local_agent_api_relaxed: false,
            checks: checks,
            native_app: "NOT_RUN",
            physical_key: "NOT_RUN"
        )
        writeCindyScenarioReceipt(receipt)
    }

    private func writeCindyScenarioReceipt(_ receipt: CindyNavigationScenarioReceipt) {
        let env = ProcessInfo.processInfo.environment
        guard let auditPath = env["MI_HOST_AUDIT_LOG"] ?? env["MI_AUDIT_LOG"], !auditPath.isEmpty else {
            return
        }
        let directory = (auditPath as NSString).deletingLastPathComponent
        let outputPath = (directory as NSString).appendingPathComponent("cindy-navigation.json")
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(receipt) else { return }
        try? data.write(to: URL(fileURLWithPath: outputPath), options: .atomic)
    }
}
#endif
