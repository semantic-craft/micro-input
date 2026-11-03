import XCTest
@testable import MicroInput
import MicroInputCore

private final class MockCommandRunner: CommandRunning, @unchecked Sendable {
    struct Invocation: Equatable {
        let executable: String
        let arguments: [String]
    }
    private let lock = NSLock()
    var invocations: [Invocation] = []
    var nextSucceeded: Bool = true
    var nextDetail: String = "ok"
    var simulatedDelayNanoseconds: UInt64 = 0
    var activeCount: Int = 0
    var maxConcurrentCount: Int = 0

    func run(executable: String, arguments: [String], timeoutSeconds: TimeInterval) async -> (Bool, String) {
        let delay: UInt64 = lock.withLock {
            activeCount += 1
            maxConcurrentCount = max(maxConcurrentCount, activeCount)
            return simulatedDelayNanoseconds
        }
        if delay > 0 {
            try? await Task.sleep(nanoseconds: delay)
        }
        return lock.withLock {
            activeCount -= 1
            invocations.append(Invocation(executable: executable, arguments: arguments))
            return (nextSucceeded, nextDetail)
        }
    }
}

final class HostExecutionTests: XCTestCase {
    @MainActor func testInstallCodingAgentPresetsAndMenuBarProfileSwitchingDecoupledFromWorkbench() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput()
        let executor = RecordingHostActionExecutor()
        let store = Store(backend: core, documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()

        await store.installCodingAgentPresets()
        let names = store.hostProfiles.map(\.name)
        XCTAssertTrue(names.contains("Antigravity"))
        XCTAssertTrue(names.contains("Herdr"))
        XCTAssertTrue(names.contains("DeepSeek Coder (DSH)"))

        let antigravity = store.hostProfiles.first(where: { $0.name == "Antigravity" })
        let herdr = store.hostProfiles.first(where: { $0.name == "Herdr" })
        let dsh = store.hostProfiles.first(where: { $0.name == "DeepSeek Coder (DSH)" })
        XCTAssertNotNil(antigravity)
        XCTAssertNotNil(herdr)
        XCTAssertNotNil(dsh)

        // Select Antigravity in Workbench editor
        if let agIdx = store.snapshot?.profiles.firstIndex(where: { $0.name == "Antigravity" }) {
            store.selectProfile(agIdx)
        }
        let workbenchProfileBefore = store.profile?.id.text

        // Switch hostActiveProfileId to Herdr via menu-bar method; Workbench selection must remain on Antigravity
        await store.setHostActiveProfile(herdr?.id.text)
        await store.setHostExecutionEnabled(true)
        XCTAssertEqual(store.hostActiveProfileId, herdr?.id.text)
        XCTAssertEqual(store.profile?.id.text, workbenchProfileBefore)

        // Reopen Store and verify hostActiveProfileId and hostExecutionEnabled survive restart
        let reopened = Store(backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await reopened.loadWorkbench()
        XCTAssertEqual(reopened.hostActiveProfileId, herdr?.id.text)
        XCTAssertTrue(reopened.hostExecutionEnabled)

        // When host_active_profile_id is unset (nil), both Store.hostActiveProfileId and Rust resolveInputAction
        // must fall back to "Antigravity" (and not profile_order.first() "日常办公").
        await reopened.setHostActiveProfile(nil)
        XCTAssertEqual(reopened.hostActiveProfileId, antigravity?.id.text)
        let fallbackAction = try XCTUnwrap(reopened.resolveInputActionForTesting(bindingId: "AG01"))
        XCTAssertTrue(fallbackAction.contains("com.google.antigravity"))

        await store.shutdown()
        await reopened.shutdown()
    }

    @MainActor func testDesktopProfilesDefaultToCindyAndRespectExecutionGate() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")
        let executor = RecordingHostActionExecutor()
        let store = Store(backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.prepareDesktopProfiles()
        XCTAssertEqual(store.profile?.name, "Cindy")
        XCTAssertEqual(Array(store.orderedHostProfiles.prefix(DesktopAgent.all.count)).map(\.name), DesktopAgent.all.map(\.name))
        XCTAssertFalse(store.hostProfiles.contains { $0.name.contains("演示") || $0.name.contains("示例") })
        await store.tryDesktopAction()
        XCTAssertTrue(executor.receipts.isEmpty, "Paused host execution must not run an action")
        await store.setHostExecutionEnabled(true)
        for agent in DesktopAgent.all {
            let index = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.name == agent.name })
            store.selectProfile(index)
            await store.setHostActiveProfile(store.profile?.id.text)
            await store.tryDesktopAction()
            XCTAssertEqual(executor.receipts.last?.target, agent.bundleID)
            store.slot = 14
            XCTAssertFalse(store.canTryDesktopAction)
        }
        let reopened = Store(backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await reopened.loadWorkbench()
        XCTAssertEqual(reopened.hostActiveProfileId, store.hostActiveProfileId)
        XCTAssertEqual(reopened.profile?.name, DesktopAgent.all.last?.name, "Reopening shows the active desktop agent, not a hidden example")
        await store.shutdown(); await reopened.shutdown()
    }

