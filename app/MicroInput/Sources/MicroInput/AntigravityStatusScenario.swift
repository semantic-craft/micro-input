import Foundation
import MicroInputCore

actor AntigravityStatusFixture: AntigravityStatusServing {
    static let shared = AntigravityStatusFixture()
    static let workspaceA = "/workspace/ag-alpha"
    static let workspaceB = "/workspace/ag-beta"
    static let sessionA = AntigravitySessionKey(conversationId: "conv-alpha", workspacePath: workspaceA)
    static let sessionB = AntigravitySessionKey(conversationId: "conv-beta", workspacePath: workspaceB)

    enum Mode { case normal, stale, rotated, gap, wrongSelected, missingSchema }

    private var mode: Mode = .normal
    private var connected = false
    private var cursor: UInt64 = 0
    private var rows: [AntigravitySessionKey: AntigravitySessionRow] = [:]
    private var seenKeys: [AntigravitySessionKey: Set<String>] = [:]
    private var pendingChanges: [AntigravityEventChange] = []
    private var selected: AntigravitySessionKey?
    private(set) var metadataReads = 0
    private(set) var duplicateEventsDropped = 0
    private(set) var transcriptReads = 0
    private(set) var tokenReads = 0
    private(set) var openRequests = 0
    private(set) var openConfirmed = 0
    private(set) var createRequests = 0
    private(set) var createConfirmed = 0
    private(set) var sendRequests = 0
    private(set) var sendConfirmed = 0
    private(set) var queryRequests = 0
    private(set) var stopRequests = 0
    private(set) var stopConfirmed = 0
    private(set) var resolveRequests = 0
    private(set) var resolveConfirmed = 0
    private(set) var switchModelRequests = 0
    private(set) var switchModelConfirmed = 0
    private(set) var duplicateCommandsDropped = 0
    private var createdSeq = 0
    private var workspaceProjects: [String: String] = [:]
    private var commandLedger: [String: AntigravityCommandReply] = [:]
    private var expiredInteractions: [AntigravitySessionKey: Set<String>] = [:]
    private var stopSchemaVerified = true
    private var approvalSchemaVerified = true
    private var modelConfigSchemaVerified = true
    private var csrfAuthVerified = true
    private var modelReadbackOverride: (modelName: String?, thinkingLevel: String?)?
    private var unknownNextCommand = false
    private var inFlightSteerEnabled = false
    private(set) var closes = 0

    func reset() {
        mode = .normal
        connected = false
        cursor = 0
        rows = [:]
        seenKeys = [:]
        pendingChanges = []
        selected = nil
        metadataReads = 0
        duplicateEventsDropped = 0
        transcriptReads = 0
        tokenReads = 0
        openRequests = 0
        openConfirmed = 0
        createRequests = 0
        createConfirmed = 0
        sendRequests = 0
        sendConfirmed = 0
        queryRequests = 0
        stopRequests = 0
        stopConfirmed = 0
        resolveRequests = 0
        resolveConfirmed = 0
        switchModelRequests = 0
        switchModelConfirmed = 0
        duplicateCommandsDropped = 0
        createdSeq = 0
        workspaceProjects = [:]
        commandLedger = [:]
        expiredInteractions = [:]
        stopSchemaVerified = true
        approvalSchemaVerified = true
        modelConfigSchemaVerified = true
        csrfAuthVerified = true
        modelReadbackOverride = nil
        unknownNextCommand = false
        inFlightSteerEnabled = false
        closes = 0
    }

    func setMode(_ value: Mode) { mode = value }
    func setUnknownNextCommand(_ enabled: Bool) { unknownNextCommand = enabled }
    func setInFlightSteerEnabled(_ enabled: Bool) { inFlightSteerEnabled = enabled }
    func setL3Schema(stop: Bool = true, approval: Bool = true, modelConfig: Bool = true) {
        stopSchemaVerified = stop
        approvalSchemaVerified = approval
        modelConfigSchemaVerified = modelConfig
    }
    func setCsrfAuth(_ valid: Bool) { csrfAuthVerified = valid }
    func setModelReadbackOverride(modelName: String?, thinkingLevel: String?) {
        if modelName == nil && thinkingLevel == nil {
            modelReadbackOverride = nil
        } else {
            modelReadbackOverride = (modelName, thinkingLevel)
        }
    }

    func setPendingInteraction(for key: AntigravitySessionKey, pending: AntigravityPendingInteraction?) {
        guard let prior = rows[key] else { return }
        let updated = AntigravitySessionRow(
            conversationId: prior.conversationId,
            workspacePath: prior.workspacePath,
            title: prior.title,
            projectId: prior.projectId,
            modelName: prior.modelName,
            thinkingLevel: prior.thinkingLevel,
            phase: pending != nil ? .runningTool : prior.phase,
            lastEventType: prior.lastEventType,
            invocationNum: max(1, prior.invocationNum),
            stepIdx: pending?.stepIndex ?? prior.stepIdx,
            executionNum: max(1, prior.executionNum),
            lastToolName: prior.lastToolName,
            terminationReason: nil,
            fullyIdle: false,
            transitionCount: prior.transitionCount + 1,
            pendingInteraction: pending,
            availableModels: prior.availableModels
        )
        rows[key] = updated
        cursor += 1
        let evKey = "\(key.conversationId):\(key.workspacePath):pending:\(pending?.interactionId ?? "clear"):\(cursor)"
        seenKeys[key, default: []].insert(evKey)
        pendingChanges.append(AntigravityEventChange(sequence: cursor, eventKey: evKey, row: updated))
    }

    func expirePendingInteraction(for key: AntigravitySessionKey, interactionId: String) {
        expiredInteractions[key, default: []].insert(interactionId)
        guard let prior = rows[key], prior.pendingInteraction?.interactionId == interactionId else { return }
        let updated = AntigravitySessionRow(
            conversationId: prior.conversationId,
            workspacePath: prior.workspacePath,
            title: prior.title,
            projectId: prior.projectId,
            modelName: prior.modelName,
            thinkingLevel: prior.thinkingLevel,
            phase: prior.phase,
            lastEventType: prior.lastEventType,
            invocationNum: prior.invocationNum,
            stepIdx: prior.stepIdx,
            executionNum: prior.executionNum,
            lastToolName: prior.lastToolName,
            terminationReason: prior.terminationReason,
            fullyIdle: prior.fullyIdle,
            transitionCount: prior.transitionCount + 1,
            pendingInteraction: nil,
            availableModels: prior.availableModels
        )
        rows[key] = updated
        cursor += 1
        let evKey = "\(key.conversationId):\(key.workspacePath):expire:\(interactionId):\(cursor)"
        seenKeys[key, default: []].insert(evKey)
        pendingChanges.append(AntigravityEventChange(sequence: cursor, eventKey: evKey, row: updated))
    }

    func deleteSession(_ key: AntigravitySessionKey) {
        rows.removeValue(forKey: key)
        if selected == key { selected = nil }
    }

    func counts() -> (reads: Int, duplicates: Int, transcriptReads: Int, tokenReads: Int, openRequests: Int, openConfirmed: Int, closes: Int, cursor: UInt64) {
        (metadataReads, duplicateEventsDropped, transcriptReads, tokenReads, openRequests, openConfirmed, closes, cursor)
    }

    func commandCounts() -> (creates: Int, createsConfirmed: Int, sends: Int, sendsConfirmed: Int, queries: Int, stops: Int, stopsConfirmed: Int, resolves: Int, resolvesConfirmed: Int, modelSwitches: Int, modelSwitchesConfirmed: Int, duplicates: Int) {
        (createRequests, createConfirmed, sendRequests, sendConfirmed, queryRequests, stopRequests, stopConfirmed, resolveRequests, resolveConfirmed, switchModelRequests, switchModelConfirmed, duplicateCommandsDropped)
    }

    private var generation: UInt64 { mode == .rotated ? 2 : 1 }
    private var activeCapabilities: AntigravityCapabilities {
        if mode == .missingSchema {
            return AntigravityCapabilities(
                bundleId: "com.google.antigravity",
                installedVersion: "2.29.0",
                languageServerCl: "1013060670",
                agentapiCommands: ["get-conversation-metadata", "new-conversation", "send-message"],
                sidecarSdkVersion: "1.0.0",
                hookEvents: ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"],
                supportsSessionCatalog: false,
                supportsTargetFocus: false,
                supportsStopTurn: false,
                supportsApprovalGate: false,
                supportsModelConfig: false,
                schemaVerified: false,
                stopSchemaVerified: false,
                approvalSchemaVerified: false,
                modelConfigSchemaVerified: false,
                csrfAuthVerified: false,
                arbitraryRpcExposed: false
            )
        }
        return AntigravityCapabilities(
            bundleId: "com.google.antigravity",
            installedVersion: "2.29.0",
            languageServerCl: "1013060670",
            agentapiCommands: ["get-conversation-metadata", "new-conversation", "send-message"],
            sidecarSdkVersion: "1.0.0",
            hookEvents: ["SessionStart", "PreInvocation", "PostInvocation", "PreToolUse", "PostToolUse", "Stop"],
            supportsSessionCatalog: true,
            supportsTargetFocus: true,
            supportsStopTurn: stopSchemaVerified && csrfAuthVerified,
            supportsApprovalGate: approvalSchemaVerified && csrfAuthVerified,
            supportsModelConfig: modelConfigSchemaVerified && csrfAuthVerified,
            schemaVerified: true,
            stopSchemaVerified: stopSchemaVerified,
            approvalSchemaVerified: approvalSchemaVerified,
            modelConfigSchemaVerified: modelConfigSchemaVerified,
            csrfAuthVerified: csrfAuthVerified,
            arbitraryRpcExposed: false
        )
    }

    @discardableResult
    func ingest(_ event: AntigravityHookEvent) throws -> Bool {
        guard [Self.workspaceA, Self.workspaceB].contains(event.workspacePath) else {
            throw AntigravityFailure.denied
        }
        let sessionKey = event.sessionKey
        var seen = seenKeys[sessionKey] ?? []
        let prior = rows[sessionKey]
        let eventKey = event.eventKey(prior: prior)
        if seen.contains(eventKey) {
            duplicateEventsDropped += 1
            return false
        }
        seen.insert(eventKey)
        seenKeys[sessionKey] = seen
        let updated = AntigravitySessionRow.reduce(prior: prior, event: event)
        if !updated.projectId.isEmpty && workspaceProjects[event.workspacePath] == nil {
            workspaceProjects[event.workspacePath] = updated.projectId
        }
        rows[sessionKey] = updated
        cursor += 1
        pendingChanges.append(AntigravityEventChange(sequence: cursor, eventKey: eventKey, row: updated))
        return true
    }

    func connect() async throws -> AntigravityStatusSnapshot {
        connected = true
        pendingChanges = []
        return try await read()
    }

    func read() async throws -> AntigravityStatusSnapshot {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema else { throw AntigravityFailure.denied }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard mode != .gap else { throw AntigravityFailure.gap }
        metadataReads += 1
        let sortedRows = rows.keys.sorted().compactMap { rows[$0] }
        let validSelected = selected.flatMap { rows[$0] != nil ? $0 : nil }
        let snapshot = AntigravityStatusSnapshot(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            allowedWorkspaces: [Self.workspaceA, Self.workspaceB],
            capabilities: activeCapabilities,
            cursor: cursor,
            rows: sortedRows,
            changes: pendingChanges,
            selected: validSelected,
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
        pendingChanges = []
        return snapshot
    }

    func open(_ target: AntigravitySessionKey) async throws -> AntigravityOpenReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.supportsCatalogNavigation else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard [Self.workspaceA, Self.workspaceB].contains(target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        openRequests += 1
        metadataReads += 1
        guard rows[target] != nil else {
            return AntigravityOpenReply(
                instanceId: "qa-antigravity-instance",
                generation: generation,
                status: "unconfirmed",
                selected: selected.flatMap { rows[$0] != nil ? $0 : nil },
                transcriptReads: UInt64(transcriptReads),
                tokenReads: UInt64(tokenReads)
            )
        }
        if mode == .wrongSelected {
            let wrongKey = target == Self.sessionA ? Self.sessionB : Self.sessionA
            if rows[wrongKey] != nil {
                selected = wrongKey
            }
            return AntigravityOpenReply(
                instanceId: "qa-antigravity-instance",
                generation: generation,
                status: "unconfirmed",
                selected: selected,
                transcriptReads: UInt64(transcriptReads),
                tokenReads: UInt64(tokenReads)
            )
        }
        selected = target
        openConfirmed += 1
        return AntigravityOpenReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: "ok",
            selected: target,
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
    }

    private func sortedRows() -> [AntigravitySessionRow] {
        rows.keys.sorted().compactMap { rows[$0] }
    }

    private func validSelected() -> AntigravitySessionKey? {
        selected.flatMap { rows[$0] != nil ? $0 : nil }
    }

    private func makeCommandReply(
        from entry: AntigravityCommandReply,
        statusOverride: String? = nil,
        reasonOverride: String? = nil,
        duplicate: Bool = false,
        reconciled: Bool = false
    ) -> AntigravityCommandReply {
        pendingChanges = []
        return AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: statusOverride ?? entry.status,
            commandOp: entry.commandOp,
            requestId: entry.requestId,
            conversationId: entry.conversationId,
            workspacePath: entry.workspacePath,
            projectId: entry.projectId,
            delivery: entry.delivery,
            inFlightSteering: entry.inFlightSteering,
            reasonCode: reasonOverride ?? entry.reasonCode,
            duplicate: duplicate,
            reconciled: reconciled,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
    }

    private func makeUnconfirmedReply(
        commandOp: String,
        requestId: String,
        conversationId: String,
        workspacePath: String,
        projectId: String,
        reasonCode: String
    ) -> AntigravityCommandReply {
        pendingChanges = []
        return AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: "unconfirmed",
            commandOp: commandOp,
            requestId: requestId,
            conversationId: conversationId,
            workspacePath: workspacePath,
            projectId: projectId,
            delivery: .none,
            inFlightSteering: false,
            reasonCode: reasonCode,
            duplicate: false,
            reconciled: false,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
    }

    func create(_ intent: AntigravityCreateIntent) async throws -> AntigravityCommandReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.supportsCreateConversation else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard intent.valid, [Self.workspaceA, Self.workspaceB].contains(intent.workspacePath) else {
            throw AntigravityFailure.denied
        }
        createRequests += 1
        metadataReads += 1

        if let existing = commandLedger[intent.requestId] {
            guard existing.commandOp == "create",
                  existing.workspacePath == intent.workspacePath,
                  existing.projectId == intent.projectId else {
                return makeUnconfirmedReply(
                    commandOp: "create",
                    requestId: intent.requestId,
                    conversationId: "",
                    workspacePath: intent.workspacePath,
                    projectId: intent.projectId,
                    reasonCode: "guard.requestIdConflict"
                )
            }
            duplicateCommandsDropped += 1
            return makeCommandReply(from: existing, duplicate: true, reconciled: false)
        }

        if let boundProject = workspaceProjects[intent.workspacePath], boundProject != intent.projectId {
            return makeUnconfirmedReply(
                commandOp: "create",
                requestId: intent.requestId,
                conversationId: "",
                workspacePath: intent.workspacePath,
                projectId: intent.projectId,
                reasonCode: "guard.projectIdMismatch"
            )
        }

        workspaceProjects[intent.workspacePath] = intent.projectId
        createdSeq += 1
        let convId: String
        if let requested = intent.conversationId, !requested.isEmpty,
           rows[AntigravitySessionKey(conversationId: requested, workspacePath: intent.workspacePath)] == nil {
            convId = requested
        } else {
            convId = "conv-created-\(createdSeq)"
        }
        let targetKey = AntigravitySessionKey(conversationId: convId, workspacePath: intent.workspacePath)
        let newRow = AntigravitySessionRow(
            conversationId: convId,
            workspacePath: intent.workspacePath,
            title: intent.title ?? "Conversation \(createdSeq)",
            projectId: intent.projectId,
            modelName: intent.modelName ?? "flash",
            phase: .running,
            lastEventType: .preInvocation,
            invocationNum: 1,
            stepIdx: 0,
            executionNum: 1,
            lastToolName: nil,
            terminationReason: nil,
            fullyIdle: false,
            transitionCount: 1
        )
        rows[targetKey] = newRow
        cursor += 1
        let evKey = "\(convId):\(intent.workspacePath):create:\(intent.requestId)"
        seenKeys[targetKey, default: []].insert(evKey)
        pendingChanges = []

        let status: String
        let reasonCode: String
        if mode == .wrongSelected {
            let wrongKey = targetKey == Self.sessionA ? Self.sessionB : Self.sessionA
            if rows[wrongKey] != nil { selected = wrongKey }
            status = "unconfirmed"
            reasonCode = "guard.selectionReadbackMismatch"
        } else {
            selected = targetKey
            createConfirmed += 1
            status = "created"
            reasonCode = "agentapi.newConversationConfirmed"
        }

        let settled = AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: status,
            commandOp: "create",
            requestId: intent.requestId,
            conversationId: convId,
            workspacePath: intent.workspacePath,
            projectId: intent.projectId,
            delivery: status == "created" ? .started : .none,
            inFlightSteering: false,
            reasonCode: reasonCode,
            duplicate: false,
            reconciled: false,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
        commandLedger[intent.requestId] = settled
        if unknownNextCommand {
            unknownNextCommand = false
            return makeCommandReply(
                from: settled,
                statusOverride: "unknown",
                reasonOverride: "bridge.deliveryUnconfirmed",
                duplicate: false,
                reconciled: false
            )
        }
        return settled
    }

    func send(_ intent: AntigravitySendIntent) async throws -> AntigravityCommandReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.supportsSendMessage else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard intent.valid, [Self.workspaceA, Self.workspaceB].contains(intent.target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        sendRequests += 1
        metadataReads += 1

        if let existing = commandLedger[intent.requestId] {
            guard existing.commandOp == "send",
                  existing.conversationId == intent.target.conversationId,
                  existing.workspacePath == intent.target.workspacePath else {
                return makeUnconfirmedReply(
                    commandOp: "send",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: intent.projectId ?? "",
                    reasonCode: "guard.requestIdConflict"
                )
            }
            duplicateCommandsDropped += 1
            return makeCommandReply(from: existing, duplicate: true, reconciled: false)
        }

        guard let prior = rows[intent.target] else {
            return makeUnconfirmedReply(
                commandOp: "send",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: intent.projectId ?? "",
                reasonCode: "guard.targetNotFound"
            )
        }
        guard selected == intent.target else {
            return makeUnconfirmedReply(
                commandOp: "send",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: prior.projectId,
                reasonCode: "guard.selectionMismatch"
            )
        }
        if let reqProj = intent.projectId, !reqProj.isEmpty, !prior.projectId.isEmpty, reqProj != prior.projectId {
            return makeUnconfirmedReply(
                commandOp: "send",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: prior.projectId,
                reasonCode: "guard.projectIdMismatch"
            )
        }

        let resolvedProject = !prior.projectId.isEmpty ? prior.projectId : (intent.projectId ?? "")
        let inFlight = [.running, .runningTool, .backgroundActive].contains(prior.phase)
        let updatedRow: AntigravitySessionRow
        let status: String
        let delivery: AntigravityDeliveryOutcome
        let inFlightSteering: Bool
        let reasonCode: String

        if !inFlight {
            updatedRow = AntigravitySessionRow(
                conversationId: prior.conversationId,
                workspacePath: prior.workspacePath,
                title: prior.title,
                projectId: resolvedProject,
                modelName: prior.modelName,
                phase: .running,
                lastEventType: .preInvocation,
                invocationNum: prior.invocationNum + 1,
                stepIdx: prior.stepIdx,
                executionNum: prior.executionNum + 1,
                lastToolName: prior.lastToolName,
                terminationReason: nil,
                fullyIdle: false,
                transitionCount: prior.transitionCount + 1
            )
            status = "started"
            delivery = .started
            inFlightSteering = false
            reasonCode = "agentapi.sendMessageStarted"
        } else if inFlightSteerEnabled && intent.deliveryMode == .steer {
            updatedRow = AntigravitySessionRow(
                conversationId: prior.conversationId,
                workspacePath: prior.workspacePath,
                title: prior.title,
                projectId: resolvedProject,
                modelName: prior.modelName,
                phase: prior.phase == .backgroundActive ? .running : prior.phase,
                lastEventType: prior.lastEventType,
                invocationNum: prior.invocationNum,
                stepIdx: prior.stepIdx + 1,
                executionNum: prior.executionNum,
                lastToolName: prior.lastToolName,
                terminationReason: prior.terminationReason,
                fullyIdle: false,
                transitionCount: prior.transitionCount + 1
            )
            status = "steered"
            delivery = .steered
            inFlightSteering = true
            reasonCode = "agentapi.sendMessageSteered"
        } else {
            updatedRow = AntigravitySessionRow(
                conversationId: prior.conversationId,
                workspacePath: prior.workspacePath,
                title: prior.title,
                projectId: resolvedProject,
                modelName: prior.modelName,
                phase: prior.phase,
                lastEventType: prior.lastEventType,
                invocationNum: prior.invocationNum,
                stepIdx: prior.stepIdx,
                executionNum: prior.executionNum,
                lastToolName: prior.lastToolName,
                terminationReason: prior.terminationReason,
                fullyIdle: false,
                transitionCount: prior.transitionCount + 1
            )
            status = "queued"
            delivery = .nextInvocation
            inFlightSteering = false
            reasonCode = intent.deliveryMode == .steer
                ? "agentapi.steerUnsupportedQueuedNextInvocation"
                : "agentapi.sendMessageQueuedNextInvocation"
        }

        rows[intent.target] = updatedRow
        cursor += 1
        let evKey = "\(intent.target.conversationId):\(intent.target.workspacePath):send:\(intent.requestId)"
        seenKeys[intent.target, default: []].insert(evKey)
        pendingChanges = []
        sendConfirmed += 1

        let settled = AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: status,
            commandOp: "send",
            requestId: intent.requestId,
            conversationId: intent.target.conversationId,
            workspacePath: intent.target.workspacePath,
            projectId: resolvedProject,
            delivery: delivery,
            inFlightSteering: inFlightSteering,
            reasonCode: reasonCode,
            duplicate: false,
            reconciled: false,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
        commandLedger[intent.requestId] = settled
        if unknownNextCommand {
            unknownNextCommand = false
            return makeCommandReply(
                from: settled,
                statusOverride: "unknown",
                reasonOverride: "bridge.deliveryUnconfirmed",
                duplicate: false,
                reconciled: false
            )
        }
        return settled
    }

    func query(_ intent: AntigravityQueryIntent) async throws -> AntigravityCommandReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.schemaVerified else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard intent.valid else { throw AntigravityFailure.denied }
        queryRequests += 1
        metadataReads += 1

        guard let existing = commandLedger[intent.requestId] else {
            return makeUnconfirmedReply(
                commandOp: intent.expectedOp ?? "query",
                requestId: intent.requestId,
                conversationId: intent.conversationId ?? "",
                workspacePath: intent.workspacePath ?? "",
                projectId: intent.projectId ?? "",
                reasonCode: "query.requestNotFound"
            )
        }
        return makeCommandReply(from: existing, duplicate: true, reconciled: true)
    }

    func stop(_ intent: AntigravityStopIntent) async throws -> AntigravityCommandReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.supportsStopConversation else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard intent.valid, [Self.workspaceA, Self.workspaceB].contains(intent.target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        stopRequests += 1
        metadataReads += 1

        if let existing = commandLedger[intent.requestId] {
            guard existing.commandOp == "stop",
                  existing.conversationId == intent.target.conversationId,
                  existing.workspacePath == intent.target.workspacePath else {
                return makeUnconfirmedReply(
                    commandOp: "stop",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: "",
                    reasonCode: "guard.requestIdConflict"
                )
            }
            duplicateCommandsDropped += 1
            return makeCommandReply(from: existing, duplicate: true, reconciled: false)
        }

        guard let prior = rows[intent.target] else {
            return makeUnconfirmedReply(
                commandOp: "stop",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: "",
                reasonCode: "guard.targetNotFound"
            )
        }
        guard selected == intent.target else {
            return makeUnconfirmedReply(
                commandOp: "stop",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: prior.projectId,
                reasonCode: "guard.selectionMismatch"
            )
        }

        let resolvedProject = prior.projectId
        let inFlight = [.running, .runningTool, .backgroundActive].contains(prior.phase)
        if !inFlight {
            let noopEntry = AntigravityCommandReply(
                instanceId: "qa-antigravity-instance",
                generation: generation,
                status: "noop",
                commandOp: "stop",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                delivery: .none,
                inFlightSteering: false,
                reasonCode: "guard.alreadyStopped",
                duplicate: false,
                reconciled: false,
                cursor: cursor,
                rows: sortedRows(),
                selected: validSelected(),
                transcriptReads: UInt64(transcriptReads),
                tokenReads: UInt64(tokenReads)
            )
            commandLedger[intent.requestId] = noopEntry
            pendingChanges = []
            return noopEntry
        }

        if intent.expectedInvocationNum > 0 && intent.expectedInvocationNum != prior.invocationNum {
            let noopEntry = AntigravityCommandReply(
                instanceId: "qa-antigravity-instance",
                generation: generation,
                status: "noop",
                commandOp: "stop",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                delivery: .none,
                inFlightSteering: false,
                reasonCode: "guard.stopTargetChanged",
                duplicate: false,
                reconciled: false,
                cursor: cursor,
                rows: sortedRows(),
                selected: validSelected(),
                transcriptReads: UInt64(transcriptReads),
                tokenReads: UInt64(tokenReads)
            )
            commandLedger[intent.requestId] = noopEntry
            pendingChanges = []
            return noopEntry
        }
        if let expectedExec = intent.expectedExecutionNum, expectedExec != prior.executionNum {
            let noopEntry = AntigravityCommandReply(
                instanceId: "qa-antigravity-instance",
                generation: generation,
                status: "noop",
                commandOp: "stop",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                delivery: .none,
                inFlightSteering: false,
                reasonCode: "guard.stopTargetChanged",
                duplicate: false,
                reconciled: false,
                cursor: cursor,
                rows: sortedRows(),
                selected: validSelected(),
                transcriptReads: UInt64(transcriptReads),
                tokenReads: UInt64(tokenReads)
            )
            commandLedger[intent.requestId] = noopEntry
            pendingChanges = []
            return noopEntry
        }

        let updatedRow = AntigravitySessionRow(
            conversationId: prior.conversationId,
            workspacePath: prior.workspacePath,
            title: prior.title,
            projectId: resolvedProject,
            modelName: prior.modelName,
            thinkingLevel: prior.thinkingLevel,
            phase: .canceled,
            lastEventType: .stop,
            invocationNum: prior.invocationNum,
            stepIdx: prior.stepIdx,
            executionNum: prior.executionNum,
            lastToolName: prior.lastToolName,
            terminationReason: "USER_CANCELED",
            fullyIdle: intent.killBackgroundTasks,
            transitionCount: prior.transitionCount + 1,
            pendingInteraction: nil,
            availableModels: prior.availableModels
        )
        rows[intent.target] = updatedRow
        cursor += 1
        let evKey = "\(intent.target.conversationId):\(intent.target.workspacePath):stop:\(intent.requestId)"
        seenKeys[intent.target, default: []].insert(evKey)
        pendingChanges = []
        stopConfirmed += 1

        let settled = AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: "stopped",
            commandOp: "stop",
            requestId: intent.requestId,
            conversationId: intent.target.conversationId,
            workspacePath: intent.target.workspacePath,
            projectId: resolvedProject,
            delivery: .canceled,
            inFlightSteering: false,
            reasonCode: "languageServer.cancelCascadeInvocationConfirmed",
            duplicate: false,
            reconciled: false,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
        commandLedger[intent.requestId] = settled
        if unknownNextCommand {
            unknownNextCommand = false
            return makeCommandReply(
                from: settled,
                statusOverride: "unknown",
                reasonOverride: "bridge.deliveryUnconfirmed",
                duplicate: false,
                reconciled: false
            )
        }
        return settled
    }

    func resolve(_ intent: AntigravityResolveIntent) async throws -> AntigravityCommandReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.supportsResolveInteraction else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard intent.valid, [Self.workspaceA, Self.workspaceB].contains(intent.target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        resolveRequests += 1
        metadataReads += 1

        if let existing = commandLedger[intent.requestId] {
            guard existing.commandOp == "resolve",
                  existing.conversationId == intent.target.conversationId,
                  existing.workspacePath == intent.target.workspacePath else {
                return makeUnconfirmedReply(
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: "",
                    reasonCode: "guard.requestIdConflict"
                )
            }
            duplicateCommandsDropped += 1
            return makeCommandReply(from: existing, duplicate: true, reconciled: false)
        }

        guard let prior = rows[intent.target] else {
            return makeUnconfirmedReply(
                commandOp: "resolve",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: "",
                reasonCode: "guard.targetNotFound"
            )
        }
        guard selected == intent.target else {
            return makeUnconfirmedReply(
                commandOp: "resolve",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: prior.projectId,
                reasonCode: "guard.selectionMismatch"
            )
        }

        let resolvedProject = prior.projectId
        if expiredInteractions[intent.target]?.contains(intent.interactionId) == true {
            let noopEntry = AntigravityCommandReply(
                instanceId: "qa-antigravity-instance",
                generation: generation,
                status: "noop",
                commandOp: "resolve",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                delivery: .none,
                inFlightSteering: false,
                reasonCode: "guard.interactionAlreadyResolved",
                duplicate: false,
                reconciled: false,
                cursor: cursor,
                rows: sortedRows(),
                selected: validSelected(),
                transcriptReads: UInt64(transcriptReads),
                tokenReads: UInt64(tokenReads)
            )
            commandLedger[intent.requestId] = noopEntry
            pendingChanges = []
            return noopEntry
        }

        guard let pending = prior.pendingInteraction else {
            return makeUnconfirmedReply(
                commandOp: "resolve",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                reasonCode: "guard.noPendingInteraction"
            )
        }
        guard pending.interactionId == intent.interactionId else {
            return makeUnconfirmedReply(
                commandOp: "resolve",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                reasonCode: "guard.interactionIdMismatch"
            )
        }
        guard pending.interactionType == intent.interactionType else {
            return makeUnconfirmedReply(
                commandOp: "resolve",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                reasonCode: "guard.interactionTypeMismatch"
            )
        }
        if intent.interactionType == .question {
            if intent.decision == .allow || intent.decision == .deny {
                return makeUnconfirmedReply(
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: resolvedProject,
                    reasonCode: "guard.questionCannotApproveAsPermission"
                )
            }
            if intent.decision == .answer && intent.selectedOptionIds.isEmpty {
                return makeUnconfirmedReply(
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: resolvedProject,
                    reasonCode: "guard.invalidQuestionOption"
                )
            }
        } else {
            if intent.decision == .answer {
                return makeUnconfirmedReply(
                    commandOp: "resolve",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: resolvedProject,
                    reasonCode: "guard.invalidInteractionDecision"
                )
            }
        }

        expiredInteractions[intent.target, default: []].insert(intent.interactionId)
        let deniedOrCanceled = intent.decision == .deny || intent.decision == .cancel
        let updatedRow = AntigravitySessionRow(
            conversationId: prior.conversationId,
            workspacePath: prior.workspacePath,
            title: prior.title,
            projectId: resolvedProject,
            modelName: prior.modelName,
            thinkingLevel: prior.thinkingLevel,
            phase: .running,
            lastEventType: deniedOrCanceled ? .preInvocation : .postToolUse,
            invocationNum: max(1, prior.invocationNum),
            stepIdx: max(prior.stepIdx, pending.stepIndex),
            executionNum: max(1, prior.executionNum),
            lastToolName: prior.lastToolName,
            terminationReason: nil,
            fullyIdle: false,
            transitionCount: prior.transitionCount + 1,
            pendingInteraction: nil,
            availableModels: prior.availableModels
        )
        rows[intent.target] = updatedRow
        cursor += 1
        let evKey = "\(intent.target.conversationId):\(intent.target.workspacePath):resolve:\(intent.requestId)"
        seenKeys[intent.target, default: []].insert(evKey)
        pendingChanges = []
        resolveConfirmed += 1

        let delivery: AntigravityDeliveryOutcome
        switch intent.decision {
        case .allow: delivery = .allow
        case .deny: delivery = .deny
        case .answer: delivery = .answer
        case .cancel: delivery = .cancel
        }

        let settled = AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: "resolved",
            commandOp: "resolve",
            requestId: intent.requestId,
            conversationId: intent.target.conversationId,
            workspacePath: intent.target.workspacePath,
            projectId: resolvedProject,
            delivery: delivery,
            inFlightSteering: false,
            reasonCode: "languageServer.handleCascadeUserInteractionConfirmed",
            duplicate: false,
            reconciled: false,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
        commandLedger[intent.requestId] = settled
        if unknownNextCommand {
            unknownNextCommand = false
            return makeCommandReply(
                from: settled,
                statusOverride: "unknown",
                reasonOverride: "bridge.deliveryUnconfirmed",
                duplicate: false,
                reconciled: false
            )
        }
        return settled
    }

    func switchModel(_ intent: AntigravitySwitchModelIntent) async throws -> AntigravityCommandReply {
        guard connected else { throw AntigravityFailure.denied }
        guard mode != .missingSchema, activeCapabilities.supportsSwitchModelConfig else {
            throw AntigravityFailure.denied
        }
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard intent.valid, [Self.workspaceA, Self.workspaceB].contains(intent.target.workspacePath) else {
            throw AntigravityFailure.denied
        }
        switchModelRequests += 1
        metadataReads += 1

        if let existing = commandLedger[intent.requestId] {
            guard existing.commandOp == "switch-model",
                  existing.conversationId == intent.target.conversationId,
                  existing.workspacePath == intent.target.workspacePath else {
                return makeUnconfirmedReply(
                    commandOp: "switch-model",
                    requestId: intent.requestId,
                    conversationId: intent.target.conversationId,
                    workspacePath: intent.target.workspacePath,
                    projectId: "",
                    reasonCode: "guard.requestIdConflict"
                )
            }
            duplicateCommandsDropped += 1
            return makeCommandReply(from: existing, duplicate: true, reconciled: false)
        }

        guard let prior = rows[intent.target] else {
            return makeUnconfirmedReply(
                commandOp: "switch-model",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: "",
                reasonCode: "guard.targetNotFound"
            )
        }
        guard selected == intent.target else {
            return makeUnconfirmedReply(
                commandOp: "switch-model",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: prior.projectId,
                reasonCode: "guard.selectionMismatch"
            )
        }

        let resolvedProject = prior.projectId
        guard let matchedModel = prior.availableModels.first(where: { $0.modelName == intent.modelName }) else {
            return makeUnconfirmedReply(
                commandOp: "switch-model",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                reasonCode: "guard.unsupportedModel"
            )
        }
        if !matchedModel.supportedThinkingLevels.contains(intent.thinkingLevel) {
            return makeUnconfirmedReply(
                commandOp: "switch-model",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                reasonCode: "guard.unsupportedThinkingLevel"
            )
        }

        let actualModel = modelReadbackOverride?.modelName ?? intent.modelName
        let actualThinking = modelReadbackOverride?.thinkingLevel ?? intent.thinkingLevel
        let updatedRow = AntigravitySessionRow(
            conversationId: prior.conversationId,
            workspacePath: prior.workspacePath,
            title: prior.title,
            projectId: resolvedProject,
            modelName: actualModel,
            thinkingLevel: actualThinking,
            phase: prior.phase,
            lastEventType: prior.lastEventType,
            invocationNum: prior.invocationNum,
            stepIdx: prior.stepIdx,
            executionNum: prior.executionNum,
            lastToolName: prior.lastToolName,
            terminationReason: prior.terminationReason,
            fullyIdle: prior.fullyIdle,
            transitionCount: prior.transitionCount + 1,
            pendingInteraction: prior.pendingInteraction,
            availableModels: prior.availableModels
        )
        rows[intent.target] = updatedRow
        cursor += 1
        let evKey = "\(intent.target.conversationId):\(intent.target.workspacePath):switch-model:\(intent.requestId)"
        seenKeys[intent.target, default: []].insert(evKey)
        pendingChanges = []

        if actualModel != intent.modelName || actualThinking != intent.thinkingLevel {
            return makeUnconfirmedReply(
                commandOp: "switch-model",
                requestId: intent.requestId,
                conversationId: intent.target.conversationId,
                workspacePath: intent.target.workspacePath,
                projectId: resolvedProject,
                reasonCode: "guard.modelReadbackMismatch"
            )
        }

        switchModelConfirmed += 1
        let settled = AntigravityCommandReply(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            status: "modelSwitched",
            commandOp: "switch-model",
            requestId: intent.requestId,
            conversationId: intent.target.conversationId,
            workspacePath: intent.target.workspacePath,
            projectId: resolvedProject,
            delivery: .modelSwitched,
            inFlightSteering: false,
            reasonCode: "languageServer.cascadeModelConfigConfirmed",
            duplicate: false,
            reconciled: false,
            cursor: cursor,
            rows: sortedRows(),
            selected: validSelected(),
            transcriptReads: UInt64(transcriptReads),
            tokenReads: UInt64(tokenReads)
        )
        commandLedger[intent.requestId] = settled
        if unknownNextCommand {
            unknownNextCommand = false
            return makeCommandReply(
                from: settled,
                statusOverride: "unknown",
                reasonOverride: "bridge.deliveryUnconfirmed",
                duplicate: false,
                reconciled: false
            )
        }
        return settled
    }

    func close() async {
        if connected { closes += 1 }
        connected = false
    }
}

