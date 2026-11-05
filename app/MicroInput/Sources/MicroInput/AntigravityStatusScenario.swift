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
        closes = 0
    }

    func setMode(_ value: Mode) { mode = value }

    func deleteSession(_ key: AntigravitySessionKey) {
        rows.removeValue(forKey: key)
        if selected == key { selected = nil }
    }

    func counts() -> (reads: Int, duplicates: Int, transcriptReads: Int, tokenReads: Int, openRequests: Int, openConfirmed: Int, closes: Int, cursor: UInt64) {
        (metadataReads, duplicateEventsDropped, transcriptReads, tokenReads, openRequests, openConfirmed, closes, cursor)
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
                schemaVerified: false
            )
        }
        return .installed2_29_0
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
                && antigravityStatus.capabilities?.supportsStopTurn == false
                && antigravityStatus.capabilities?.supportsApprovalGate == false

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
            let staleDeletedSelection = antigravityStatus.selection(for: AntigravityStatusFixture.sessionB)
            await fixture.deleteSession(AntigravityStatusFixture.sessionB)
            if let staleDeletedSelection {
                _ = openAntigravitySession(staleDeletedSelection)
                await hostExecutionTask?.value
            }
            checks["deleted_target_no_redirect"] = antigravityStatus.lastResult == .unconfirmed
                && antigravityStatus.selected == AntigravityStatusFixture.sessionA
                && lastHostActionReceipt?.outcome == "unconfirmed"

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
        let payload: [String: Any] = [
            "status": checks.count == 17 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "installed_version": AntigravityCapabilities.installed2_29_0.installedVersion,
            "language_server_cl": AntigravityCapabilities.installed2_29_0.languageServerCl,
            "transcript_reads": finalCounts.transcriptReads,
            "token_reads": finalCounts.tokenReads,
            "open_requests": finalCounts.openRequests,
            "open_confirmed": finalCounts.openConfirmed,
            "checks": checks,
            "native_app": "NOT_RUN",
            "physical_key": "NOT_RUN",
        ]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            let dir = URL(fileURLWithPath: audit).deletingLastPathComponent()
            try? data.write(to: dir.appendingPathComponent("antigravity-status.json"))
            try? data.write(to: dir.appendingPathComponent("antigravity-navigation.json"))
        }
    }
}
#endif
