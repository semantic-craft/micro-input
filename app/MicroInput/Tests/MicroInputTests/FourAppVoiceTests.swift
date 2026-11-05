import XCTest
import MicroInputCore
@testable import MicroInput

@MainActor
private final class TestVoiceRecording: DictationRecording {
    var stopped = false
    func stop() async -> (Bool, String) {
        stopped = true
        return (true, "已确认原听写停止；未发送消息")
    }
    func isStopped() async -> Bool { stopped }
}

@MainActor
private final class TestVoiceDictationDriver: DictationDriving {
    let recording = TestVoiceRecording()
    var simulated: Bool { true }
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart {
        isHeld() ? .started(recording) : .blocked("按键已松开")
    }
}

@MainActor
final class FourAppVoiceTests: XCTestCase {
    private func makeStore(
        ownershipState: String = "released",
        driver: (any DictationDriving)? = nil
    ) -> (Store, MicroInputCore.MicroInput) {
        let ownership = NativeOwnershipController(isolated: true)
        if ownershipState == "native" {
            ownership.setDesiredOwner("native")
            let req = ownership.beginRequest(microDrained: true)
            ownership.receive(NativeOwnershipReply(
                version: 1,
                requestId: req.requestId,
                clientInstance: req.clientInstance,
                generation: req.generation,
                nativeInstance: UUID(),
                nativeGeneration: 1,
                state: "native",
                reason: "test",
                inputDrained: true,
                pttDrained: true,
                lightingDrained: true,
                capabilities: ["fenced-release-v1"]
            ), for: req)
        }
        let core = MicroInputCore.MicroInput()
        let store = Store(
            nativeOwnership: ownership,
            backend: core,
            dictationDriver: driver ?? TestVoiceDictationDriver()
        )
        store.routingValid = true
        store.hostExecutionEnabled = true
        return (store, core)
    }

    func testCindyNativeVoiceReuse() async {
        let (store, _) = makeStore(ownershipState: "native")
        let voice = store.fourAppVoice

        let cindySession = KeypadNavigationPlace.cindy(workspacePath: "/ws/cindy", sessionId: "cindy-sess-1")

        // 1. Cindy reuses native voice when native ownership is active
        let (started, startDetail) = await voice.startDictation(
            app: "cindy",
            session: cindySession,
            binding: "AG06",
            profile: "fixture-profile",
            store: store
        )
        XCTAssertTrue(started)
        XCTAssertTrue(startDetail.contains("Cindy 复用原生语音"))
        XCTAssertTrue(voice.dictationState.isRecording)

        // 2. Stop Cindy native voice
        let (stopped, stopDetail) = await voice.stopDictation(
            app: "cindy",
            session: cindySession,
            binding: "AG06",
            store: store
        )
        XCTAssertTrue(stopped)
        XCTAssertTrue(stopDetail.contains("已确认 Cindy 原生听写停止"))
        XCTAssertFalse(voice.dictationState.isRecording)
    }

    func testFourAppDictationStartStopAndTargetBinding() async {
        let (store, _) = makeStore(ownershipState: "released")
        let voice = store.fourAppVoice

        let testCases: [(app: String, validPlace: KeypadNavigationPlace, wrongPlace: KeypadNavigationPlace)] = [
            (
                "antigravity",
                .antigravity(conversationId: "ag-conv-1", workspacePath: "/ws/ag"),
                .zcode(sourceServer: "", remoteSession: "", workspace: "/ws/ag", taskId: "t1")
            ),
            (
                "cindy",
                .cindy(workspacePath: "/ws/cindy", sessionId: "sess-1"),
                .dsh(sessionId: "sess-1", workspaceId: "/ws", hostId: "h1")
            ),
            (
                "zcode",
                .zcode(sourceServer: "", remoteSession: "", workspace: "/ws/zcode", taskId: "zc-task-1"),
                .cindy(workspacePath: "/ws/zcode", sessionId: "zc-task-1")
            ),
            (
                "dsh-desktop",
                .dsh(sessionId: "dsh-sess-1", workspaceId: "/ws/dsh", hostId: "h1"),
                .antigravity(conversationId: "dsh-sess-1", workspacePath: "/ws/dsh")
            )
        ]

        for tc in testCases {
            // Mismatched target binding is rejected
            let (mismatched, _) = await voice.startDictation(
                app: tc.app,
                session: tc.wrongPlace,
                binding: "AG06",
                profile: "fixture",
                store: store
            )
            XCTAssertFalse(mismatched, "Expected mismatch rejection for \(tc.app)")

            // Matching target starts dictation
            let (started, startDetail) = await voice.startDictation(
                app: tc.app,
                session: tc.validPlace,
                binding: "AG06",
                profile: "fixture",
                store: store
            )
            XCTAssertTrue(started, "Failed to start dictation for \(tc.app): \(startDetail)")
            XCTAssertTrue(voice.dictationState.isRecording)

            // Double start while recording is rejected
            let (doubleStart, _) = await voice.startDictation(
                app: tc.app,
                session: tc.validPlace,
                binding: "AG06",
                profile: "fixture",
                store: store
            )
            XCTAssertFalse(doubleStart, "Double start should be rejected for \(tc.app)")

            // Stop with mismatched target is rejected
            let (mismatchedStop, _) = await voice.stopDictation(
                app: tc.app,
                session: tc.wrongPlace,
                binding: "AG06",
                store: store
            )
            XCTAssertFalse(mismatchedStop, "Mismatched stop should be rejected for \(tc.app)")

            // Matching stop confirms stopped
            let (stopped, stopDetail) = await voice.stopDictation(
                app: tc.app,
                session: tc.validPlace,
                binding: "AG06",
                store: store
            )
            XCTAssertTrue(stopped, "Failed to stop dictation for \(tc.app): \(stopDetail)")
            XCTAssertFalse(voice.dictationState.isRecording)
            XCTAssertTrue(stopDetail.contains("未发送消息"), "Dictation stop must not send messages")
        }
    }