#if MI_SYNTHETIC_BACKEND
extension Store {
    func prepareAntigravityStatusScenario() async {
        let fixture = AntigravityStatusFixture.shared
        await fixture.reset()
        var checks: [String: Bool] = [:]

        func waitReady() async throws {
            for _ in 0..<200 {
                if antigravityStatus.state == .ready { return }
                try await Task.sleep(for: .milliseconds(5))
            }
            throw AntigravityFailure.unavailable
        }

        do {
            await installCodingAgentPresets()
            guard let profile = hostProfiles.first(where: { $0.name == "Antigravity" }) else {
                throw AntigravityFailure.unavailable
            }
            await setHostActiveProfile(profile.id.text)

            let before = await fixture.counts()
            await antigravityStatus.refresh()
            let disabled = await fixture.counts()
            checks["disabled_zero_reads"] = before.reads == disabled.reads

            try await fixture.ingest(AntigravityHookEvent(
                conversationId: AntigravityStatusFixture.sessionA.conversationId,
                workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
                eventType: .sessionStart,
                modelName: "gemini-2.5-pro",
                invocationNum: 0,
                stepIdx: 0,
                executionNum: 0,
                toolName: nil,
                terminationReason: nil,
                hasError: false,
                fullyIdle: true,
                eventId: nil,
                title: "Shared Investigation",
                projectId: "proj-alpha"
            ))
            try await fixture.ingest(AntigravityHookEvent(
                conversationId: AntigravityStatusFixture.sessionB.conversationId,
                workspacePath: AntigravityStatusFixture.sessionB.workspacePath,
                eventType: .sessionStart,
                modelName: "gemini-2.5-flash",
                invocationNum: 0,
                stepIdx: 0,
                executionNum: 0,
                toolName: nil,
                terminationReason: nil,
                hasError: false,
                fullyIdle: true,
                eventId: nil,
                title: "Shared Investigation",
                projectId: "proj-beta"
            ))

            antigravityStatus.setEnabled(true)
            try await waitReady()

            checks["version_and_capabilities_recorded"] = antigravityStatus.capabilities == .installed2_29_0
                && antigravityStatus.capabilities?.installedVersion == "2.29.0"
                && antigravityStatus.capabilities?.languageServerCl == "1013060670"
                && antigravityStatus.capabilities?.supportsSessionCatalog == true
                && antigravityStatus.capabilities?.supportsTargetFocus == true
                && antigravityStatus.capabilities?.schemaVerified == true
                && antigravityStatus.capabilities?.supportsStopTurn == true
                && antigravityStatus.capabilities?.supportsApprovalGate == true
                && antigravityStatus.capabilities?.supportsModelConfig == true
                && antigravityStatus.capabilities?.stopSchemaVerified == true
                && antigravityStatus.capabilities?.approvalSchemaVerified == true
                && antigravityStatus.capabilities?.modelConfigSchemaVerified == true
                && antigravityStatus.capabilities?.csrfAuthVerified == true
                && antigravityStatus.capabilities?.arbitraryRpcExposed == false

            let selA1 = antigravityStatus.selection(for: AntigravityStatusFixture.sessionA)
            if let selA1 {
                _ = openAntigravitySession(selA1)
                await hostExecutionTask?.value
            }
            let confirmedA = antigravityStatus.lastResult == .confirmed
                && antigravityStatus.selected == AntigravityStatusFixture.sessionA
                && lastHostActionReceipt?.actionType == "antigravity_open"
                && lastHostActionReceipt?.outcome == "confirmed"

            let selB1 = antigravityStatus.selection(for: AntigravityStatusFixture.sessionB)
            if let selB1 {
                _ = openAntigravitySession(selB1)
                await hostExecutionTask?.value
            }
            let confirmedB = antigravityStatus.lastResult == .confirmed
                && antigravityStatus.selected == AntigravityStatusFixture.sessionB
                && lastHostActionReceipt?.actionType == "antigravity_open"
                && lastHostActionReceipt?.outcome == "confirmed"

            checks["existing_two_sessions_catalog_and_selected_readback_confirmed"] = confirmedA && confirmedB
            checks["same_name_cross_workspace_no_collision"] = antigravityStatus.rows.count == 2
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.title == "Shared Investigation"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB)?.title == "Shared Investigation"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.projectId == "proj-alpha"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB)?.projectId == "proj-beta"
                && antigravityStatus.selected == AntigravityStatusFixture.sessionB

