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
    @MainActor func testShellCommandRequiresExplicitConsentAndPersistsAcrossRestart() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.hostProfiles.first { $0.name == "Antigravity" })
        let profileIndex = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.id.text == profile.id.text })
        store.selectProfile(profileIndex)
        await store.setHostActiveProfile(profile.id.text)
        await store.setHostExecutionEnabled(true)
        store.slot = 0
        await store.searchActions(query: "命令动作", category: "命令", favoritesOnly: false)
        let template = try XCTUnwrap(store.library.entries.first { $0.action.id == "shell-command" }?.action)
        await store.assign(template.id)
        XCTAssertEqual(store.draft.edits[store.slotKey]?.payload["type"]?.text, "shell_command")
        XCTAssertEqual(store.profile?.id.text, profile.id.text, "workbench profile mismatch")
        XCTAssertEqual(store.hostActiveProfileId, profile.id.text, "active profile mismatch")
        XCTAssertFalse(store.editingDevice, "assign accidentally entered device editor")

        XCTAssertNil(store.resolveInputActionForTesting(bindingId: "AG00"), "Command consent must default to off")
        await store.setCommandExecutionEnabled(true)
        let resolved = try XCTUnwrap(store.resolveInputActionForTesting(bindingId: "AG00"))
        let payload = try JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(resolved.utf8))
        XCTAssertEqual(payload.action_type, "shell_command")
        XCTAssertEqual(payload.working_directory, "/tmp")
        XCTAssertTrue(store.actionCapabilities[store.slotKey]?.execution_available == true, "Workbench capability must follow the command consent switch")
        XCTAssertTrue(
            store.library.entries.first { $0.action.id == "shell-command" }?.capability.execution_available == true,
            "Action library capability must follow the command consent switch"
        )

        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        XCTAssertTrue(reopened.commandExecutionEnabled, "Command consent must persist independently")
        XCTAssertNotNil(reopened.resolveInputActionForTesting(bindingId: "AG00"))
        await store.shutdown()
        await reopened.shutdown()
    }

    @MainActor func testLiveShellCommandRunsInWorkingDirectoryAndRedactsCommandAndOutput() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let auditPath = directory.appendingPathComponent("audit.jsonl").path
        let executor = LiveHostActionExecutor(auditLogPath: auditPath)
        let command = #"printf secret-command-output > shell-marker"#
        let actionJSON = #"{"action_type":"shell_command","command":"\#(command)","working_directory":"\#(directory.path)"}"#
        let receipt = await executor.execute(actionJSON: actionJSON, bindingId: "AG00", profileId: "profile-command")

        XCTAssertTrue(receipt.succeeded)
        XCTAssertEqual(receipt.actionType, "shell_command")
        XCTAssertEqual(receipt.target, directory.path)
        XCTAssertEqual(receipt.textByteLen, command.utf8.count)
        XCTAssertFalse(receipt.detail.contains(command))
        XCTAssertFalse(receipt.detail.contains("secret-command-output"))
        XCTAssertEqual(try String(contentsOf: directory.appendingPathComponent("shell-marker"), encoding: .utf8), "secret-command-output")

        let audit = try String(contentsOfFile: auditPath, encoding: .utf8)
        XCTAssertFalse(audit.contains(command))
        XCTAssertFalse(audit.contains("secret-command-output"))
    }

    @MainActor func testTryActionRunsAShellCommandOnceOnlyAfterConsent() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let auditPath = directory.appendingPathComponent("audit.jsonl").path
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(
            nativeOwnership: NativeOwnershipController(isolated: true),
            backend: MicroInputCore.MicroInput(),
            documentURL: url,
            actionExecutor: LiveHostActionExecutor(auditLogPath: auditPath)
        )
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.hostProfiles.first { $0.name == "Antigravity" })
        let profileIndex = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.id.text == profile.id.text })
        store.selectProfile(profileIndex)
        await store.setHostActiveProfile(profile.id.text)
        await store.setHostExecutionEnabled(true)
        store.slot = 0
        await store.searchActions(query: "命令动作", category: "命令", favoritesOnly: false)
        var commandAction = try XCTUnwrap(
            store.library.entries.first { $0.action.id == "shell-command" }?.action
        )
        let command = #"printf command-result > ui-command-marker"#
        commandAction.payload["command"] = .string(command)
        commandAction.payload["working_directory"] = .string(directory.path)
        let saved = await store.saveAction(commandAction)
        XCTAssertTrue(saved)

        XCTAssertFalse(store.canTryDesktopAction, "Default consent must leave the try action disabled")
        XCTAssertFalse(FileManager.default.fileExists(atPath: directory.appendingPathComponent("ui-command-marker").path))
        await store.setCommandExecutionEnabled(true)
        XCTAssertTrue(store.actionCapabilities[store.slotKey]?.execution_available == true)
        XCTAssertTrue(store.canTryDesktopAction, "Explicit consent enables the visible try action")

        await store.tryDesktopAction()
        let receipt = try XCTUnwrap(store.lastHostActionReceipt)
        XCTAssertTrue(receipt.succeeded)
        XCTAssertEqual(receipt.actionType, "shell_command")
        XCTAssertEqual(receipt.target, directory.path)
        XCTAssertEqual(receipt.textByteLen, command.utf8.count)
        XCTAssertEqual(receipt.detail, "exit=0 cwd_bytes=\(directory.path.utf8.count) command_bytes=\(command.utf8.count)")
        XCTAssertEqual(
            try String(contentsOf: directory.appendingPathComponent("ui-command-marker"), encoding: .utf8),
            "command-result"
        )
        let audit = try String(contentsOfFile: auditPath, encoding: .utf8)
        XCTAssertFalse(audit.contains(command))
        XCTAssertFalse(audit.contains("command-result"))
        await store.shutdown()
    }

    @MainActor func testInstallCodingAgentPresetsAndMenuBarProfileSwitchingDecoupledFromWorkbench() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput()
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url, actionExecutor: executor)
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
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await reopened.loadWorkbench()
        XCTAssertEqual(reopened.hostActiveProfileId, herdr?.id.text)
        XCTAssertTrue(reopened.hostExecutionEnabled)

        // When host_active_profile_id is unset (nil), both Store.hostActiveProfileId and Rust resolveInputAction
        // must fall back to "Antigravity" (and not profile_order.first() "日常办公").
        await reopened.setHostActiveProfile(nil)
        XCTAssertEqual(reopened.hostActiveProfileId, antigravity?.id.text)
        let fallbackAction = try XCTUnwrap(reopened.resolveInputActionForTesting(bindingId: "AG00"))
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
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.prepareDesktopProfiles()
        XCTAssertEqual(store.profile?.name, "Cindy")
        XCTAssertEqual(store.hostActiveProfileId, store.profile?.id.text)
        XCTAssertFalse(store.routing?.manual_override ?? true, "Initial default is not a manual override")
        XCTAssertEqual(Array(store.orderedHostProfiles.prefix(DesktopAgent.all.count)).map(\.name), DesktopAgent.all.map(\.name))
        XCTAssertFalse(store.hostProfiles.contains { $0.name.contains("演示") || $0.name.contains("示例") })
        await store.tryDesktopAction()
        XCTAssertTrue(executor.receipts.isEmpty, "Paused host execution must not run an action")
        await store.setHostExecutionEnabled(true)
        for agent in DesktopAgent.all {
            let index = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.name == agent.name })
            store.selectProfile(index)
            await store.setHostActiveProfile(store.profile?.id.text)
            if ["Cindy", "Codex"].contains(agent.name) {
                XCTAssertTrue(store.isProtectedLayerSelected)
                XCTAssertFalse(store.canTryDesktopAction)
                await store.tryDesktopAction()
                continue
            }
            await store.tryDesktopAction()
            XCTAssertEqual(executor.receipts.last?.target, agent.bundleID)
            store.slot = 14
            XCTAssertEqual(store.canTryDesktopAction, agent.name != "Herdr")
            store.slot = agent.name == "Herdr" ? 13 : 10
            XCTAssertEqual(store.canTryDesktopAction, agent.name == "Antigravity")
        }
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await reopened.loadWorkbench()
        XCTAssertEqual(reopened.hostActiveProfileId, store.hostActiveProfileId)
        XCTAssertEqual(reopened.profile?.name, DesktopAgent.all.last?.name, "Reopening shows the active desktop agent, not a hidden example")
        await store.shutdown(); await reopened.shutdown()
    }

    @MainActor func testQwenDictationRecognizesShortcutSuffixAndBothStates() {
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "qwenwork", command: "dictation", role: "AXButton", labels: ["停止录音（Right ⌘）"]))
        XCTAssertTrue(DesktopAgentControls.dictationState(labels: ["停止录音（Right ⌘）"], recording: true))
        XCTAssertTrue(DesktopAgentControls.dictationState(labels: ["语音输入（Right ⌘）"], recording: false))
        XCTAssertFalse(DesktopAgentControls.dictationState(labels: ["语音输入（Right ⌘）"], recording: true))
    }

    @MainActor func testAntigravityDictationLiveVoiceAndProjectControlsRecognizeAllStates() {
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "antigravity", command: "dictation", role: "AXButton", labels: ["Record Audio"]))
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "antigravity", command: "dictation", role: "AXButton", labels: ["Stop Recording"]))
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "antigravity", command: "dictation", role: "AXButton", labels: ["Recording"]))
        XCTAssertTrue(DesktopAgentControls.dictationState(labels: ["Record Audio"], recording: false))
        XCTAssertTrue(DesktopAgentControls.dictationState(labels: ["Stop Recording"], recording: true))
        XCTAssertTrue(DesktopAgentControls.dictationState(labels: ["Recording"], recording: true))

        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "antigravity", command: "live-voice", role: "AXButton", labels: ["Start Live Chat"]))
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "antigravity", command: "live-voice", role: "AXButton", labels: ["End Live chat"]))
        XCTAssertTrue(DesktopAgentControls.liveVoiceState(labels: ["Start Live Chat"], active: false))
        XCTAssertTrue(DesktopAgentControls.liveVoiceState(labels: ["Start Voice mode"], active: false))
        XCTAssertTrue(DesktopAgentControls.liveVoiceState(labels: ["End Live chat"], active: true))
        XCTAssertTrue(DesktopAgentControls.liveVoiceState(labels: ["Live chat, 0:12"], active: true))
        XCTAssertFalse(DesktopAgentControls.liveVoiceState(labels: ["Start Live Chat"], active: true))

        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "antigravity", command: "project", role: "AXButton", labels: ["Select project"]))
    }

    @MainActor func testDialScrollRequestMathCoalescingAndWindowTargeting() async throws {
        defer { DesktopAgentControls.resetScrollHooks() }

        XCTAssertEqual(DialScrollRequest.from(command: "scroll-up")?.mode, .lines(-3))
        XCTAssertEqual(DialScrollRequest.from(command: "scroll-down")?.mode, .lines(3))
        XCTAssertEqual(DialScrollRequest.from(command: "scroll-up", dialHeld: true)?.mode, .lines(-15))
        XCTAssertEqual(DialScrollRequest.from(command: "scroll-down", dialHeld: true)?.mode, .lines(15))
        XCTAssertEqual(DialScrollRequest.from(command: "scroll-to-bottom")?.mode, .bottom)
        XCTAssertEqual(DialScrollRequest.from(command: "scroll-lines:42")?.mode, .lines(42))
        XCTAssertEqual(DialScrollRequest.from(command: "scroll-lines:250")?.mode, .lines(100))
        XCTAssertNil(DialScrollRequest.from(command: "scroll-lines:0"))

        // Positive lines (scroll down) map to negative wheel1 delta; clamped to [-100, 100].
        XCTAssertEqual(DialScrollRequest.wheelDelta(lines: 3), -3)
        XCTAssertEqual(DialScrollRequest.wheelDelta(lines: -15), 15)
        XCTAssertEqual(DialScrollRequest.wheelDelta(lines: 180), -100)
        XCTAssertEqual(DialScrollRequest.wheelDelta(lines: -180), 100)

        let down3 = try XCTUnwrap(DialScrollRequest.from(command: "scroll-down"))
        let down15 = try XCTUnwrap(DialScrollRequest.from(command: "scroll-down-fast"))
        let merged18 = try XCTUnwrap(down3.coalesced(with: down15))
        XCTAssertEqual(merged18.mode, .lines(18))
        XCTAssertEqual(merged18.canonicalCommand, "scroll-lines:18")

        let down90 = try XCTUnwrap(DialScrollRequest.from(command: "scroll-lines:90"))
        let capped100 = try XCTUnwrap(down90.coalesced(with: down15))
        XCTAssertEqual(capped100.mode, .lines(100))
        XCTAssertEqual(capped100.canonicalCommand, "scroll-lines:100")

        let windows: [[String: Any]] = [
            [
                kCGWindowOwnerPID as String: pid_t(777),
                kCGWindowLayer as String: 3, // non-normal overlay layer ignored
                kCGWindowNumber as String: 10,
                kCGWindowBounds as String: CGRect(x: 0, y: 0, width: 800, height: 600).dictionaryRepresentation as NSDictionary
            ],
            [
                kCGWindowOwnerPID as String: pid_t(777),
                kCGWindowLayer as String: 0,
                kCGWindowNumber as String: 11,
                kCGWindowBounds as String: CGRect(x: 0, y: 0, width: 60, height: 60).dictionaryRepresentation as NSDictionary // too small ignored
            ],
            [
                kCGWindowOwnerPID as String: pid_t(777),
                kCGWindowLayer as String: 0,
                kCGWindowNumber as String: 42,
                kCGWindowBounds as String: CGRect(x: 100, y: 200, width: 1000, height: 800).dictionaryRepresentation as NSDictionary
            ]
        ]
        let matched = try XCTUnwrap(DialScrollWindow.frontmost(in: windows, processID: 777))
        XCTAssertEqual(matched.windowNumber, 42)
        XCTAssertEqual(matched.scrollPoint(), CGPoint(x: 600, y: 520))

        var warpedPoints: [CGPoint] = []
        var postedWheels: [(Int32, Int)] = []
        var focusedAdapters: [String] = []
        DesktopAgentControls.scrollTrustChecker = { true }
        DesktopAgentControls.scrollRunningProcessID = { bundle in
            bundle == "com.google.antigravity" ? 777 : nil
        }
        DesktopAgentControls.scrollWindowFinder = { pid in
            pid == 777 ? matched : nil
        }
        DesktopAgentControls.scrollMouseLocation = { CGPoint(x: 12, y: 34) }
        DesktopAgentControls.scrollMouseWarper = { warpedPoints.append($0) }
        DesktopAgentControls.scrollWheelPoster = { delta, win in
            postedWheels.append((delta, win))
            return true
        }
        DesktopAgentControls.scrollSleeper = { _ in }
        DesktopAgentControls.scrollInputFocuser = { adapter, _ in
            focusedAdapters.append(adapter)
            return true
        }

        let (okDown, detailDown) = await DesktopAgentControls.performScroll(adapter: "antigravity", command: "scroll-down")
        XCTAssertTrue(okDown)
        XCTAssertTrue(detailDown.contains("antigravity/scroll-down"))
        XCTAssertEqual(warpedPoints, [CGPoint(x: 600, y: 520), CGPoint(x: 12, y: 34)], "Cursor must warp to conversation point and restore original location")
        XCTAssertEqual(postedWheels.map(\.0), [-3])
        XCTAssertEqual(postedWheels.map(\.1), [42])

        warpedPoints.removeAll()
        postedWheels.removeAll()
        let (okBottom, detailBottom) = await DesktopAgentControls.performScroll(adapter: "antigravity", command: "scroll-to-bottom")
        XCTAssertTrue(okBottom)
        XCTAssertTrue(detailBottom.contains("lines=1000"))
        XCTAssertEqual(postedWheels.count, 10)
        XCTAssertTrue(postedWheels.allSatisfy { $0.0 == -100 && $0.1 == 42 })
        XCTAssertEqual(focusedAdapters, ["antigravity"])
        XCTAssertEqual(warpedPoints, [CGPoint(x: 600, y: 520), CGPoint(x: 12, y: 34)])

        // Non-running target app must not warp cursor or post wheel events to non-target windows.
        warpedPoints.removeAll()
        postedWheels.removeAll()
        let (okMissing, _) = await DesktopAgentControls.performScroll(adapter: "cindy", command: "scroll-down")
        XCTAssertFalse(okMissing)
        XCTAssertTrue(warpedPoints.isEmpty)
        XCTAssertTrue(postedWheels.isEmpty)
    }

    @MainActor func testDialQueueCoalescesPendingScrollStepsAndSuppressesDuplicateJumpToBottom() async throws {
        final class SlowRecordingExecutor: HostActionExecuting {
            var details: [String] = []
            func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt {
                try? await Task.sleep(nanoseconds: 40_000_000)
                let payload = try! JSONDecoder().decode(ResolvedHostActionPayload.self, from: Data(actionJSON.utf8))
                let meta = RecordingHostActionExecutor.describeMetadata(payload)
                details.append(meta.detail)
                return HostActionReceipt(
                    timestampMs: UInt64(Date().timeIntervalSince1970 * 1000),
                    profileId: profileId,
                    bindingId: bindingId,
                    actionType: payload.action_type,
                    target: meta.target,
                    textByteLen: nil,
                    outcome: "simulated",
                    detail: meta.detail
                )
            }
        }

        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let executor = SlowRecordingExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        await store.setHostExecutionEnabled(true)
        let state = try XCTUnwrap(store.routing)
        let profile = try XCTUnwrap(state.profile_id)

        // All 4 synchronous scroll-down steps on MainActor coalesce in pendingHostActions into scroll-lines:12 before the Task drains.
        let downJSON = #"{"action_type":"agent_command","adapter":"antigravity","command":"scroll-down"}"#
        let bottomJSON = #"{"action_type":"agent_command","adapter":"antigravity","command":"scroll-to-bottom"}"#
        store.enqueueHostAction(json: downJSON, binding: "AG14", profile: profile, state: state)
        store.enqueueHostAction(json: downJSON, binding: "AG14", profile: profile, state: state)
        store.enqueueHostAction(json: downJSON, binding: "AG14", profile: profile, state: state)
        store.enqueueHostAction(json: downJSON, binding: "AG14", profile: profile, state: state)
        // Two queued scroll-to-bottom requests collapse into one pending jump-to-bottom burst.
        store.enqueueHostAction(json: bottomJSON, binding: "ENC_CLK", profile: profile, state: state)
        store.enqueueHostAction(json: bottomJSON, binding: "ENC_CLK", profile: profile, state: state)

        for _ in 0..<40 where store.hostExecutionTask != nil || !store.pendingHostActions.isEmpty {
            try await Task.sleep(nanoseconds: 15_000_000)
        }
        XCTAssertEqual(executor.details, [
            "antigravity/scroll-lines:12",
            "antigravity/scroll-to-bottom"
        ])
        await store.shutdown()
    }

    @MainActor func testDesktopControlsRejectContentAndUnavailableReasoningModes() {
        XCTAssertFalse(DesktopAgentControls.extraControl(adapter: "dsh-desktop", command: "model", role: "AXStaticText", labels: ["选择模型，当前 DeepSeek"]))
        XCTAssertTrue(DesktopAgentControls.extraControl(adapter: "dsh-desktop", command: "effort", role: "AXPopUpButton", labels: ["选择模型，当前 DeepSeek，推理等级 High"]))
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
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url, actionExecutor: executor)
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

        // 1. Observed row-0 order: AG00 -> logical-0 (OpenApp com.google.antigravity) and AG01 -> logical-1 (Cmd+L)
        try await core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: false)
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: false)

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
        XCTAssertEqual(executor.receipts[0].bindingId, "AG00")
        XCTAssertEqual(executor.receipts[0].actionType, "open_app")
        XCTAssertEqual(executor.receipts[0].target, "com.google.antigravity")

        XCTAssertEqual(executor.receipts[1].bindingId, "AG01")
        XCTAssertEqual(executor.receipts[1].actionType, "agent_command")
        XCTAssertEqual(executor.receipts[1].target, "antigravity")
        XCTAssertEqual(executor.receipts[1].detail, "antigravity/focus-input")

        // 4. Dial clockwise/counterclockwise/click and held-dial fast scroll on Antigravity, Qianwen, ZCode, DSH
        for (agentName, adapter) in [("Antigravity", "antigravity"), ("千问办公", "qwenwork"), ("ZCode", "zcode"), ("DeepSeek Coder (DSH)", "dsh-desktop")] {
            let pid = try XCTUnwrap(store.hostProfiles.first(where: { $0.name == agentName })?.id.text)
            await store.setHostActiveProfile(pid)
            let startIndex = executor.receipts.count

            try await core.emitSyntheticBindingEvent(bindingId: "AG13", pressed: true)
            try await core.emitSyntheticBindingEvent(bindingId: "AG13", pressed: false)
            try await core.emitSyntheticBindingEvent(bindingId: "AG14", pressed: true)
            try await core.emitSyntheticBindingEvent(bindingId: "AG14", pressed: false)
            // Hold ENC_CLK and turn AG14 -> upgrades to scroll-down-fast
            try await core.emitSyntheticBindingEvent(bindingId: "ENC_CLK", pressed: true)
            try await core.emitSyntheticBindingEvent(bindingId: "AG14", pressed: true)
            try await core.emitSyntheticBindingEvent(bindingId: "AG14", pressed: false)
            try await core.emitSyntheticBindingEvent(bindingId: "ENC_CLK", pressed: false)

            for _ in 0..<25 where executor.receipts.count - startIndex < 4 {
                try await Task.sleep(nanoseconds: 20_000_000)
            }
            let expectedDetails = adapter == "antigravity" ? [
                "antigravity/prev-conversation",
                "antigravity/next-conversation",
                "antigravity/search",
                "antigravity/next-conversation"
            ] : [
                "\(adapter)/scroll-up",
                "\(adapter)/scroll-down",
                "\(adapter)/scroll-to-bottom",
                "\(adapter)/scroll-down-fast"
            ]
            XCTAssertEqual(Array(executor.receipts.dropFirst(startIndex)).map(\.detail), expectedDetails)
        }

        await store.shutdown()
    }

    @MainActor func testInputStreamGapRequiresExplicitResumeAndReleaseBeforeExecuting() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url, actionExecutor: executor)
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

        // Overflow the 256-entry reader broadcast queue to trigger InputStreamGap in hostInputTask
        try await core.emitSyntheticInput(count: 600)

        // A gap invalidates the subscription, not the device session.
        try await Task.sleep(nanoseconds: 80_000_000)

        XCTAssertTrue(store.inputNeedsResume)
        XCTAssertNotNil(store.connectedID)
        XCTAssertTrue(store.hostExecutionEnabled)
        store.resumeInput()
        try await Task.sleep(nanoseconds: 50_000_000)
        // Quarantine the first held press until an observed release.
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: false)
        try await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertTrue(executor.receipts.isEmpty)
        // A fresh press after release is eligible.
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG01", pressed: false)

        for _ in 0..<25 where executor.receipts.isEmpty {
            try await Task.sleep(nanoseconds: 20_000_000)
        }

        XCTAssertEqual(executor.receipts.count, 1)
        XCTAssertEqual(executor.receipts.first?.bindingId, "AG01")
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
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url, actionExecutor: executor)
        store.autoHostReady = true
        await store.loadWorkbench()

        let names = Set(store.hostProfiles.map(\.name))
        XCTAssertTrue(names.contains("Antigravity"))
        XCTAssertTrue(names.contains("Herdr"))
        XCTAssertTrue(names.contains("DeepSeek Coder (DSH)"))
        XCTAssertFalse(store.hostExecutionEnabled, "Startup must preserve the persisted execution pause")
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
        let storeMulti = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url, actionExecutor: executor)
        storeMulti.autoHostReady = true
        await storeMulti.loadWorkbench()
        XCTAssertNil(storeMulti.connectedID)

        await storeMulti.shutdown()
        await store.shutdown()
    }

    @MainActor func testPrepareHostExecutionScenarioProducesAllTwentyReceipts() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.prepareHostExecutionScenario()
        XCTAssertEqual(executor.receipts.count, 21, "Receipts: \(executor.receipts.map(\.detail)) status=\(store.status) profile=\(store.profile?.name ?? "nil") active=\(store.hostActiveProfileId ?? "nil")")
        let details = Set(executor.receipts.map(\.detail))
        let bindings = Set(executor.receipts.map(\.bindingId))
        XCTAssertTrue(bindings.contains("menu-navigate-back"))
        XCTAssertTrue(bindings.contains("menu-navigate-forward"))
        XCTAssertTrue(details.contains("no_newline=true (bytes=8)"))
        XCTAssertTrue(details.contains(where: { $0.hasSuffix("/submit") }))
        await store.shutdown()
    }
    #endif

    func testKeypadNavigationHistoryStateMachineCoversBackForwardTruncationDedupSkipAndCapacity() {
        var history = KeypadNavigationHistory()
        let herdr = KeypadNavigationPlace.app(bundleID: "dev.herdr.app")
        let antigravity = KeypadNavigationPlace.app(bundleID: "com.google.antigravity")
        let qwen = KeypadNavigationPlace.app(bundleID: "cn.qwenwork.desktop.mac")
        let cindy = KeypadNavigationPlace.app(bundleID: "com.xd.cindy")
        let codex = KeypadNavigationPlace.app(bundleID: "com.openai.codex")

        XCTAssertFalse(history.canGoBack())
        XCTAssertFalse(history.canGoForward())
        XCTAssertNil(history.back())
        XCTAssertNil(history.forward())

        for place in [herdr, antigravity, qwen, cindy, codex] {
            history.visit(place)
        }
        // Consecutive duplicate visit is deduplicated
        history.visit(codex)
        XCTAssertEqual(history.entries.count, 5)
        XCTAssertEqual(history.current, codex)

        // Step back across Codex -> Cindy -> Qwen -> Antigravity -> Herdr
        XCTAssertEqual(history.back(), cindy)
        XCTAssertEqual(history.back(), qwen)
        // Re-visiting current place while in the middle of history preserves forward stack
        history.visit(qwen)
        XCTAssertTrue(history.canGoForward())
        XCTAssertEqual(history.back(), antigravity)
        XCTAssertEqual(history.back(), herdr)
        XCTAssertNil(history.back())

        // Step forward Herdr -> Antigravity -> Qwen
        XCTAssertEqual(history.forward(), antigravity)
        XCTAssertEqual(history.forward(), qwen)

        // Visiting a new place after stepping back truncates forward branch (cindy, codex)
        let sessionPlace = KeypadNavigationPlace.session(adapter: "herdr", session: "agent-1")
        history.visit(sessionPlace)
        XCTAssertFalse(history.canGoForward())
        XCTAssertNil(history.forward())
        XCTAssertEqual(history.entries, [herdr, antigravity, qwen, sessionPlace])

        // Skipping unavailable places in both directions
        XCTAssertEqual(history.back(available: { $0 != qwen }), antigravity)
        XCTAssertEqual(history.forward(available: { $0 != qwen }), sessionPlace)
        XCTAssertNil(history.back(available: { _ in false }))
        XCTAssertEqual(history.current, sessionPlace)

        // Capacity cap of 50 entries
        var capped = KeypadNavigationHistory()
        for i in 0..<60 {
            capped.visit(.session(adapter: "herdr", session: "s-\(i)"))
        }
        XCTAssertEqual(capped.entries.count, KeypadNavigationHistory.maxHistory)
        XCTAssertEqual(capped.entries.first, .session(adapter: "herdr", session: "s-10"))
        XCTAssertEqual(capped.current, .session(adapter: "herdr", session: "s-59"))
    }

    @MainActor func testCrossAgentKeypadBackAndForwardReplaysHistoryWithoutBreakingForwardStack() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let executor = RecordingHostActionExecutor()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        await store.setHostExecutionEnabled(true)

        let apps = [
            "dev.herdr.app",
            "com.google.antigravity",
            "cn.qwenwork.desktop.mac",
            "com.xd.cindy",
            "com.openai.codex"
        ]
        for bundleID in apps {
            let receipt = await store.executeHostAction(
                actionJSON: #"{"action_type":"open_app","bundle_id":"\#(bundleID)"}"#,
                bindingId: "AG00",
                profileId: "p-test"
            )
            XCTAssertEqual(receipt?.outcome, "simulated")
        }
        // Consecutive action in the same app (Codex focus-input) does not duplicate the top history entry
        _ = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"codex","command":"focus-input"}"#,
            bindingId: "AG01",
            profileId: "p-test"
        )
        // Non-activating dial scroll does not pollute navigation history
        _ = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"antigravity","command":"scroll-down"}"#,
            bindingId: "ENC_CW",
            profileId: "p-test"
        )
        XCTAssertEqual(store.navigationHistory.entries.count, 5)
        XCTAssertEqual(store.navigationHistory.current, .app(bundleID: "com.openai.codex"))

        // Back: Codex -> Cindy -> Qwen
        let back1 = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"navigation","command":"back"}"#,
            bindingId: "AG11",
            profileId: "p-test"
        )
        XCTAssertEqual(back1?.target, "com.xd.cindy")
        XCTAssertTrue(store.navigationHistory.canGoForward())

        let back2 = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"navigation","command":"back"}"#,
            bindingId: "AG11",
            profileId: "p-test"
        )
        XCTAssertEqual(back2?.target, "cn.qwenwork.desktop.mac")

        // Forward: Qwen -> Cindy
        let forward1 = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"navigation","command":"forward"}"#,
            bindingId: "AG12",
            profileId: "p-test"
        )
        XCTAssertEqual(forward1?.target, "com.xd.cindy")
        XCTAssertTrue(store.navigationHistory.canGoForward())

        // Opening Herdr anew after stepping back truncates Codex from forward stack
        _ = await store.executeHostAction(
            actionJSON: #"{"action_type":"open_app","bundle_id":"dev.herdr.app"}"#,
            bindingId: "AG00",
            profileId: "p-test"
        )
        XCTAssertFalse(store.navigationHistory.canGoForward())
        let blockedForward = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"navigation","command":"forward"}"#,
            bindingId: "AG12",
            profileId: "p-test"
        )
        XCTAssertEqual(blockedForward?.outcome, "blocked")
        XCTAssertEqual(blockedForward?.actionType, "navigate_forward")

        await store.shutdown()
    }

    @MainActor func testJoystickPromptSlotsFocusTargetInputStashRestoreClipboardAndDefaultToPasteOnly() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")
        let auditPath = directory.appendingPathComponent("audit.jsonl").path

        let pasteboard = InMemoryPasteboard(items: [
            [.string: Data("user-original-clip".utf8), .rtf: Data("{\\rtf1 original}".utf8)]
        ])
        let executor = LiveHostActionExecutor(auditLogPath: auditPath)
        executor.pasteboard = pasteboard
        executor.clipboardRestoreDelayNanoseconds = 5_000_000
        executor.submitSettleDelayNanoseconds = 1_000_000
        executor.macroAccessibilityChecker = { true }
        executor.macroSleeper = { _ in }

        var callLog: [String] = []
        var pastedClipboardSnapshots: [String?] = []
        var focusShouldSucceed = true

        executor.appActivator = { bundleId in
            callLog.append("activate:\(bundleId)")
            return true
        }
        executor.agentInputFocuser = { adapter in
            callLog.append("focus:\(adapter)")
            return focusShouldSucceed ? (true, "已确认输入框焦点；未输入或发送文本") : (false, "输入框未确认获得焦点")
        }
        executor.activeDictationStopper = { adapter in
            callLog.append("stop_dictation:\(adapter)")
            return (true, "dictation_inactive:\(adapter)")
        }
        executor.pasteShortcutPoster = {
            callLog.append("cmd_v")
            pastedClipboardSnapshots.append(pasteboard.currentPlainText)
            return true
        }
        executor.returnKeyPoster = {
            callLog.append("return")
            return true
        }

        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        await store.setHostExecutionEnabled(true)

        let targetAgents: [(name: String, adapter: String, bundleID: String)] = [
            ("Antigravity", "antigravity", "com.google.antigravity"),
            ("千问办公", "qwenwork", "cn.qwenwork.desktop.mac"),
            ("ZCode", "zcode", "dev.zcode.app"),
            ("DeepSeek Coder (DSH)", "dsh-desktop", "com.deepseek.dsh")
        ]

        let promptIDs = ["prompt-continue", "prompt-test", "prompt-review", "prompt-summary"]
        let expectedTexts = [
            "Continue",
            "Run the tests and fix any failures",
            "Review your diff for bugs",
            "Summarize where things stand and what's left"
        ]
        let bindings = ["AG15", "AG16", "AG17", "AG18"]

        for agent in targetAgents {
            let profile = try XCTUnwrap(store.hostProfiles.first { $0.name == agent.name })
            let profileIdx = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.id.text == profile.id.text })
            store.selectProfile(profileIdx)
            await store.setHostActiveProfile(profile.id.text)

            // Assign 4-way joystick sectors (slots 16..19 -> AG15..AG18)
            for (offset, promptID) in promptIDs.enumerated() {
                store.slot = 16 + offset
                await store.assign(promptID)
            }

            for (offset, binding) in bindings.enumerated() {
                callLog.removeAll()
                pastedClipboardSnapshots.removeAll()
                let resolved = try XCTUnwrap(store.resolveInputActionForTesting(bindingId: binding))
                let rawReceipt = await store.executeHostAction(actionJSON: resolved, bindingId: binding, profileId: profile.id.text)
                let receipt = try XCTUnwrap(rawReceipt)
                await executor.flushClipboardRestoreForTesting()

                XCTAssertEqual(receipt.outcome, "executed")
                XCTAssertEqual(receipt.actionType, "paste_text")
                XCTAssertEqual(receipt.target, agent.bundleID)
                XCTAssertEqual(receipt.textByteLen, expectedTexts[offset].utf8.count)
                XCTAssertTrue(receipt.detail.contains("no_newline=true"))
                XCTAssertTrue(receipt.detail.contains("auto_submit=false"))

                // Must activate app -> focus input -> Cmd+V, and NEVER press Return
                XCTAssertEqual(callLog, ["activate:\(agent.bundleID)", "focus:\(agent.adapter)", "cmd_v"])
                XCTAssertEqual(pastedClipboardSnapshots, [expectedTexts[offset]])
                // Original multi-type clipboard must be restored after paste
                XCTAssertEqual(pasteboard.currentPlainText, "user-original-clip")
                XCTAssertEqual(pasteboard.items.first?[.rtf], Data("{\\rtf1 original}".utf8))
            }
        }

        // Custom prompt with trailing CRLF strips newlines, and if input focus fails, clipboard & keys remain untouched
        focusShouldSucceed = false
        callLog.removeAll()
        pastedClipboardSnapshots.removeAll()
        let changeCountBeforeFailedFocus = pasteboard.changeCount
        let rawFailedFocusReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"paste_text","text":"Custom engineering check\r\n","adapter":"antigravity","target_bundle_id":"com.google.antigravity","no_newline":true,"submit":false}"#,
            bindingId: "AG15",
            profileId: "p-ag"
        )
        let failedFocusReceipt = try XCTUnwrap(rawFailedFocusReceipt)
        XCTAssertEqual(failedFocusReceipt.outcome, "blocked")
        XCTAssertEqual(callLog, ["activate:com.google.antigravity", "focus:antigravity"])
        XCTAssertTrue(pastedClipboardSnapshots.isEmpty)
        XCTAssertEqual(pasteboard.changeCount, changeCountBeforeFailedFocus, "Failed input focus must not touch clipboard")
        XCTAssertEqual(pasteboard.currentPlainText, "user-original-clip")

        // External clipboard modification during restore window is preserved (not overwritten by stale restore)
        focusShouldSucceed = true
        var simulateExternalCopy = false
        executor.macroSleeper = { _ in
            if simulateExternalCopy {
                pasteboard.writePlainText("external-user-copy-during-window")
            }
        }
        simulateExternalCopy = true
        let rawExtReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"paste_text","text":"Continue\n","adapter":"codex","target_bundle_id":"com.openai.codex","no_newline":true,"submit":false}"#,
            bindingId: "AG15",
            profileId: "p-codex"
        )
        let extReceipt = try XCTUnwrap(rawExtReceipt)
        await executor.flushClipboardRestoreForTesting()
        XCTAssertEqual(extReceipt.outcome, "executed")
        XCTAssertEqual(pasteboard.currentPlainText, "external-user-copy-during-window", "External user copy during restore window must not be overwritten")

        await store.shutdown()
    }

    @MainActor func testSubmitControlStopsActiveDictationBeforeSendingReturnAndBlocksWhenUnconfirmed() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent("workbench.json")

        let pasteboard = InMemoryPasteboard(items: [[.string: Data("keep-me".utf8)]])
        let executor = LiveHostActionExecutor()
        executor.pasteboard = pasteboard
        executor.clipboardRestoreDelayNanoseconds = 1_000_000
        executor.submitSettleDelayNanoseconds = 1_000_000
        executor.macroAccessibilityChecker = { true }
        executor.macroSleeper = { _ in }

        var callLog: [String] = []
        var dictationStopResult: (Bool, String) = (true, "dictation_stopped:antigravity")

        executor.appActivator = { bundleId in
            callLog.append("activate:\(bundleId)")
            return true
        }
        executor.activeDictationStopper = { adapter in
            callLog.append("stop_dictation:\(adapter)")
            return dictationStopResult
        }
        executor.agentInputFocuser = { adapter in
            callLog.append("focus:\(adapter)")
            return (true, "已确认输入框焦点；未输入或发送文本")
        }
        executor.pasteShortcutPoster = {
            callLog.append("cmd_v:\(pasteboard.currentPlainText ?? "")")
            return true
        }
        executor.returnKeyPoster = {
            callLog.append("return")
            return true
        }

        let pttDriver = ControllableDictationDriver()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url, actionExecutor: executor, dictationDriver: pttDriver)
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        await store.setHostExecutionEnabled(true)

        let agProfile = try XCTUnwrap(store.hostProfiles.first { $0.name == "Antigravity" })
        let agIdx = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.id.text == agProfile.id.text })
        store.selectProfile(agIdx)
        await store.setHostActiveProfile(agProfile.id.text)

        // Bind AG12 (slot 12) to agent-submit and AG08 (slot 8) to agent-prompt-submit
        store.slot = 12
        await store.assign("agent-submit")
        store.slot = 8
        await store.assign("agent-prompt-submit")

        // 1. Explicit submit key stops active dictation FIRST, focuses input SECOND, and sends Return THIRD
        callLog.removeAll()
        let submitJSON = try XCTUnwrap(store.resolveInputActionForTesting(bindingId: "AG12"))
        let rawSubmitReceipt = await store.executeHostAction(actionJSON: submitJSON, bindingId: "AG12", profileId: agProfile.id.text)
        let submitReceipt = try XCTUnwrap(rawSubmitReceipt)
        XCTAssertEqual(submitReceipt.outcome, "executed")
        XCTAssertEqual(callLog, [
            "activate:com.google.antigravity",
            "stop_dictation:antigravity",
            "focus:antigravity",
            "return"
        ])

        // 2. Explicit submit-prompt stops active dictation -> focuses input -> pastes without newline -> sends Return -> restores clipboard
        callLog.removeAll()
        dictationStopResult = (true, "dictation_stopped:antigravity")
        let submitPromptJSON = try XCTUnwrap(store.resolveInputActionForTesting(bindingId: "AG08"))
        let rawSubmitPromptReceipt = await store.executeHostAction(actionJSON: submitPromptJSON, bindingId: "AG08", profileId: agProfile.id.text)
        let submitPromptReceipt = try XCTUnwrap(rawSubmitPromptReceipt)
        await executor.flushClipboardRestoreForTesting()
        XCTAssertEqual(submitPromptReceipt.outcome, "executed")
        XCTAssertEqual(callLog, [
            "activate:com.google.antigravity",
            "stop_dictation:antigravity",
            "focus:antigravity",
            "cmd_v:Continue",
            "return"
        ])
        XCTAssertEqual(pasteboard.currentPlainText, "keep-me")

        // 3. If active dictation cannot be confirmed stopped, both submit and submit-prompt are BLOCKED before focus/paste/Return
        callLog.removeAll()
        dictationStopResult = (false, "dictation_stop_unconfirmed:antigravity")
        let rawBlockedSubmit = await store.executeHostAction(actionJSON: submitJSON, bindingId: "AG12", profileId: agProfile.id.text)
        let blockedSubmit = try XCTUnwrap(rawBlockedSubmit)
        XCTAssertEqual(blockedSubmit.outcome, "blocked")
        XCTAssertTrue(blockedSubmit.detail.contains("dictation_stop_unconfirmed"))
        XCTAssertEqual(callLog, ["activate:com.google.antigravity", "stop_dictation:antigravity"])

        callLog.removeAll()
        let rawBlockedPromptSubmit = await store.executeHostAction(actionJSON: submitPromptJSON, bindingId: "AG08", profileId: agProfile.id.text)
        let blockedPromptSubmit = try XCTUnwrap(rawBlockedPromptSubmit)
        XCTAssertEqual(blockedPromptSubmit.outcome, "blocked")
        XCTAssertEqual(callLog, ["activate:com.google.antigravity", "stop_dictation:antigravity"])
        XCTAssertEqual(pasteboard.currentPlainText, "keep-me")

        // 4. Queue-level PushToTalk protection: if PushToTalk stop is unconfirmed, queued submit is blocked at the routing gate
        dictationStopResult = (true, "dictation_inactive:antigravity")
        pttDriver.stopSucceeded = false
        store.startHeldDictation(adapter: "antigravity", binding: "AG06", profile: agProfile.id.text, ingress: 1)
        for _ in 0..<20 where !store.pushToTalk.isActive {
            try? await Task.sleep(nanoseconds: 5_000_000)
        }
        callLog.removeAll()
        let state = try XCTUnwrap(store.routing)
        await withCheckedContinuation { continuation in
            store.enqueueHostAction(
                json: submitJSON,
                binding: "AG12",
                profile: agProfile.id.text,
                state: state,
                done: { continuation.resume() }
            )
        }
        XCTAssertFalse(store.pushToTalk.cleanupConfirmed)
        XCTAssertEqual(store.lastHostActionReceipt?.outcome, "blocked")
        XCTAssertTrue(store.lastHostActionReceipt?.detail.contains("dictation_cleanup_unconfirmed") == true)
        XCTAssertTrue(callLog.isEmpty, "Executor must not run when Push-to-Talk cleanup is unconfirmed")

        await store.shutdown()
    }

    @MainActor func testAccessibilityTrustRefreshesReactivelyOnMonitorAndForegroundChange() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        var trusted = false
        let store = Store(
            nativeOwnership: NativeOwnershipController(isolated: true),
            backend: MicroInputCore.MicroInput(),
            documentURL: dir.appendingPathComponent("workbench.json"),
            actionExecutor: RecordingHostActionExecutor()
        )
        store.accessibilityTrustChecker = { trusted }
        store.accessibilityPollNanoseconds = 10_000_000 // 10ms

        XCTAssertFalse(store.refreshAccessibilityTrust())
        XCTAssertFalse(store.accessibilityTrusted)
        XCTAssertNotNil(store.accessibilityMonitorTask, "Untrusted state must start background trust monitor")

        trusted = true
        for _ in 0..<20 where !store.accessibilityTrusted {
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTAssertTrue(store.accessibilityTrusted, "Store must publish accessibilityTrusted=true once TCC grants permission")
        XCTAssertNil(store.accessibilityMonitorTask, "Monitor must stop once permission is granted")

        trusted = false
        store.foregroundChanged(bundleID: Bundle.main.bundleIdentifier)
        XCTAssertFalse(store.accessibilityTrusted, "Foreground change to app itself must refresh accessibilityTrusted")

        await store.shutdown()
        XCTAssertNil(store.accessibilityMonitorTask)
    }

    @MainActor func testPrepareDesktopProfilesDoesNotClobberReadyStatusWhenPresetsExist() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-idempotent-presets-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = Store(
            nativeOwnership: NativeOwnershipController(isolated: true),
            backend: MicroInputCore.MicroInput(),
            documentURL: dir.appendingPathComponent("workbench.json"),
            actionExecutor: RecordingHostActionExecutor()
        )
        await store.loadWorkbench()
        await store.prepareDesktopProfiles()
        XCTAssertTrue(store.hasAllDesktopAgentPresets)

        store.status = "USB 透传层已就绪 · 宿主接管已开启（Antigravity）· 不写设备"
        await store.prepareDesktopProfiles()
        XCTAssertEqual(store.status, "USB 透传层已就绪 · 宿主接管已开启（Antigravity）· 不写设备",
                       "prepareDesktopProfiles must not re-mutate document or clobber host ready status when all DesktopAgent presets already exist")
        await store.shutdown()
    }

    func testDocumentInstanceLockRejectsDuplicateOwnerUntilReleased() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-instance-lock-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }

        let resolvedOverride = Store.defaultDocumentDirectoryURL(environment: ["MI_DOCUMENT_DIR": dir.path])
        XCTAssertEqual(resolvedOverride.standardizedFileURL.path, dir.standardizedFileURL.path)

        XCTAssertNil(DocumentInstanceLock.ownerPID(directory: dir))
        let first = try XCTUnwrap(DocumentInstanceLock.acquire(directory: dir, pid: 4242))
        XCTAssertEqual(DocumentInstanceLock.ownerPID(directory: dir), 4242)
        XCTAssertNil(DocumentInstanceLock.acquire(directory: dir, pid: 9999),
                     "Second MicroInput instance against the same document directory must be rejected before creating a second StatusMenu")

        first.release()
        XCTAssertNil(DocumentInstanceLock.ownerPID(directory: dir))
        let second = DocumentInstanceLock.acquire(directory: dir, pid: 9999)
        XCTAssertNotNil(second, "Releasing the lock must allow a subsequent instance to acquire it")
        XCTAssertEqual(DocumentInstanceLock.ownerPID(directory: dir), 9999)
        second?.release()
    }
}

@MainActor private final class InMemoryPasteboard: PasteboardAccessing {
    private(set) var changeCount: Int = 1
    private(set) var items: [[NSPasteboard.PasteboardType: Data]]

    init(items: [[NSPasteboard.PasteboardType: Data]] = []) {
        self.items = items
    }

    var currentPlainText: String? {
        guard let data = items.first?[.string] else { return nil }
        return String(data: data, encoding: .utf8)
    }

    func snapshotItems(maxBytes: Int) -> [[NSPasteboard.PasteboardType: Data]] {
        items
    }

    func writePlainText(_ text: String) {
        changeCount += 1
        items = [[.string: Data(text.utf8)]]
    }

    func restoreItems(_ restored: [[NSPasteboard.PasteboardType: Data]]) {
        changeCount += 1
        items = restored
    }
}

@MainActor private final class ControllableDictationDriver: DictationDriving, DictationRecording {
    var stopSucceeded = true
    var simulated: Bool { true }
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart {
        .started(self)
    }
    func stop() async -> (Bool, String) {
        (stopSucceeded, stopSucceeded ? "已确认听写停止；未发送消息" : "听写停止未确认")
    }
    func isStopped() async -> Bool {
        stopSucceeded
    }
}