    @MainActor func testDesktopControlsRejectContentAndUnavailableReasoningModes() {
        XCTAssertFalse(DesktopAgentControls.extraControl(adapter: "dsh-desktop", command: "model", role: "AXStaticText", labels: ["选择模型，当前 DeepSeek"]))
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "dsh-desktop", command: "effort", role: "AXPopUpButton", labels: ["选择模型，当前 DeepSeek，推理等级 High"]))
        XCTAssertFalse(DesktopAgentControls.extraControl(adapter: "qoder-cn", command: "effort", role: "AXPopUpButton", labels: ["模型"]))
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "qwenwork", command: "model", role: "AXPopUpButton", labels: ["高级"]))
        XCTAssertFalse(DesktopAgentControls.extraControl(adapter: "qwenwork", command: "effort", role: "AXPopUpButton", labels: ["高级"]))
        XCTAssertFalse(DesktopAgentControls.extraControl(adapter: "zcode", command: "model", role: "AXButton", labels: ["Ran code about GLM-5.3"]))
    }

    @MainActor func testDelayedAXFocusIsConfirmedWithoutRepeatingTheAction() async {
        var reads = 0
        let confirmed = await DesktopAgentControls.confirmFocus({ reads += 1; return reads >= 3 }, interval: 1)
        XCTAssertTrue(confirmed)
        XCTAssertEqual(reads, 3)
        let absent = await DesktopAgentControls.confirmFocus({ false }, interval: 1)
        XCTAssertFalse(absent)
    }

    @MainActor func testAuditLogWriterSchemaAndPasteTextRedaction() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let logURL = directory.appendingPathComponent("audit.jsonl")

        let executor = RecordingHostActionExecutor(auditLogPath: logURL.path)
        let actionJSON = #"{"action_type":"paste_text","text":"请只做审查，不要提交\r\n"}"#
        let receipt = await executor.execute(actionJSON: actionJSON, bindingId: "AG02", profileId: "profile-ag")
        XCTAssertTrue(receipt.succeeded)
        XCTAssertEqual(receipt.outcome, "simulated")
        XCTAssertEqual(receipt.textByteLen, "请只做审查，不要提交".utf8.count)
        XCTAssertTrue(receipt.detail.contains("no_newline=true"))
        XCTAssertFalse(receipt.detail.contains("请只做审查"), "Audit receipt detail must not leak clipboard plaintext")

        let data = try Data(contentsOf: logURL)
        let line = try XCTUnwrap(String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines))
        XCTAssertFalse(line.contains("请只做审查"), "On-disk audit.jsonl must never contain clipboard plaintext")

        let json = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
        XCTAssertNotNil(json["timestamp_ms"] as? UInt64)
        XCTAssertEqual(json["profile_id"] as? String, "profile-ag")
        XCTAssertEqual(json["binding_id"] as? String, "AG02")
        XCTAssertEqual(json["action_type"] as? String, "paste_text")
        XCTAssertEqual(json["outcome"] as? String, "simulated")
        XCTAssertEqual(json["text_byte_len"] as? Int, "请只做审查，不要提交".utf8.count)
    }

    func testHerdrAndDshAdaptersWithMockCommandRunnerAndLargePipeDrain() async {
        let runner = MockCommandRunner()
        let herdr = HerdrAdapter(runner: runner, executableOverride: "/usr/bin/true")
        let dsh = DshAdapter(runner: runner, executableOverride: "/usr/bin/true")

        let focusRes = await herdr.focusSession("session-2")
        XCTAssertTrue(focusRes.0)
        XCTAssertEqual(runner.invocations.last?.arguments, ["focus-session", "session-2"])

        let sendTextRes = await herdr.sendTextNoNewline(session: "session-2", text: "请只做审查\r\n")
        XCTAssertTrue(sendTextRes.0)
        XCTAssertEqual(runner.invocations.last?.arguments, ["pane", "send-text", "session-2", "请只做审查"])

        let multiArgRes = await herdr.executeCommand("workspace next --wrap")
        XCTAssertTrue(multiArgRes.0)
        XCTAssertEqual(runner.invocations.last?.arguments, ["workspace", "next", "--wrap"])

        let emptySessionRes = await dsh.focusSession("unselected")
        XCTAssertFalse(emptySessionRes.0)

        let dshSendTextRes = await dsh.sendTextNoNewline(session: "session-1", text: "/explain\n")
        XCTAssertTrue(dshSendTextRes.0)
        XCTAssertEqual(runner.invocations.last?.arguments, ["pane", "send-text", "session-1", "/explain"])

        let dshCmdRes = await dsh.executeCommand("approve-current")
        XCTAssertTrue(dshCmdRes.0)
        XCTAssertEqual(runner.invocations.last?.arguments, ["approve-current"])

        runner.nextSucceeded = false
        runner.nextDetail = "退出码 127: executable not found"
        let failedRes = await dsh.executeCommand("approve-current")
        XCTAssertFalse(failedRes.0)
        XCTAssertTrue(failedRes.1.contains("executable not found"))

        // Verify adapter actor serializes concurrent calls across await suspension points
        runner.nextSucceeded = true
        runner.simulatedDelayNanoseconds = 30_000_000
        runner.maxConcurrentCount = 0
        async let c1 = herdr.executeCommand("cmd-1")
        async let c2 = herdr.executeCommand("cmd-2")
        async let c3 = herdr.focusSession("session-3")
        let concurrentResults = await [c1.0, c2.0, c3.0]
        XCTAssertEqual(concurrentResults, [true, true, true])
        XCTAssertEqual(runner.maxConcurrentCount, 1, "Adapter actor must serialize execution across suspension points")
        runner.simulatedDelayNanoseconds = 0

        // Verify runProcess drains >64 KiB (128 KiB) stdout without deadlocking
        let largeOut = await AgentAdapters.runProcess(
            executable: "/bin/sh",
            arguments: ["-c", "head -c 131072 /dev/zero | tr '\\0' 'x'"],
            timeoutSeconds: 2.0
        )
        XCTAssertTrue(largeOut.0)
        XCTAssertEqual(largeOut.1.utf8.count, 131072)
    }

    #if MI_SYNTHETIC_BACKEND
    @MainActor func testVerifiedInputLoopExecutesTargetedActionsAndIgnoresUnboundControls() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        let executor = RecordingHostActionExecutor()
        let store = Store(backend: core, documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        await store.setHostExecutionEnabled(true)

        try await core.setSyntheticDiscovery(state: .reconnectedDevice)
        try await core.setSyntheticConfig(scenario: 1)
        await store.refresh()
        store.selectDevice("synthetic-reconnected-interface-0")
        await store.connect("synthetic-reconnected-interface-0")
        await store.readIdentity()
        XCTAssertEqual(store.passThroughHealth, .snapshotMissing)
        await store.readDeviceConfig()
        XCTAssertNotEqual(store.passThroughHealth, .disconnected)
        XCTAssertNotEqual(store.passThroughHealth, .snapshotMissing)

        // While browsing device snapshot, switch hostActiveProfileId to Antigravity; browsingDeviceSnapshot must remain true!
        XCTAssertTrue(store.browsingDeviceSnapshot)
        let agId = try XCTUnwrap(store.hostProfiles.first(where: { $0.name == "Antigravity" })?.id.text)
        await store.setHostActiveProfile(agId)
        XCTAssertTrue(store.browsingDeviceSnapshot, "Switching hostActiveProfileId must not kick Workbench out of device snapshot view")
        store.showLocalDraft()

        // Wait briefly for hostInputLoop task to subscribe
        try await Task.sleep(nanoseconds: 50_000_000)

        // 1. Row-0 inversion: AG01 -> logical-0 (OpenApp com.google.antigravity) and AG00 -> logical-1 (Cmd+L)
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: false)
        try await core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: false)

        // 2. WideKeyMerger: AG10 press + AG11 press + AG10 release + AG11 release -> exactly 1 Cmd+Return action
        try await core.emitSyntheticBindingEvent(bindingId: "AG10", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG11", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG10", pressed: false)
        try await core.emitSyntheticBindingEvent(bindingId: "AG11", pressed: false)

        // 3. Joystick sector: AG15 -> logical-16 (Cmd+Opt+Up)
        try await core.emitSyntheticBindingEvent(bindingId: "AG15", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG15", pressed: false)

        for _ in 0..<25 where executor.receipts.count < 2 {
            try await Task.sleep(nanoseconds: 20_000_000)
        }

        XCTAssertEqual(executor.receipts.count, 2)
        XCTAssertEqual(executor.receipts[0].bindingId, "AG01")
        XCTAssertEqual(executor.receipts[0].actionType, "open_app")
        XCTAssertEqual(executor.receipts[0].target, "com.google.antigravity")

        XCTAssertEqual(executor.receipts[1].bindingId, "AG00")
        XCTAssertEqual(executor.receipts[1].actionType, "agent_command")
        XCTAssertEqual(executor.receipts[1].target, "antigravity")
        XCTAssertEqual(executor.receipts[1].detail, "antigravity/focus-input")

        await store.shutdown()
    }

    @MainActor func testInputStreamGapAutoResubscribesAndContinuesExecuting() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        let executor = RecordingHostActionExecutor()
        let store = Store(backend: core, documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        await store.setHostExecutionEnabled(true)

        let agId = try XCTUnwrap(store.hostProfiles.first(where: { $0.name == "Antigravity" })?.id.text)
        await store.setHostActiveProfile(agId)

        try await core.setSyntheticDiscovery(state: .reconnectedDevice)
        try await core.setSyntheticConfig(scenario: 1)
        await store.refresh()
        store.selectDevice("synthetic-reconnected-interface-0")
        await store.connect("synthetic-reconnected-interface-0")
        await store.readIdentity()

        try await Task.sleep(nanoseconds: 50_000_000)

        // Overflow the 512-entry bounded input queue to trigger InputStreamGap in hostInputTask
        try await core.emitSyntheticInput(count: 600)

        // Wait briefly for hostInputTask to catch InputStreamGap and re-subscribe
        try await Task.sleep(nanoseconds: 80_000_000)

        // Emit a valid binding event after the gap and verify it still routes and executes
        try await core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: false)

        for _ in 0..<25 where executor.receipts.isEmpty {
            try await Task.sleep(nanoseconds: 20_000_000)
        }

        XCTAssertEqual(executor.receipts.count, 1)
        XCTAssertEqual(executor.receipts.first?.bindingId, "AG00")
        XCTAssertEqual(executor.receipts.first?.actionType, "agent_command")
        XCTAssertEqual(executor.receipts.first?.target, "antigravity")
        XCTAssertEqual(executor.receipts.first?.detail, "antigravity/focus-input")

        await store.shutdown()
    }

    @MainActor func testAutoSetupHostExecutionInstallsPresetsConnectsAndReturnsToLocalDraft() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .reconnectedDevice)
        try await core.setSyntheticConfig(scenario: 1)

        let executor = RecordingHostActionExecutor()
        let store = Store(backend: core, documentURL: url, actionExecutor: executor)
        store.autoHostReady = true
        await store.loadWorkbench()

        let names = Set(store.hostProfiles.map(\.name))
        XCTAssertTrue(names.contains("Antigravity"))
        XCTAssertTrue(names.contains("Herdr"))
        XCTAssertTrue(names.contains("DeepSeek Coder (DSH)"))
        XCTAssertTrue(store.hostExecutionEnabled)
        XCTAssertEqual(store.connectedID, "synthetic-reconnected-interface-0")
        XCTAssertEqual(store.identity?.version, "synthetic-0.6.3")
        XCTAssertNotEqual(store.passThroughHealth, .disconnected)
        XCTAssertNotEqual(store.passThroughHealth, .snapshotMissing)
        XCTAssertFalse(store.browsingDeviceSnapshot)
        XCTAssertTrue(store.canEditDraft)

        // E-096: Sleep/wake with autoHostReady = true must NOT auto-reopen the device
        store.systemWillSleep()
        XCTAssertNil(store.connectedID)
        await store.systemDidWake()
        XCTAssertNil(store.connectedID)

        // Explicit connect then suspend() + refresh() must remain disconnected
        store.selectDevice("synthetic-reconnected-interface-0")
        await store.connect("synthetic-reconnected-interface-0")
        XCTAssertEqual(store.connectedID, "synthetic-reconnected-interface-0")
        await store.suspend()
        await store.refresh()
        XCTAssertNil(store.connectedID)

        // R-D04: Multiple USB candidates (.twoDevices) must NOT auto-open on startup
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let storeMulti = Store(backend: core, documentURL: url, actionExecutor: executor)
        storeMulti.autoHostReady = true
        await storeMulti.loadWorkbench()
        XCTAssertNil(storeMulti.connectedID)

        await storeMulti.shutdown()
        await store.shutdown()
    }
    #endif
}