            await fixture.setMode(.wrongSelected)
            if let selWrong = antigravityStatus.selection(for: AntigravityStatusFixture.sessionA) {
                _ = openAntigravitySession(selWrong)
                await hostExecutionTask?.value
            }
            checks["wrong_selected_readback_unconfirmed"] = antigravityStatus.lastResult == .unconfirmed
                && lastHostActionReceipt?.outcome == "unconfirmed"
            await fixture.setMode(.normal)

            let toolEventA = AntigravityHookEvent(
                conversationId: AntigravityStatusFixture.sessionA.conversationId,
                workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
                eventType: .preToolUse,
                modelName: "gemini-2.5-pro",
                invocationNum: 1,
                stepIdx: 2,
                executionNum: 1,
                toolName: "run_command",
                terminationReason: nil,
                hasError: false,
                fullyIdle: false,
                eventId: nil
            )
            let acceptedFirst = try await fixture.ingest(toolEventA)
            let acceptedDuplicate = try await fixture.ingest(toolEventA)
            await antigravityStatus.refresh()

            let rowA1 = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            let rowB1 = antigravityStatus.row(for: AntigravityStatusFixture.sessionB)
            checks["two_sessions_isolated"] = antigravityStatus.rows.count == 2
                && rowA1?.phase == .runningTool
                && rowA1?.lastToolName == "run_command"
                && rowA1?.transitionCount == 2
                && rowA1?.title == "Shared Investigation"
                && rowB1?.phase == .idle
                && rowB1?.lastToolName == nil
                && rowB1?.transitionCount == 1
                && rowB1?.title == "Shared Investigation"