    func testStopActiveDictationBeforeSubmit() async {
        let (store, _) = makeStore(ownershipState: "released")
        let voice = store.fourAppVoice

        let zcodePlace = KeypadNavigationPlace.zcode(sourceServer: "", remoteSession: "", workspace: "/ws", taskId: "t1")
        _ = await voice.startDictation(app: "zcode", session: zcodePlace, binding: "AG06", profile: "fixture", store: store)
        XCTAssertTrue(voice.dictationState.isRecording)

        // Submit-before-stop halts active dictation before submit proceeds
        let (stoppedBeforeSubmit, detail) = await voice.stopBeforeSubmit(app: "zcode", store: store)
        XCTAssertTrue(stoppedBeforeSubmit)
        XCTAssertTrue(detail.contains("提交前已先停止活跃听写") || detail.contains("dictation_inactive"))
        XCTAssertFalse(voice.dictationState.isRecording)
    }

    func testVoiceReplyToggleAndReadbackVerification() async {
        let (store, _) = makeStore(ownershipState: "released")
        let voice = store.fourAppVoice

        // 1. Cindy: supports voice reply, verifies readback
        XCTAssertFalse(store.cindySessions.voiceReplyEnabled)
        let (cindyToggled, cindyMode, _) = await voice.toggleVoiceReply(app: "cindy", store: store)
        XCTAssertTrue(cindyToggled)
        XCTAssertEqual(cindyMode, .supported(enabled: true))
        XCTAssertTrue(store.cindySessions.voiceReplyEnabled)

        let (cindyDisabled, cindyMode2, _) = await voice.setVoiceReply(app: "cindy", enabled: false, store: store)
        XCTAssertTrue(cindyDisabled)
        XCTAssertEqual(cindyMode2, .supported(enabled: false))
        XCTAssertFalse(store.cindySessions.voiceReplyEnabled)

        // 2. ZCode: supports voice reply, verifies readback
        XCTAssertFalse(store.zcodeSessions.voiceReplyEnabled)
        let (zcodeToggled, zcodeMode, _) = await voice.toggleVoiceReply(app: "zcode", store: store)
        XCTAssertTrue(zcodeToggled)
        XCTAssertEqual(zcodeMode, .supported(enabled: true))
        XCTAssertTrue(store.zcodeSessions.voiceReplyEnabled)

        // 3. DSH: supports voice reply, verifies readback
        XCTAssertFalse(store.dshSessions.voiceReplyEnabled)
        let (dshToggled, dshMode, _) = await voice.toggleVoiceReply(app: "dsh-desktop", store: store)
        XCTAssertTrue(dshToggled)
        XCTAssertEqual(dshMode, .supported(enabled: true))
        XCTAssertTrue(store.dshSessions.voiceReplyEnabled)

        // 4. Antigravity: unsupported model/interface
        // Crucial predicate: must return false (uncompleted) and NOT mask as disabled or success
        let (agToggled, agMode, agDetail) = await voice.toggleVoiceReply(app: "antigravity", store: store)
        XCTAssertFalse(agToggled, "Antigravity voice reply should fail closed as unsupported")
        XCTAssertFalse(agMode.isSupported)
        if case .unsupported(let app, let reason) = agMode {
            XCTAssertEqual(app, "antigravity")
            XCTAssertTrue(reason.contains("Antigravity 2.29.0"))
        } else {
            XCTFail("Antigravity voice reply must be unsupported")
        }
        XCTAssertTrue(agDetail.contains("模型/接口不支持语音回复，保持未完成缺口"))
    }

    func testExecuteHostActionVoiceReplyReceipts() async {
        let (store, _) = makeStore(ownershipState: "released")

        // 1. Cindy voice-reply action execution
        let cindyReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"cindy","command":"voice-reply"}"#,
            bindingId: "AG06",
            profileId: "cindy-profile"
        )
        XCTAssertNotNil(cindyReceipt)
        XCTAssertEqual(cindyReceipt?.actionType, "cindy_voice_reply")
        XCTAssertEqual(cindyReceipt?.outcome, "voiceReplyEnabled")

        // 2. ZCode voice-reply action execution
        let zcodeReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"zcode","command":"voice-reply"}"#,
            bindingId: "AG06",
            profileId: "zcode-profile"
        )
        XCTAssertNotNil(zcodeReceipt)
        XCTAssertEqual(zcodeReceipt?.actionType, "zcode_voice_reply")
        XCTAssertEqual(zcodeReceipt?.outcome, "voiceReplyEnabled")

        // 3. DSH voice-reply action execution
        let dshReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"voice-reply"}"#,
            bindingId: "AG06",
            profileId: "dsh-profile"
        )
        XCTAssertNotNil(dshReceipt)
        XCTAssertEqual(dshReceipt?.actionType, "dsh-desktop_voice_reply")
        XCTAssertEqual(dshReceipt?.outcome, "voiceReplyEnabled")

        // 4. Antigravity voice-reply action execution
        let agReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"antigravity","command":"voice-reply"}"#,
            bindingId: "AG06",
            profileId: "ag-profile"
        )
        XCTAssertNotNil(agReceipt)
        XCTAssertEqual(agReceipt?.actionType, "antigravity_voice_reply")
        XCTAssertEqual(agReceipt?.outcome, "unsupported")
        XCTAssertTrue(agReceipt?.detail.contains("模型/接口不支持语音回复，保持未完成缺口") == true)
    }
}
