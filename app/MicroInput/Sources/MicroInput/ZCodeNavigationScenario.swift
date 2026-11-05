#if MI_SYNTHETIC_BACKEND
import Foundation
import MicroInputCore

actor ZCodeNavigationFixture: ZCodeSessionServing {
    static let shared = ZCodeNavigationFixture()
    static let first = ZCodeTaskAddress(workspacePath: "/qa/workspace-a", taskId: "same-id")
    static let second = ZCodeTaskAddress(workspacePath: "/qa/workspace-b", taskId: "same-id")
    enum Mode { case normal, wrongSelected, deleted, rotated, disconnected }
    private var mode = Mode.normal
    private var selected: ZCodeTaskAddress?
    private(set) var metadataReads = 0
    private(set) var opens = 0
    private(set) var navigations: [ZCodeTaskAddress] = []
    private var generation: UInt64 { mode == .rotated ? 2 : 1 }
    func setMode(_ mode: Mode) { self.mode = mode }
    func counts() -> (reads: Int, opens: Int, navigations: [ZCodeTaskAddress]) { (metadataReads, opens, navigations) }
    func connect() async throws -> ZCodeSessionSnapshot { try await read() }
    func read() async throws -> ZCodeSessionSnapshot {
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        metadataReads += 1
        let addresses = mode == .deleted ? [Self.second] : [Self.first, Self.second]
        return ZCodeSessionSnapshot(instanceId: "qa-zcode-instance", generation: generation,
            scopes: [Self.first.scope, Self.second.scope],
            rows: addresses.map { ZCodeTaskRow(address: $0, title: "同名任务", liveStatus: "idle") },
            selected: addresses.contains(where: { $0 == selected }) ? selected : nil)
    }
    func open(_ selection: ZCodeSelection) async throws -> ZCodeOpenReply {
        opens += 1
        guard mode != .disconnected else { throw ZCodeFailure.unavailable }
        guard selection.instanceId == "qa-zcode-instance", selection.generation == generation else {
            return reply(.stale)
        }
        guard [Self.first, Self.second].contains(selection.address) else { return reply(.denied) }
        guard mode != .deleted || selection.address != Self.first else { return reply(.stale) }
        if mode == .wrongSelected { return reply(.confirmed, selected: Self.second) }
        selected = selection.address
        navigations.append(selection.address)
        return reply(.confirmed, selected: selected)
    }
    private func reply(_ status: ZCodeOpenResult, selected: ZCodeTaskAddress? = nil) -> ZCodeOpenReply {
        ZCodeOpenReply(instanceId: "qa-zcode-instance", generation: generation, status: status, selected: selected)
    }
    func close() async {}
}

extension Store {
    func prepareZCodeNavigationScenario() async {
        let fixture = ZCodeNavigationFixture.shared
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
                && deleted.navigations == successful.navigations
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
                && afterOld.navigations == successful.navigations
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
        let payload: [String: Any] = ["status": checks.count == 8 && checks.values.allSatisfy({ $0 }) ? "PASS" : "FAIL",
            "checks": checks, "body_reads": 0, "native_app": "NOT_RUN", "physical_key": "NOT_RUN"]
        if let audit = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"],
           let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys, .prettyPrinted]) {
            try? data.write(to: URL(fileURLWithPath: audit).deletingLastPathComponent().appendingPathComponent("zcode-navigation.json"))
        }
    }
}
#endif