            let dupCounts = await fixture.counts()
            checks["duplicate_events_deduplicated"] = acceptedFirst && !acceptedDuplicate
                && dupCounts.duplicates == 1
                && antigravityStatus.lastChanges.count == 1
                && rowA1?.transitionCount == 2

            let stopNotIdle = AntigravityHookEvent(
                conversationId: AntigravityStatusFixture.sessionA.conversationId,
                workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
                eventType: .stop,
                modelName: "gemini-2.5-pro",
                invocationNum: nil,
                stepIdx: nil,
                executionNum: nil,
                toolName: nil,
                terminationReason: "NO_TOOL_CALL",
                hasError: false,
                fullyIdle: false,
                eventId: nil
            )
            let stopNotIdleFirst = try await fixture.ingest(stopNotIdle)
            let stopNotIdleDuplicate = try await fixture.ingest(stopNotIdle)
            await antigravityStatus.refresh()
            let rowAfterNotIdle = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            checks["stop_fully_idle_false_not_completed"] = stopNotIdleFirst && !stopNotIdleDuplicate
                && rowAfterNotIdle?.phase == .backgroundActive
                && rowAfterNotIdle?.phase.isCompleted == false
                && rowAfterNotIdle?.fullyIdle == false
                && rowAfterNotIdle?.transitionCount == 3

