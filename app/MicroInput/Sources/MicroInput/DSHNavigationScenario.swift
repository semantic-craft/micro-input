#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

actor DSHNavigationFixture: DSHSessionServing {
    static let shared = DSHNavigationFixture()
    static let first = "qa-session-a"
    static let second = "qa-session-b"
    static let cold = "qa-session-cold"
    enum Mode { case normal, wrongSelected, deleted, rotated, disconnected, gap }
    private var mode = Mode.normal
    private var selected: String?
    private var cursor: UInt64 = 0
    private var pending: UInt64 = 0
    private var changes: [DSHSessionChange] = []
    private var connected = false
    private(set) var metadataReads = 0
    private(set) var opens = 0
    private(set) var closes = 0
    private(set) var navigations: [String] = []
    func setMode(_ value: Mode) { mode = value }
    func counts() -> (reads: Int, opens: Int, closes: Int, navigations: [String]) {
        (metadataReads, opens, closes, navigations)
    }
    private var generation: UInt64 { mode == .rotated ? 2 : 1 }
    private func row(_ id: String, pending: UInt64 = 0) -> DSHSessionRow {
        DSHSessionRow(sessionId: id, workspaceId: id == Self.first ? "workspace-a" : "workspace-b",
                      title: "同名会话", active: true, running: true, pendingInput: pending)
    }
    func pendingRoundTrip() {
        for value in [UInt64(1), UInt64(0)] {
            pending = value; cursor += 1
            changes.append(DSHSessionChange(sequence: cursor, row: row(Self.first, pending: value)))
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
        let ids = mode == .deleted ? [Self.second] : [Self.first, Self.second]
        let result = DSHSessionSnapshot(hostId: "qa-host", generation: generation,
            sessionIds: [Self.first, Self.second, Self.cold], cursor: cursor,
            rows: ids.map { row($0, pending: $0 == Self.first ? pending : 0) },
            unavailableSessionIds: [Self.cold] + (mode == .deleted ? [Self.first] : []),
            changes: changes, selected: ids.contains(selected ?? "") ? selected : nil)
        changes = []
        return result
    }
    func open(_ selection: DSHSelection) async throws -> DSHOpenReply {
        opens += 1
        guard connected, mode != .disconnected else { throw DSHFailure.unavailable }
        guard selection.hostId == "qa-host", selection.generation == generation else { return reply(.stale) }
        guard [Self.first, Self.second].contains(selection.sessionId),
              selection.workspaceId == row(selection.sessionId).workspaceId else { return reply(.denied) }
        guard mode != .deleted || selection.sessionId != Self.first else { return reply(.stale) }
        if mode == .wrongSelected { return reply(.confirmed, selected: Self.second) }
        selected = selection.sessionId; navigations.append(selection.sessionId)
        return reply(.confirmed, selected: selected)
    }
    private func reply(_ status: DSHOpenResult, selected: String? = nil) -> DSHOpenReply {
        DSHOpenReply(hostId: "qa-host", generation: generation, status: status, selected: selected)
    }
    func close() async { if connected { closes += 1 }; connected = false }
}

extension Store {
    func prepareDSHNavigationScenario() async {
        let fixture = DSHNavigationFixture.shared
        var checks: [String: Bool] = [:]
        func waitReady() async throws {
            for _ in 0..<200 {
                if dshSessions.state == .ready { return }
                try await Task.sleep(for: .milliseconds(5))
            }
            throw DSHFailure.unavailable
        }
        func queuedOpen(_ selection: DSHSelection) async {
            openDSHSession(selection)
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
            await fixture.setMode(.wrongSelected); await queuedOpen(first)
            checks["wrong_selected_unconfirmed"] = lastHostActionReceipt?.outcome != "confirmed"
            let outside = DSHSelection(sessionId: "outside", workspaceId: first.workspaceId,
                hostId: first.hostId, generation: first.generation, localEpoch: first.localEpoch)
            let deniedBefore = await fixture.counts(); await queuedOpen(outside)
            let deniedAfter = await fixture.counts()
            checks["out_of_scope_zero_reads"] = deniedBefore.reads == deniedAfter.reads && deniedBefore.opens == deniedAfter.opens
            await fixture.setMode(.deleted); await queuedOpen(first)
            let deletedCounts = await fixture.counts()
            checks["deleted_target_no_redirect"] = lastHostActionReceipt?.outcome != "confirmed"
                && deletedCounts.navigations == successful.navigations
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
        let payload: [String: Any] = ["status": checks.count == 11 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "checks": checks, "native_app": "NOT_RUN", "physical_key": "NOT_RUN"]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("dsh-navigation.json"))
        }
    }
}
#endif
