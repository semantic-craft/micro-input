import Foundation
import MicroInputCore

actor AntigravityStatusFixture: AntigravityStatusServing {
    static let shared = AntigravityStatusFixture()
    static let workspaceA = "/workspace/ag-alpha"
    static let workspaceB = "/workspace/ag-beta"
    static let sessionA = AntigravitySessionKey(conversationId: "conv-alpha", workspacePath: workspaceA)
    static let sessionB = AntigravitySessionKey(conversationId: "conv-beta", workspacePath: workspaceB)

    enum Mode { case normal, stale, rotated, gap }

    private var mode: Mode = .normal
    private var connected = false
    private var cursor: UInt64 = 0
    private var rows: [AntigravitySessionKey: AntigravitySessionRow] = [:]
    private var seenKeys: [AntigravitySessionKey: Set<String>] = [:]
    private var pendingChanges: [AntigravityEventChange] = []
    private(set) var metadataReads = 0
    private(set) var duplicateEventsDropped = 0
    private(set) var transcriptReads = 0
    private(set) var closes = 0

    func reset() {
        mode = .normal
        connected = false
        cursor = 0
        rows = [:]
        seenKeys = [:]
        pendingChanges = []
        metadataReads = 0
        duplicateEventsDropped = 0
        transcriptReads = 0
        closes = 0
    }

    func setMode(_ value: Mode) { mode = value }

    func counts() -> (reads: Int, duplicates: Int, transcriptReads: Int, closes: Int, cursor: UInt64) {
        (metadataReads, duplicateEventsDropped, transcriptReads, closes, cursor)
    }

    private var generation: UInt64 { mode == .rotated ? 2 : 1 }

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
        guard mode != .stale else { throw AntigravityFailure.stale }
        guard mode != .gap else { throw AntigravityFailure.gap }
        metadataReads += 1
        let sortedRows = rows.keys.sorted().compactMap { rows[$0] }
        let snapshot = AntigravityStatusSnapshot(
            instanceId: "qa-antigravity-instance",
            generation: generation,
            allowedWorkspaces: [Self.workspaceA, Self.workspaceB],
            capabilities: .installed2_29_0,
            cursor: cursor,
            rows: sortedRows,
            changes: pendingChanges,
            transcriptReads: UInt64(transcriptReads)
        )
        pendingChanges = []
        return snapshot
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
                eventId: nil
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
                eventId: nil
            ))

            antigravityStatus.setEnabled(true)
            try await waitReady()

            checks["version_and_capabilities_recorded"] = antigravityStatus.capabilities == .installed2_29_0
                && antigravityStatus.capabilities?.installedVersion == "2.29.0"
                && antigravityStatus.capabilities?.languageServerCl == "1013060670"
                && antigravityStatus.capabilities?.supportsSessionCatalog == false
                && antigravityStatus.capabilities?.supportsTargetFocus == false
                && antigravityStatus.capabilities?.supportsStopTurn == false
                && antigravityStatus.capabilities?.supportsApprovalGate == false

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
                && rowB1?.phase == .idle
                && rowB1?.lastToolName == nil
                && rowB1?.transitionCount == 1

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

            await antigravityStatus.shutdown()
            checks["bridge_subscription_closed"] = (await fixture.counts()).closes > 0
        } catch {
            checks["scenario_completed"] = false
        }

        let finalCounts = await fixture.counts()
        let payload: [String: Any] = [
            "status": checks.count == 11 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "installed_version": AntigravityCapabilities.installed2_29_0.installedVersion,
            "language_server_cl": AntigravityCapabilities.installed2_29_0.languageServerCl,
            "transcript_reads": finalCounts.transcriptReads,
            "checks": checks,
            "native_app": "NOT_RUN",
            "physical_key": "NOT_RUN",
        ]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("antigravity-status.json"))
        }
    }
}
#endif