            let stopFullyIdle = AntigravityHookEvent(
                conversationId: AntigravityStatusFixture.sessionA.conversationId,
                workspacePath: AntigravityStatusFixture.sessionA.workspacePath,
                eventType: .stop,
                modelName: "gemini-2.5-pro",
                invocationNum: nil,
                stepIdx: nil,
                executionNum: nil,
                toolName: nil,
                terminationReason: "NO_TOOL_CALL",
                hasError: false,
                fullyIdle: true,
                eventId: nil
            )
            let stopIdleAccepted = try await fixture.ingest(stopFullyIdle)
            await antigravityStatus.refresh()
            let rowAfterIdle = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            checks["stop_fully_idle_true_completed"] = stopIdleAccepted
                && rowAfterIdle?.phase == .completed
                && rowAfterIdle?.phase.isCompleted == true
                && rowAfterIdle?.fullyIdle == true
                && rowAfterIdle?.transitionCount == 4

            if let selConfirmA = antigravityStatus.selection(for: AntigravityStatusFixture.sessionA) {
                _ = openAntigravitySession(selConfirmA)
                await hostExecutionTask?.value
            }

            let createdKey = AntigravitySessionKey(
                conversationId: "conv-alpha-created",
                workspacePath: AntigravityStatusFixture.workspaceA
            )
            let mismatchCreateCmd = antigravityStatus.createIntent(
                workspacePath: AntigravityStatusFixture.workspaceA,
                projectId: "proj-wrong",
                prompt: "Should be blocked by projectId binding",
                title: "Wrong Project",
                conversationId: "conv-alpha-wrong",
                requestId: "req-create-wrong-proj"
            )
            _ = createAntigravityConversation(mismatchCreateCmd)
            await hostExecutionTask?.value
            let projectMismatchBlocked = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.projectIdMismatch"
                && antigravityStatus.row(for: AntigravitySessionKey(conversationId: "conv-alpha-wrong", workspacePath: AntigravityStatusFixture.workspaceA)) == nil

