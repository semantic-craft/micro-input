import XCTest
import MicroInputCore
@testable import MicroInput

@MainActor
private final class TestDictationRecording: DictationRecording {
    var stopped = false
    func stop() async -> (Bool, String) {
        stopped = true
        return (true, "已停止听写")
    }
    func isStopped() async -> Bool { stopped }
}

@MainActor
private final class TestDictationDriver: DictationDriving {
    let recording = TestDictationRecording()
    var simulated: Bool { true }
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart {
        isHeld() ? .started(recording) : .blocked("按键已松开")
    }
}

@MainActor
final class FourAppInteractionTests: XCTestCase {
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
            dictationDriver: driver ?? TestDictationDriver()
        )
        store.routingValid = true
        store.hostExecutionEnabled = true
        return (store, core)
    }

    private func position(_ angle: Double, _ distance: Double = 0.9) -> RadialPositionInfo {
        RadialPositionInfo(angleTurns: angle, distance: distance)
    }

    private func radialRoute(_ revision: UInt64 = 1) -> HostRoutingState {
        HostRoutingState(
            document_id: "doc",
            document_revision: revision,
            host_state_revision: 1,
            profile_id: "profile",
            profile_name: "Synthetic profile",
            execution_enabled: true,
            auto_follow_enabled: false,
            manual_override: false,
            command_execution_enabled: false,
            app_profile_links: [:],
            keys: []
        )
    }

    private func radialSession(slots: [RadialSlot]? = nil) -> RadialMenuSession {
        let defaultSlots = (0..<8).map { RadialSlot(label: "Slot \($0)", intent: .cheatSheet, unavailableReason: nil) }
        return RadialMenuSession(inputSession: 7, routing: radialRoute(), slots: slots ?? defaultSlots, selected: nil)
    }

    // MARK: - 1. Cancellation produces zero action

    func testRadialMenuSlowReturnToCenterCancelsZeroAction() {
        var state = RadialMenuState()
        state.setEnabled(true)
        state.setConfirmMode(.springBack)
        state.update(position(0, 0), makeSession: { nil })

        // Move to sector 0
        state.update(position(0, 0.9), makeSession: { self.radialSession() })
        XCTAssertEqual(state.session?.selected, 0)

        // Dwell > 500ms flick threshold
        Thread.sleep(forTimeInterval: 0.55)

        // Slow return to center cancels with zero action
        let returned = state.update(position(0, 0), makeSession: { nil })
        XCTAssertNil(returned, "Slow return to center must cancel to idle with zero action")
        XCTAssertNil(state.session)
        XCTAssertFalse(state.capturesConfirm)
    }

    func testRadialMenuCancelProducesZeroAction() {
        var state = RadialMenuState()
        state.setEnabled(true)
        state.update(position(0, 0), makeSession: { nil })
        state.update(position(0.25, 0.9), makeSession: { self.radialSession() })
        XCTAssertNotNil(state.session)

        state.cancel()
        XCTAssertNil(state.session)
        XCTAssertNil(state.confirm())
    }

    func testStopOnIdleOrUnselectedSessionProducesNoop() async {
        let (store, _) = makeStore()

        // 1. Cindy stop when no session running -> noop
        let cindyReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"cindy","command":"stop"}"#,
            bindingId: "AG15",
            profileId: "cindy-prof"
        )
        XCTAssertNotNil(cindyReceipt)
        XCTAssertEqual(cindyReceipt?.actionType, "cindy_stop")
        XCTAssertEqual(cindyReceipt?.outcome, "noop")
        XCTAssertTrue(cindyReceipt?.detail.contains("guard.stopTargetChanged") == true)

        // 2. Antigravity stop when no session running -> noop
        let agReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"antigravity","command":"stop"}"#,
            bindingId: "AG15",
            profileId: "ag-prof"
        )
        XCTAssertNotNil(agReceipt)
        XCTAssertEqual(agReceipt?.actionType, "antigravity_stop")
        XCTAssertEqual(agReceipt?.outcome, "noop")
        XCTAssertTrue(agReceipt?.detail.contains("guard.stopTargetChanged") == true)

        // 3. ZCode stop when no session running -> noop
        let zcodeReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"zcode","command":"stop"}"#,
            bindingId: "AG15",
            profileId: "zcode-prof"
        )
        XCTAssertNotNil(zcodeReceipt)
        XCTAssertEqual(zcodeReceipt?.actionType, "zcode_stop")
        XCTAssertEqual(zcodeReceipt?.outcome, "noop")
        XCTAssertTrue(zcodeReceipt?.detail.contains("guard.stopTargetChanged") == true)

        // 4. DSH stop when no session running -> noop
        let dshReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"dsh-desktop","command":"stop"}"#,
            bindingId: "AG15",
            profileId: "dsh-prof"
        )
        XCTAssertNotNil(dshReceipt)
        XCTAssertEqual(dshReceipt?.actionType, "dsh-desktop_stop")
        XCTAssertEqual(dshReceipt?.outcome, "noop")
        XCTAssertTrue(dshReceipt?.detail.contains("guard.stopTargetChanged") == true)
    }

    // MARK: - 2. 8-Way Radial Prompt, Stop & Spring-Back Confirmation

    func testRadialMenuSpringBackQuickFlickConfirms() {
        var state = RadialMenuState()
        state.setEnabled(true)
        state.setConfirmMode(.springBack)
        state.update(position(0, 0), makeSession: { nil })

        let stopSlot = RadialSlot(label: "Stop", intent: .stop(adapter: "cindy"), unavailableReason: nil)
        var slots = (0..<8).map { RadialSlot(label: "Slot \($0)", intent: .cheatSheet, unavailableReason: nil) }
        slots[6] = stopSlot

        // Deflect to sector 6
        state.update(position(0.75, 0.9), makeSession: { self.radialSession(slots: slots) })
        XCTAssertEqual(state.session?.selected, 6)

        // Quick return to center (< 500ms) confirms
        let flicked = state.update(position(0, 0), makeSession: { nil })
        XCTAssertNotNil(flicked)
        XCTAssertEqual(flicked?.1, .stop(adapter: "cindy"))
        XCTAssertNil(state.session)
    }

    func testRadialMenuEncoderPressConfirmationMode() {
        var state = RadialMenuState()
        state.setEnabled(true)
        state.setConfirmMode(.encoderPress)
        state.update(position(0, 0), makeSession: { nil })

        let promptSlot = RadialSlot(label: "Prompt", intent: .prompt(text: "Test prompt", adapter: "cindy", submit: false), unavailableReason: nil)
        var slots = (0..<8).map { RadialSlot(label: "Slot \($0)", intent: .cheatSheet, unavailableReason: nil) }
        slots[1] = promptSlot

        // Move to sector 1
        state.update(position(0.125, 0.9), makeSession: { self.radialSession(slots: slots) })
        XCTAssertEqual(state.session?.selected, 1)

        // Returning to center does NOT confirm in encoderPress mode
        let returned = state.update(position(0, 0), makeSession: { nil })
        XCTAssertNil(returned)

        // Re-deflect to sector 1
        state.update(position(0.125, 0.9), makeSession: { self.radialSession(slots: slots) })

        // Confirm via explicit encoder press
        let confirmed = state.confirm()
        XCTAssertNotNil(confirmed)
        XCTAssertEqual(confirmed?.1, .prompt(text: "Test prompt", adapter: "cindy", submit: false))
    }

    func testPaseoEngineeringSlotsContainStopAndPromptTemplates() {
        let slots = RadialSlotConfig.fourAppEngineeringSlots
        XCTAssertEqual(slots.count, 8)
        XCTAssertEqual(slots[6].kind, .stop)
        XCTAssertEqual(slots[6].label, "Stop · 停止")
        XCTAssertEqual(slots[0].kind, .navigateBack)
        XCTAssertEqual(slots[1].kind, .prompt)
        XCTAssertEqual(slots[2].kind, .prompt)
        XCTAssertEqual(slots[3].kind, .prompt)
        XCTAssertEqual(slots[4].kind, .navigateForward)
        XCTAssertEqual(slots[5].kind, .prompt)
        XCTAssertEqual(slots[7].kind, .prompt)
    }

    // MARK: - 3. Dictation: Stop Before Submit

    func testStopBeforeSubmitHaltsRecording() async {
        let (store, _) = makeStore(ownershipState: "released")
        let voice = store.fourAppVoice
        let cindySession = KeypadNavigationPlace.cindy(workspacePath: "/ws/cindy", sessionId: "cindy-sess-1")

        let (started, _) = await voice.startDictation(
            app: "cindy",
            session: cindySession,
            binding: "AG10",
            profile: "fixture-profile",
            store: store
        )
        XCTAssertTrue(started)
        XCTAssertTrue(voice.dictationState.isRecording)

        // Stop before submit
        let (stopped, reason) = await voice.stopBeforeSubmit(app: "cindy", store: store)
        XCTAssertTrue(stopped)
        XCTAssertTrue(reason.contains("提交前已先停止活跃听写") || reason.contains("已确认原听写停止"))
        XCTAssertFalse(voice.dictationState.isRecording)
    }

    // MARK: - 4. Target State Derivation Across Four Apps

    func testDeriveStateForCindy() {
        let (store, _) = makeStore()
        let controller = FourAppLightingController()

        // Default: idle
        XCTAssertEqual(controller.deriveState(for: "cindy", in: store), .idle)

        let addr = CindyTaskAddress(workspacePath: "/ws", sessionId: "sess1")
        let runningRow = CindyTaskRow(
            address: addr,
            title: "Session 1",
            phase: "running",
            attention: false,
            pinned: false
        )
        store.cindySessions.setSnapshotForTesting(rows: [runningRow], selected: addr)
        XCTAssertEqual(controller.deriveState(for: "cindy", in: store), .thinking)

        let waitingRow = CindyTaskRow(
            address: addr,
            title: "Session 1",
            phase: "needs-interaction",
            attention: true,
            pinned: false
        )
        store.cindySessions.setSnapshotForTesting(rows: [waitingRow], selected: addr)
        XCTAssertEqual(controller.deriveState(for: "cindy", in: store), .awaitingInput)

        let errorRow = CindyTaskRow(
            address: addr,
            title: "Session 1",
            phase: "error",
            attention: true,
            pinned: false
        )
        store.cindySessions.setSnapshotForTesting(rows: [errorRow], selected: addr)
        XCTAssertEqual(controller.deriveState(for: "cindy", in: store), .error)
    }

    func testDeriveStateForAntigravity() {
        let (store, _) = makeStore()
        let controller = FourAppLightingController()

        let key = AntigravitySessionKey(conversationId: "conv-1", workspacePath: "/ws")
        let row = AntigravitySessionRow(
            conversationId: key.conversationId,
            workspacePath: key.workspacePath,
            title: "AG Session",
            projectId: "p1",
            modelName: "gemini-pro",
            thinkingLevel: "high",
            phase: .running,
            lastEventType: .preInvocation,
            invocationNum: 1,
            stepIdx: 1,
            executionNum: 1,
            lastToolName: nil,
            terminationReason: nil,
            fullyIdle: false,
            transitionCount: 1,
            pendingInteraction: nil
        )
        store.antigravityStatus.setSnapshotForTesting(rows: [row], selected: key)
        XCTAssertEqual(controller.deriveState(for: "antigravity", in: store), .thinking)

        let waitingRow = AntigravitySessionRow(
            conversationId: key.conversationId,
            workspacePath: key.workspacePath,
            title: "AG Session",
            projectId: "p1",
            modelName: "gemini-pro",
            thinkingLevel: "high",
            phase: .running,
            lastEventType: .preInvocation,
            invocationNum: 1,
            stepIdx: 1,
            executionNum: 1,
            lastToolName: nil,
            terminationReason: nil,
            fullyIdle: false,
            transitionCount: 2,
            pendingInteraction: AntigravityPendingInteraction(
                interactionId: "pi-1",
                interactionType: .approval
            )
        )
        store.antigravityStatus.setSnapshotForTesting(rows: [waitingRow], selected: key)
        XCTAssertEqual(controller.deriveState(for: "antigravity", in: store), .awaitingInput)
    }

    func testDeriveStateForZCode() {
        let (store, _) = makeStore()
        let controller = FourAppLightingController()

        let addr = ZCodeTaskAddress(workspacePath: "/ws", taskId: "zsess")
        let runningRow = ZCodeTaskRow(
            address: addr,
            title: "ZCode Task",
            liveStatus: "running"
        )
        store.zcodeSessions.setSnapshotForTesting(rows: [runningRow], selected: addr)
        XCTAssertEqual(controller.deriveState(for: "zcode", in: store), .thinking)

        let waitingRow = ZCodeTaskRow(
            address: addr,
            title: "ZCode Task",
            liveStatus: "waiting"
        )
        store.zcodeSessions.setSnapshotForTesting(rows: [waitingRow], selected: addr)
        XCTAssertEqual(controller.deriveState(for: "zcode", in: store), .awaitingInput)
    }

    func testDeriveStateForDSH() {
        let (store, _) = makeStore()
        let controller = FourAppLightingController()

        let runningRow = DSHSessionRow(
            sessionId: "dsh-sess-1",
            workspaceId: "ws1",
            title: "DSH Task",
            active: true,
            running: true,
            pendingInput: 0
        )
        store.dshSessions.setSnapshotForTesting(rows: [runningRow], selected: "dsh-sess-1")
        XCTAssertEqual(controller.deriveState(for: "dsh-desktop", in: store), .thinking)

        let waitingRow = DSHSessionRow(
            sessionId: "dsh-sess-1",
            workspaceId: "ws1",
            title: "DSH Task",
            active: true,
            running: false,
            pendingInput: 1
        )
        store.dshSessions.setSnapshotForTesting(rows: [waitingRow], selected: "dsh-sess-1")
        XCTAssertEqual(controller.deriveState(for: "dsh-desktop", in: store), .awaitingInput)
    }

    // MARK: - 5. Dual-Zone & Per-Key Lighting Feedback

    func testDualZoneLightingFeedbackDerivation() {
        let controller = FourAppLightingController()

        // Idle: white backlight, green underglow
        let dzIdle = controller.deriveDualZoneFeedback(phase: .idle, dictationActive: false, nativeOwnershipActive: false)
        XCTAssertEqual(dzIdle.backlightColor, "#FFFFFF")
        XCTAssertEqual(dzIdle.underglowColor, "#00CCAA")

        // Awaiting input: amber backlight
        let dzAwaiting = controller.deriveDualZoneFeedback(phase: .awaitingInput, dictationActive: false, nativeOwnershipActive: false)
        XCTAssertEqual(dzAwaiting.backlightColor, "#FFAA00")

        // Thinking: blue backlight
        let dzThinking = controller.deriveDualZoneFeedback(phase: .thinking, dictationActive: false, nativeOwnershipActive: false)
        XCTAssertEqual(dzThinking.backlightColor, "#0088FF")

        // Dictation active: purple underglow
        let dzDictation = controller.deriveDualZoneFeedback(phase: .idle, dictationActive: true, nativeOwnershipActive: false)
        XCTAssertEqual(dzDictation.underglowColor, "#8800FF")

        // Native ownership active: underglow turns off (black)
        let dzNative = controller.deriveDualZoneFeedback(phase: .idle, dictationActive: false, nativeOwnershipActive: true)
        XCTAssertEqual(dzNative.underglowColor, "#000000")
        XCTAssertEqual(dzNative.underglowBrightness, 0.0)
    }

    func testPerKeySlotFeedbackDerivation() {
        let controller = FourAppLightingController()
        let slots = [
            FourAppSlot(slotIndex: 0, isPinned: false, place: .cindy(workspacePath: "/ws", sessionId: "s1"), title: "Task 1", app: "cindy", status: "running"),
            FourAppSlot(slotIndex: 1, isPinned: false, place: .cindy(workspacePath: "/ws", sessionId: "s2"), title: "Task 2", app: "cindy", status: "waiting"),
            FourAppSlot(slotIndex: 2, isPinned: false, place: nil, title: "", app: "cindy", status: "idle"),
            FourAppSlot(slotIndex: 3, isPinned: false, place: nil, title: "", app: "cindy", status: "idle"),
            FourAppSlot(slotIndex: 4, isPinned: false, place: nil, title: "", app: "cindy", status: "idle"),
            FourAppSlot(slotIndex: 5, isPinned: false, place: nil, title: "", app: "cindy", status: "idle")
        ]

        let pk = controller.derivePerKeyFeedback(slots: slots, dictationActive: false)
        XCTAssertEqual(pk.count, 8) // 6 slots + wide key (AG10) + encoder (ENC_CLK)
        XCTAssertEqual(pk[0].bindingId, "AG00")
        XCTAssertEqual(pk[0].phase, .thinking)
        XCTAssertEqual(pk[1].bindingId, "AG01")
        XCTAssertEqual(pk[1].phase, .awaitingInput)
        XCTAssertEqual(pk[2].bindingId, "AG02")
        XCTAssertEqual(pk[2].color, "#222222") // empty slot

        // Wide key (AG10)
        let wideKey = pk.first(where: { $0.bindingId == "AG10" })
        XCTAssertNotNil(wideKey)
        XCTAssertEqual(wideKey?.label, "听写就绪")

        // Encoder (ENC_CLK)
        let encoderKey = pk.first(where: { $0.bindingId == "ENC_CLK" })
        XCTAssertNotNil(encoderKey)
    }

    // MARK: - 6. Single Lighting Sender Invariant

    func testSingleSenderInvariantHandoverYieldsLighting() async {
        let (store, _) = makeStore(ownershipState: "released")
        let controller = store.fourAppLighting

        // When ownership allows execution
        XCTAssertTrue(controller.canSendLighting(in: store))

        // Native ownership transition to native -> yields execution
        store.nativeOwnership.setDesiredOwner("native")
        let req = store.nativeOwnership.beginRequest(microDrained: true)
        store.nativeOwnership.receive(NativeOwnershipReply(
            version: 1,
            requestId: req.requestId,
            clientInstance: req.clientInstance,
            generation: req.generation,
            nativeInstance: UUID(),
            nativeGeneration: 1,
            state: "native",
            reason: "handover",
            inputDrained: true,
            pttDrained: true,
            lightingDrained: true,
            capabilities: ["fenced-release-v1"]
        ), for: req)

        // Native ownership now active -> cannot send lighting
        XCTAssertFalse(controller.canSendLighting(in: store))

        // updateFeedback yields to native
        let result = await controller.updateFeedback(in: store)
        XCTAssertEqual(result, .yieldedToNative)
    }

    func testReleaseClearsLightingToken() async {
        let (store, _) = makeStore(ownershipState: "released")
        let controller = store.fourAppLighting

        await controller.release(in: store)
        XCTAssertEqual(controller.lastUpdateResult, .released)
        XCTAssertNil(controller.activeRuntimeToken)
    }

    // MARK: - 7. Voice Gap Explicitly Preserved

    func testVoiceReplyGapPreservedWithDetailedReason() async {
        let (store, _) = makeStore()
        let agReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"antigravity","command":"voice-reply"}"#,
            bindingId: "AG06",
            profileId: "ag-profile"
        )
        XCTAssertNotNil(agReceipt)
        XCTAssertEqual(agReceipt?.actionType, "antigravity_voice_reply")
        XCTAssertEqual(agReceipt?.outcome, "unsupported")
        XCTAssertTrue(agReceipt?.detail.contains("模型/接口不支持语音回复，保持未完成缺口") == true)
        XCTAssertFalse(agReceipt?.detail.contains("disabled") == true, "Must not be faked as simple disabled")
    }

    // MARK: - 8. Dial Scroll & Jump to Bottom Across All Four Apps

    func testDialScrollAcrossFourApps() async {
        defer { DesktopAgentControls.resetScrollHooks() }

        var wheels: [(Int32, Int)] = []
        var focusedAdapters: [String] = []

        DesktopAgentControls.scrollTrustChecker = { true }
        DesktopAgentControls.scrollRunningProcessID = { _ in 1234 }
        DesktopAgentControls.scrollWindowFinder = { _ in DialScrollWindow(windowNumber: 99, bounds: CGRect(x: 100, y: 100, width: 800, height: 600)) }
        DesktopAgentControls.scrollMouseLocation = { CGPoint(x: 10, y: 10) }
        DesktopAgentControls.scrollMouseWarper = { _ in }
        DesktopAgentControls.scrollWheelPoster = { delta, win in
            wheels.append((delta, win))
            return true
        }
        DesktopAgentControls.scrollSleeper = { _ in }
        DesktopAgentControls.scrollInputFocuser = { adapter, _ in
            focusedAdapters.append(adapter)
            return true
        }

        let adapters = ["antigravity", "cindy", "zcode", "dsh", "dsh-desktop"]

        for adapter in adapters {
            // 1. Scroll Up
            wheels.removeAll()
            let (okUp, detailUp) = await DesktopAgentControls.performScroll(adapter: adapter, command: "scroll-up")
            XCTAssertTrue(okUp, "\(adapter) scroll-up failed: \(detailUp)")
            XCTAssertTrue(detailUp.contains("\(adapter)/scroll-up"))
            XCTAssertEqual(wheels.map(\.0), [3], "\(adapter) scroll-up delta should be 3")

            // 2. Scroll Down
            wheels.removeAll()
            let (okDown, detailDown) = await DesktopAgentControls.performScroll(adapter: adapter, command: "scroll-down")
            XCTAssertTrue(okDown, "\(adapter) scroll-down failed: \(detailDown)")
            XCTAssertTrue(detailDown.contains("\(adapter)/scroll-down"))
            XCTAssertEqual(wheels.map(\.0), [-3], "\(adapter) scroll-down delta should be -3")

            // 3. Jump to Bottom
            wheels.removeAll()
            let (okBottom, detailBottom) = await DesktopAgentControls.performScroll(adapter: adapter, command: "scroll-to-bottom")
            XCTAssertTrue(okBottom, "\(adapter) scroll-to-bottom failed: \(detailBottom)")
            XCTAssertTrue(detailBottom.contains("lines=1000"))
            XCTAssertEqual(wheels.count, 10, "\(adapter) scroll-to-bottom should burst 10 times")
            XCTAssertTrue(wheels.allSatisfy { $0.0 == -100 && $0.1 == 99 })
            XCTAssertTrue(focusedAdapters.contains(adapter))
        }
    }

    // MARK: - 9. Hysteresis 0.75 Snap-Back, Escape Interception & Full 14-Key Hardware Lighting

    func testRadialMenuHysteresisRequires075PerimeterAnd250msSnapBackAndEscapeDismiss() {
        var state = RadialMenuState()
        state.setEnabled(true)
        state.setConfirmMode(.springBack)

        let makeSession: () -> RadialMenuSession? = {
            let slots = RadialSlotConfig.fourAppEngineeringSlots.map { cfg -> RadialSlot in
                RadialSlot(label: cfg.label, intent: .stop(adapter: "antigravity"), unavailableReason: nil, symbol: cfg.symbol, colorHex: cfg.colorHex)
            }
            return self.radialSession(slots: slots)
        }

        let t0 = Date()
        // 1. Center first
        _ = state.update(position(0, 0), now: t0, makeSession: makeSession)

        // 2. Deflect only to 0.60 (>= 0.50 openDistance, < 0.75 selectDistance): menu opens, not armed at perimeter
        _ = state.update(position(0.0, 0.60), now: t0.addingTimeInterval(0.05), makeSession: makeSession)
        XCTAssertNotNil(state.session)
        XCTAssertFalse(state.armedAtPerimeter)
        let shallowReturn = state.update(position(0, 0), now: t0.addingTimeInterval(0.10), makeSession: makeSession)
        XCTAssertNil(shallowReturn, "Deflection below 0.75 perimeter must not confirm on snap-back")

        // 3. Deflect to 0.85 (>= 0.75 perimeter), pass through 0.60 at t+0.15, reach center at t+0.30 (150ms <= 250ms since leaving perimeter) -> confirms!
        _ = state.update(position(0.0, 0.85), now: t0.addingTimeInterval(0.12), makeSession: makeSession)
        XCTAssertTrue(state.armedAtPerimeter)
        _ = state.update(position(0.0, 0.60), now: t0.addingTimeInterval(0.15), makeSession: makeSession)
        let fastSnap = state.update(position(0.0, 0.10), now: t0.addingTimeInterval(0.30), makeSession: makeSession)
        XCTAssertEqual(fastSnap?.1, .stop(adapter: "antigravity"))

        // 4. Reset to idle, deflect to 0.85, exit perimeter to 0.60 at t+1.05, linger 300ms (> 250ms) until t+1.35 -> cancels!
        _ = state.update(position(0, 0), now: t0.addingTimeInterval(1.00), makeSession: makeSession)
        _ = state.update(position(0.0, 0.85), now: t0.addingTimeInterval(1.02), makeSession: makeSession)
        _ = state.update(position(0.0, 0.60), now: t0.addingTimeInterval(1.05), makeSession: makeSession)
        let lingeredReturn = state.update(position(0.0, 0.10), now: t0.addingTimeInterval(1.36), makeSession: makeSession)
        XCTAssertNil(lingeredReturn, "Lingering >250ms after exiting 0.75 perimeter must cancel snap-back")
    }

    func testFullKeypadFeedbackAndCockpitActions() async {
        let (store, _) = makeStore(ownershipState: "released")
        await store.prepareDesktopProfiles()
        await store.applyFourAppCockpitPreset()

        // Verify 14-key full keypad feedback (AG00..AG12 + ENC_CLK)
        let slots = store.fourAppSlots.slots
        let fullKeys = store.fourAppLighting.deriveFullKeypadFeedback(
            slots: slots,
            dictationActive: true,
            activePhase: .awaitingInput,
            hasAttention: true
        )
        XCTAssertEqual(fullKeys.count, 14)
        XCTAssertEqual(fullKeys.first(where: { $0.bindingId == "AG07" })?.color, "#00FF66")
        XCTAssertEqual(fullKeys.first(where: { $0.bindingId == "AG08" })?.color, "#FF3B30")
        XCTAssertEqual(fullKeys.first(where: { $0.bindingId == "AG10" })?.color, "#FF00FF")
        XCTAssertEqual(fullKeys.first(where: { $0.bindingId == "AG11" })?.color, "#FF00FF")
        XCTAssertEqual(fullKeys.first(where: { $0.bindingId == "AG12" })?.color, "#0066FF")

        _ = await store.fourAppLighting.updateFeedback(in: store)
        XCTAssertNotNil(store.fourAppLighting.lastHardwareSyncJSON)

        // Verify approve / deny / new-or-stop host routing
        let approveReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"host","command":"approve"}"#,
            bindingId: "AG07",
            profileId: store.hostActiveProfileId ?? ""
        )
        XCTAssertNotNil(approveReceipt)
        let newOrStopReceipt = await store.executeHostAction(
            actionJSON: #"{"action_type":"agent_command","adapter":"host","command":"new-or-stop"}"#,
            bindingId: "AG09",
            profileId: store.hostActiveProfileId ?? ""
        )
        XCTAssertNotNil(newOrStopReceipt)
    }
}