            let createCmd = antigravityStatus.createIntent(
                workspacePath: AntigravityStatusFixture.workspaceA,
                projectId: "proj-alpha",
                prompt: "Create new conversation bound to proj-alpha",
                title: "Alpha Created Conversation",
                modelName: "gemini-2.5-flash",
                conversationId: createdKey.conversationId,
                requestId: "req-create-alpha-1"
            )
            _ = createAntigravityConversation(createCmd)
            await hostExecutionTask?.value
            let createdRow = antigravityStatus.row(for: createdKey)
            checks["create_binds_project_id_and_confirms_selected"] = projectMismatchBlocked
                && antigravityStatus.lastCommandResult == .created
                && antigravityStatus.selected == createdKey
                && createdRow?.projectId == "proj-alpha"
                && createdRow?.title == "Alpha Created Conversation"
                && createdRow?.phase == .running
                && lastHostActionReceipt?.actionType == "antigravity_create"
                && lastHostActionReceipt?.outcome == "created"
                && lastHostActionReceipt?.textByteLen == nil

            if let selBackToA = antigravityStatus.selection(for: AntigravityStatusFixture.sessionA) {
                _ = openAntigravitySession(selBackToA)
                await hostExecutionTask?.value
            }
            let bBeforeSend = antigravityStatus.row(for: AntigravityStatusFixture.sessionB)
            let createdBeforeSend = antigravityStatus.row(for: createdKey)
            let unselectedSendCmd = antigravityStatus.sendIntent(
                for: AntigravityStatusFixture.sessionB,
                projectId: "proj-beta",
                message: "Must not touch unselected session B",
                deliveryMode: .auto,
                requestId: "req-send-unselected-b"
            )
            _ = sendAntigravityMessage(unselectedSendCmd)
            await hostExecutionTask?.value
            let unselectedSendBlocked = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.selectionMismatch"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB) == bBeforeSend
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.transitionCount == 4

            let sendIdleACmd = antigravityStatus.sendIntent(
                for: AntigravityStatusFixture.sessionA,
                projectId: "proj-alpha",
                message: "Resume session A from completed state",
                deliveryMode: .auto,
                requestId: "req-send-alpha-idle-1"
            )
            _ = sendAntigravityMessage(sendIdleACmd)
            await hostExecutionTask?.value
            let aAfterSend = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            let bAfterSend = antigravityStatus.row(for: AntigravityStatusFixture.sessionB)
            let createdAfterSend = antigravityStatus.row(for: createdKey)
            checks["two_sessions_send_only_modifies_target"] = unselectedSendBlocked
                && antigravityStatus.lastCommandResult == .started
                && antigravityStatus.lastCommandReply?.delivery == .started
                && antigravityStatus.lastCommandReply?.inFlightSteering == false
                && aAfterSend?.phase == .running
                && aAfterSend?.invocationNum == 2
                && aAfterSend?.transitionCount == 5
                && bAfterSend == bBeforeSend
                && createdAfterSend == createdBeforeSend
                && lastHostActionReceipt?.actionType == "antigravity_send"
                && lastHostActionReceipt?.outcome == "started"
                && lastHostActionReceipt?.textByteLen == nil

            let rowCountBeforeRunningSend = antigravityStatus.rows.count
            let runningDefaultSteerCmd = antigravityStatus.sendIntent(
                for: AntigravityStatusFixture.sessionA,
                projectId: "proj-alpha",
                message: "In-flight message under official agentapi contract",
                deliveryMode: .steer,
                requestId: "req-send-alpha-running-default"
            )
            _ = sendAntigravityMessage(runningDefaultSteerCmd)
            await hostExecutionTask?.value
            let defaultQueuedConfirmed = antigravityStatus.lastCommandResult == .queued
                && antigravityStatus.lastCommandReply?.delivery == .nextInvocation
                && antigravityStatus.lastCommandReply?.inFlightSteering == false
                && antigravityStatus.lastCommandReply?.reasonCode == "agentapi.steerUnsupportedQueuedNextInvocation"
                && antigravityStatus.rows.count == rowCountBeforeRunningSend
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.transitionCount == 6
                && lastHostActionReceipt?.actionType == "antigravity_send"
                && lastHostActionReceipt?.outcome == "queued"

            await fixture.setInFlightSteerEnabled(true)
            let runningSteerEnabledCmd = antigravityStatus.sendIntent(
                for: AntigravityStatusFixture.sessionA,
                projectId: "proj-alpha",
                message: "In-flight message with steering enabled",
                deliveryMode: .steer,
                requestId: "req-send-alpha-running-steered"
            )
            _ = sendAntigravityMessage(runningSteerEnabledCmd)
            await hostExecutionTask?.value
            await fixture.setInFlightSteerEnabled(false)
            let explicitSteerConfirmed = antigravityStatus.lastCommandResult == .steered
                && antigravityStatus.lastCommandReply?.delivery == .steered
                && antigravityStatus.lastCommandReply?.inFlightSteering == true
                && antigravityStatus.lastCommandReply?.reasonCode == "agentapi.sendMessageSteered"
                && antigravityStatus.rows.count == rowCountBeforeRunningSend
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.stepIdx == 3
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.transitionCount == 7
                && lastHostActionReceipt?.actionType == "antigravity_send"
                && lastHostActionReceipt?.outcome == "steered"
            checks["running_session_send_steering_verified_separately"] = defaultQueuedConfirmed && explicitSteerConfirmed

            await fixture.setUnknownNextCommand(true)
            let timeoutSendCmd = antigravityStatus.sendIntent(
                for: AntigravityStatusFixture.sessionA,
                projectId: "proj-alpha",
                message: "Message whose response is initially unknown",
                deliveryMode: .queue,
                requestId: "req-send-timeout-1"
            )
            _ = sendAntigravityMessage(timeoutSendCmd)
            await hostExecutionTask?.value
            let cmdCountsAfterTimeout = await fixture.commandCounts()
            let unknownLatched = antigravityStatus.lastCommandResult == .unknown
                && antigravityStatus.pendingUnknownCommand?.operation.requestId == "req-send-timeout-1"
                && lastHostActionReceipt?.actionType == "antigravity_send"
                && lastHostActionReceipt?.outcome == "unknown"
            let blockedNewSendWhileUnknown = antigravityStatus.sendIntent(
                for: AntigravityStatusFixture.sessionA,
                projectId: "proj-alpha",
                message: "Must not issue new request while unknown is latched",
                deliveryMode: .queue,
                requestId: "req-send-blocked-2"
            )
            let queryReconcileCmd = antigravityStatus.queryIntent(requestId: "req-send-timeout-1")
            _ = queryAntigravityCommand(queryReconcileCmd)
            await hostExecutionTask?.value
            let cmdCountsAfterReconcile = await fixture.commandCounts()
            checks["timeout_unknown_reconciles_without_resend"] = unknownLatched
                && blockedNewSendWhileUnknown == nil
                && antigravityStatus.lastCommandResult == .queued
                && antigravityStatus.lastCommandReply?.reconciled == true
                && antigravityStatus.lastCommandReply?.duplicate == true
                && antigravityStatus.pendingUnknownCommand == nil
                && cmdCountsAfterReconcile.sends == cmdCountsAfterTimeout.sends
                && cmdCountsAfterReconcile.queries == 1
                && lastHostActionReceipt?.actionType == "antigravity_query"
                && lastHostActionReceipt?.outcome == "queued"

            let bBeforeStop = antigravityStatus.row(for: AntigravityStatusFixture.sessionB)
            let unselectedStopCmd = antigravityStatus.stopIntent(
                for: AntigravityStatusFixture.sessionB,
                requestId: "req-stop-unselected-b"
            )
            _ = stopAntigravityConversation(unselectedStopCmd)
            await hostExecutionTask?.value
            let unselectedStopBlocked = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.selectionMismatch"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB) == bBeforeStop

            let staleStopCmd = antigravityStatus.stopIntent(
                for: AntigravityStatusFixture.sessionA,
                expectedInvocationNum: 1,
                expectedExecutionNum: 1,
                requestId: "req-stop-stale-inv"
            )
            _ = stopAntigravityConversation(staleStopCmd)
            await hostExecutionTask?.value
            let staleStopNoop = antigravityStatus.lastCommandResult == .noop
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.stopTargetChanged"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.phase == .running

            let validStopCmd = antigravityStatus.stopIntent(
                for: AntigravityStatusFixture.sessionA,
                expectedInvocationNum: 2,
                expectedExecutionNum: 2,
                requestId: "req-stop-alpha-1"
            )
            _ = stopAntigravityConversation(validStopCmd)
            await hostExecutionTask?.value
            let aAfterStop = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            let bAfterStop = antigravityStatus.row(for: AntigravityStatusFixture.sessionB)
            checks["stop_selected_session_only_and_stale_noop"] = unselectedStopBlocked
                && staleStopNoop
                && antigravityStatus.lastCommandResult == .stopped
                && antigravityStatus.lastCommandReply?.delivery == .canceled
                && antigravityStatus.lastCommandReply?.reasonCode == "languageServer.cancelCascadeInvocationConfirmed"
                && aAfterStop?.phase == .canceled
                && aAfterStop?.fullyIdle == true
                && aAfterStop?.terminationReason == "USER_CANCELED"
                && bAfterStop == bBeforeStop
                && lastHostActionReceipt?.actionType == "antigravity_stop"
                && lastHostActionReceipt?.outcome == "stopped"
                && lastHostActionReceipt?.textByteLen == nil

            await fixture.setPendingInteraction(
                for: AntigravityStatusFixture.sessionA,
                pending: AntigravityPendingInteraction(
                    interactionId: "int-question-1",
                    interactionType: .question,
                    stepIndex: 4,
                    optionIds: ["opt-fast", "opt-safe"]
                )
            )
            await fixture.setPendingInteraction(
                for: AntigravityStatusFixture.sessionB,
                pending: AntigravityPendingInteraction(
                    interactionId: "int-perm-b",
                    interactionType: .filePermission,
                    stepIndex: 1
                )
            )
            await antigravityStatus.refresh()

            let questionAsAllowCmd = antigravityStatus.resolveIntent(
                for: AntigravityStatusFixture.sessionA,
                interactionId: "int-question-1",
                interactionType: .question,
                decision: .allow,
                requestId: "req-resolve-question-as-allow"
            )
            _ = resolveAntigravityInteraction(questionAsAllowCmd)
            await hostExecutionTask?.value
            let questionAsAllowRejected = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.questionCannotApproveAsPermission"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.pendingInteraction?.interactionId == "int-question-1"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB)?.pendingInteraction?.interactionId == "int-perm-b"

            let questionAnswerCmd = antigravityStatus.resolveIntent(
                for: AntigravityStatusFixture.sessionA,
                interactionId: "int-question-1",
                interactionType: .question,
                decision: .answer,
                selectedOptionIds: ["opt-fast"],
                requestId: "req-resolve-question-ok"
            )
            _ = resolveAntigravityInteraction(questionAnswerCmd)
            await hostExecutionTask?.value
            let questionAnswered = antigravityStatus.lastCommandResult == .resolved
                && antigravityStatus.lastCommandReply?.delivery == .answer
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.pendingInteraction == nil
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB)?.pendingInteraction?.interactionId == "int-perm-b"
                && lastHostActionReceipt?.actionType == "antigravity_resolve"
                && lastHostActionReceipt?.outcome == "resolved"

            await fixture.setPendingInteraction(
                for: AntigravityStatusFixture.sessionA,
                pending: AntigravityPendingInteraction(
                    interactionId: "int-perm-a",
                    interactionType: .runCommand,
                    stepIndex: 5
                )
            )
            await antigravityStatus.refresh()

            let permAllowCmd = antigravityStatus.resolveIntent(
                for: AntigravityStatusFixture.sessionA,
                interactionId: "int-perm-a",
                interactionType: .runCommand,
                decision: .allow,
                requestId: "req-resolve-perm-a"
            )
            _ = resolveAntigravityInteraction(permAllowCmd)
            await hostExecutionTask?.value
            let permAllowed = antigravityStatus.lastCommandResult == .resolved
                && antigravityStatus.lastCommandReply?.delivery == .allow
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.pendingInteraction == nil
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB)?.pendingInteraction?.interactionId == "int-perm-b"

            _ = resolveAntigravityInteraction(permAllowCmd)
            await hostExecutionTask?.value
            let permDuplicateIdempotent = antigravityStatus.lastCommandResult == .resolved
                && antigravityStatus.lastCommandReply?.duplicate == true

            await fixture.setPendingInteraction(
                for: AntigravityStatusFixture.sessionA,
                pending: AntigravityPendingInteraction(
                    interactionId: "int-perm-expired",
                    interactionType: .permission,
                    stepIndex: 6
                )
            )
            await antigravityStatus.refresh()
            let expiredResolveCmd = antigravityStatus.resolveIntent(
                for: AntigravityStatusFixture.sessionA,
                interactionId: "int-perm-expired",
                interactionType: .permission,
                decision: .allow,
                requestId: "req-resolve-expired"
            )
            await fixture.expirePendingInteraction(
                for: AntigravityStatusFixture.sessionA,
                interactionId: "int-perm-expired"
            )
            _ = resolveAntigravityInteraction(expiredResolveCmd)
            await hostExecutionTask?.value
            let expiredResolveNoop = antigravityStatus.lastCommandResult == .noop
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.interactionAlreadyResolved"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionB)?.pendingInteraction?.interactionId == "int-perm-b"

            await fixture.setPendingInteraction(for: AntigravityStatusFixture.sessionB, pending: nil)
            await antigravityStatus.refresh()
            checks["resolve_interaction_and_question_guard"] = questionAsAllowRejected
                && questionAnswered
                && permAllowed
                && permDuplicateIdempotent
                && expiredResolveNoop

            let unsupportedThinkingCmd = antigravityStatus.switchModelIntent(
                for: AntigravityStatusFixture.sessionA,
                modelName: "gemini-2.5-flash-lite",
                thinkingLevel: "high",
                requestId: "req-model-unsupported-thinking"
            )
            _ = switchAntigravityModel(unsupportedThinkingCmd)
            await hostExecutionTask?.value
            let cmdCountsAfterUnsupportedModel = await fixture.commandCounts()
            let unsupportedThinkingRejected = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.unsupportedThinkingLevel"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.modelName == "gemini-2.5-pro"
                && cmdCountsAfterUnsupportedModel.modelSwitches == 0

            let validSwitchCmd = antigravityStatus.switchModelIntent(
                for: AntigravityStatusFixture.sessionA,
                modelName: "gemini-2.5-flash",
                thinkingLevel: "medium",
                requestId: "req-model-switch-a"
            )
            _ = switchAntigravityModel(validSwitchCmd)
            await hostExecutionTask?.value
            let aAfterSwitch = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            let bAfterSwitch = antigravityStatus.row(for: AntigravityStatusFixture.sessionB)
            let validSwitchConfirmed = antigravityStatus.lastCommandResult == .modelSwitched
                && antigravityStatus.lastCommandReply?.delivery == .modelSwitched
                && antigravityStatus.lastCommandReply?.reasonCode == "languageServer.cascadeModelConfigConfirmed"
                && aAfterSwitch?.modelName == "gemini-2.5-flash"
                && aAfterSwitch?.thinkingLevel == "medium"
                && bAfterSwitch?.modelName == "gemini-2.5-flash"
                && bAfterSwitch?.thinkingLevel == "high"
                && lastHostActionReceipt?.actionType == "antigravity_switch_model"
                && lastHostActionReceipt?.outcome == "modelSwitched"
                && lastHostActionReceipt?.textByteLen == nil

            await fixture.setModelReadbackOverride(modelName: "gemini-2.5-pro", thinkingLevel: "low")
            let mismatchSwitchCmd = antigravityStatus.switchModelIntent(
                for: AntigravityStatusFixture.sessionA,
                modelName: "gemini-2.5-pro",
                thinkingLevel: "high",
                requestId: "req-model-mismatch-a"
            )
            _ = switchAntigravityModel(mismatchSwitchCmd)
            await hostExecutionTask?.value
            let mismatchSwitchRejected = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.modelReadbackMismatch"
            await fixture.setModelReadbackOverride(modelName: nil, thinkingLevel: nil)

            let restoreSwitchCmd = antigravityStatus.switchModelIntent(
                for: AntigravityStatusFixture.sessionA,
                modelName: "gemini-2.5-pro",
                thinkingLevel: "high",
                requestId: "req-model-restore-a"
            )
            _ = switchAntigravityModel(restoreSwitchCmd)
            await hostExecutionTask?.value
            checks["switch_model_and_thinking_guard"] = unsupportedThinkingRejected
                && validSwitchConfirmed
                && mismatchSwitchRejected
                && antigravityStatus.lastCommandResult == .modelSwitched
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.modelName == "gemini-2.5-pro"
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA)?.thinkingLevel == "high"

            let missingTargetKey = AntigravitySessionKey(
                conversationId: "conv-does-not-exist",
                workspacePath: AntigravityStatusFixture.workspaceA
            )
            let missingSendCmd = antigravityStatus.sendIntent(
                for: missingTargetKey,
                projectId: "proj-alpha",
                message: "Should not redirect to session A or create a session",
                deliveryMode: .auto,
                requestId: "req-send-missing-target"
            )
            let aBeforeMissingSend = antigravityStatus.row(for: AntigravityStatusFixture.sessionA)
            let rowCountBeforeMissingSend = antigravityStatus.rows.count
            _ = sendAntigravityMessage(missingSendCmd)
            await hostExecutionTask?.value
            let missingSendZeroRedirect = antigravityStatus.lastCommandResult == .unconfirmed
                && antigravityStatus.lastCommandReply?.reasonCode == "guard.targetNotFound"
                && antigravityStatus.selected == AntigravityStatusFixture.sessionA
                && antigravityStatus.row(for: AntigravityStatusFixture.sessionA) == aBeforeMissingSend
                && antigravityStatus.rows.count == rowCountBeforeMissingSend

            let staleDeletedSelection = antigravityStatus.selection(for: AntigravityStatusFixture.sessionB)
            await fixture.deleteSession(AntigravityStatusFixture.sessionB)
            if let staleDeletedSelection {
                _ = openAntigravitySession(staleDeletedSelection)
                await hostExecutionTask?.value
            }
            checks["deleted_target_no_redirect"] = antigravityStatus.lastResult == .unconfirmed
                && antigravityStatus.selected == AntigravityStatusFixture.sessionA
                && lastHostActionReceipt?.outcome == "unconfirmed"
            checks["missing_or_deleted_target_send_zero_redirect"] = missingSendZeroRedirect
                && checks["deleted_target_no_redirect"] == true

            if let selBeforeMissingSchema = antigravityStatus.selection(for: AntigravityStatusFixture.sessionA) {
                await fixture.setMode(.missingSchema)
                _ = openAntigravitySession(selBeforeMissingSchema)
                await hostExecutionTask?.value
            }
            checks["missing_schema_or_auth_fails_closed"] = antigravityStatus.lastResult == .denied
                && antigravityStatus.state == .unavailable(.denied)
                && lastHostActionReceipt?.outcome == "denied"

            antigravityStatus.setEnabled(false)
            await fixture.setMode(.normal)
            await fixture.setCsrfAuth(true)
            antigravityStatus.setEnabled(true)
            try await waitReady()
            if let selForCsrfCheck = antigravityStatus.selection(for: AntigravityStatusFixture.sessionA) {
                _ = openAntigravitySession(selForCsrfCheck)
                await hostExecutionTask?.value
            }
            let stopCmdBeforeCsrfDrop = antigravityStatus.stopIntent(
                for: AntigravityStatusFixture.sessionA,
                requestId: "req-stop-csrf-drop"
            )
            await fixture.setCsrfAuth(false)
            _ = stopAntigravityConversation(stopCmdBeforeCsrfDrop)
            await hostExecutionTask?.value
            checks["l3_schema_or_csrf_invalidation_fails_closed"] = antigravityStatus.lastCommandResult == .denied
                && antigravityStatus.state == .unavailable(.denied)
                && lastHostActionReceipt?.actionType == "antigravity_stop"
                && lastHostActionReceipt?.outcome == "denied"
            await fixture.setCsrfAuth(true)

            antigravityStatus.setEnabled(false)
            await fixture.setMode(.normal)
            antigravityStatus.setEnabled(true)
            try await waitReady()
            await fixture.setMode(.stale)
            await antigravityStatus.refresh()
            checks["stream_disconnect_marks_stale"] = antigravityStatus.state == .unavailable(.stale)
                && antigravityStatus.rows.isEmpty

            antigravityStatus.setEnabled(false)
            await fixture.setMode(.normal)
            antigravityStatus.setEnabled(true)
            try await waitReady()
            await fixture.setMode(.rotated)
            await antigravityStatus.refresh()
            checks["generation_change_marks_stale"] = antigravityStatus.state == .unavailable(.stale)
                && antigravityStatus.rows.isEmpty

            antigravityStatus.setEnabled(false)
            await fixture.setMode(.normal)
            antigravityStatus.setEnabled(true)
            try await waitReady()
            await fixture.setMode(.gap)
            await antigravityStatus.refresh()
            checks["journal_gap_marks_unavailable"] = antigravityStatus.state == .unavailable(.gap)
                && antigravityStatus.rows.isEmpty

            let finalCounts = await fixture.counts()
            checks["zero_transcript_reads"] = finalCounts.transcriptReads == 0
                && antigravityStatus.transcriptReads == 0
            checks["zero_token_reads"] = finalCounts.tokenReads == 0
                && antigravityStatus.tokenReads == 0

            await antigravityStatus.shutdown()
            checks["bridge_subscription_closed"] = (await fixture.counts()).closes > 0
        } catch {
            checks["scenario_completed"] = false
        }

        let finalCounts = await fixture.counts()
        let finalCmdCounts = await fixture.commandCounts()
        let payload: [String: Any] = [
            "status": checks.count == 26 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "installed_version": AntigravityCapabilities.installed2_29_0.installedVersion,
            "language_server_cl": AntigravityCapabilities.installed2_29_0.languageServerCl,
            "transcript_reads": finalCounts.transcriptReads,
            "token_reads": finalCounts.tokenReads,
            "open_requests": finalCounts.openRequests,
            "open_confirmed": finalCounts.openConfirmed,
            "create_requests": finalCmdCounts.creates,
            "create_confirmed": finalCmdCounts.createsConfirmed,
            "send_requests": finalCmdCounts.sends,
            "send_confirmed": finalCmdCounts.sendsConfirmed,
            "query_requests": finalCmdCounts.queries,
            "stop_requests": finalCmdCounts.stops,
            "stop_confirmed": finalCmdCounts.stopsConfirmed,
            "resolve_requests": finalCmdCounts.resolves,
            "resolve_confirmed": finalCmdCounts.resolvesConfirmed,
            "switch_model_requests": finalCmdCounts.modelSwitches,
            "switch_model_confirmed": finalCmdCounts.modelSwitchesConfirmed,
            "duplicate_commands_dropped": finalCmdCounts.duplicates,
            "checks": checks,
            "native_app": "NOT_RUN",
            "physical_key": "NOT_RUN",
        ]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            let dir = URL(fileURLWithPath: audit).deletingLastPathComponent()
            try? data.write(to: dir.appendingPathComponent("antigravity-status.json"))
            try? data.write(to: dir.appendingPathComponent("antigravity-navigation.json"))
            try? data.write(to: dir.appendingPathComponent("antigravity-agentapi.json"))
            try? data.write(to: dir.appendingPathComponent("antigravity-l3-control.json"))
        }
    }
}
#endif
