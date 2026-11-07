import XCTest
import AppKit
import Combine
import MicroInputCore
@testable import MicroInput

private func radialRoute(_ revision: UInt64 = 1) -> HostRoutingState {
    HostRoutingState(document_id: "doc", document_revision: revision, host_state_revision: 1, profile_id: "profile", profile_name: "Synthetic profile",
                     execution_enabled: true, auto_follow_enabled: false, manual_override: false,
                     command_execution_enabled: false, app_profile_links: [:], keys: [])
}
private func radialSession(intent: RadialIntent = .cheatSheet) -> RadialMenuSession {
    RadialMenuSession(inputSession: 7, routing: radialRoute(), slots: (0..<8).map { RadialSlot(label: "Slot \($0)", intent: intent, unavailableReason: nil) }, selected: nil)
}
private func position(_ angle: Double, _ distance: Double = 0.9) -> RadialPositionInfo { RadialPositionInfo(angleTurns: angle, distance: distance) }

final class RadialMenuStateTests: XCTestCase {
    func testEightScreenCoordinateSectorsAndThresholds() {
        for index in 0..<8 { XCTAssertEqual(RadialMenuState.sector(position(Double(index) / 8)), index) }
        XCTAssertEqual(RadialMenuState.sector(position(1)), 0)
        XCTAssertEqual(RadialMenuState.sector(position(0.08)), 1)
        for value in [position(.nan), position(-0.1), position(0, .infinity), position(0, 0.49)] {
            XCTAssertNil(RadialMenuState.sector(value))
        }
    }
    func testEnableRequiresCenterAndContinuousMotionUpdatesWithinOldSector() {
        var state = RadialMenuState()
        state.update(position(0), makeSession: { radialSession() })
        XCTAssertNil(state.session)
        state.setEnabled(true)
        state.update(position(0), makeSession: { radialSession() })
        XCTAssertNil(state.session)
        state.update(position(0, 0), makeSession: { nil })
        state.update(position(0), makeSession: { radialSession() })
        XCTAssertEqual(state.session?.selected, 0)
        state.update(position(0.08), makeSession: { XCTFail("must retain frozen session"); return nil })
        XCTAssertEqual(state.session?.selected, 1)
        state.update(position(0.08, 0.4), makeSession: { nil })
        XCTAssertNil(state.session?.selected)
        XCTAssertNil(state.confirm())
    }
    func testConfirmOnceThenCenterCancelProducesNoIntent() {
        var state = RadialMenuState(); state.setEnabled(true)
        state.update(position(0, 0), makeSession: { nil })
        state.update(position(0.25), makeSession: { radialSession() })
        XCTAssertEqual(state.confirm()?.1, .cheatSheet)
        XCTAssertNil(state.confirm()); XCTAssertTrue(state.capturesConfirm)
        state.update(position(0.5), makeSession: { radialSession() })
        XCTAssertNil(state.session)
        state.update(position(0, 0), makeSession: { nil })
        state.update(position(0.5), makeSession: { radialSession() })
        state.update(position(0, 0), makeSession: { nil })
        XCTAssertNil(state.confirm()); XCTAssertNil(state.session)
    }
    func testFrozenSessionSurvivesSectorChangeAndCancelNeedsCenter() {
        var state = RadialMenuState(); state.setEnabled(true)
        state.update(position(0, 0), makeSession: { nil })
        state.update(position(0), makeSession: { radialSession(intent: .openApp("first")) })
        state.update(position(0.125), makeSession: { radialSession(intent: .openApp("different")) })
        XCTAssertEqual(state.confirm()?.1, .openApp("first"))
        state.cancel(); state.update(position(0), makeSession: { radialSession() })
        XCTAssertNil(state.session)
    }
    func testDisabledSlotCannotConfirmAndDisabledStateDoesNotCaptureEncoder() {
        var state = RadialMenuState(); state.setEnabled(true)
        state.update(position(0, 0), makeSession: { nil })
        state.update(position(0), makeSession: {
            RadialMenuSession(inputSession: 7, routing: radialRoute(), slots: [RadialSlot(label: "Unavailable", intent: nil, unavailableReason: "missing")], selected: nil)
        })
        XCTAssertNil(state.confirm()); XCTAssertNotNil(state.session)
        state.setEnabled(false); XCTAssertFalse(state.capturesConfirm); XCTAssertNil(state.session)
    }
    func testOldCenterCannotReleaseQuarantineOrArmMenu() {
        var boundary = HostInputBoundary()
        boundary.advance(InputWatermark(session_id: 7, ingress_sequence: 10))
        func event(_ sequence: UInt64, _ distance: Double) -> InputEventInfo {
            InputEventInfo(sessionId: 7, sequence: sequence, ingressSequence: sequence, radialCenter: distance < 0.35, radialPosition: position(0, distance),
                timestampMs: 0, kind: .unknownNotification, logicalSwitch: nil, bindingId: nil, physicalControl: nil, keycapGroup: nil, actionRole: nil,
                lostCount: 0, verification: "verified-host-routing-0.6.3", actionEligible: false)
        }
        XCTAssertFalse(boundary.consume(event(9, 0), gateOpen: true).radial)
        XCTAssertFalse(boundary.consume(event(11, 0.9), gateOpen: true).radial)
        XCTAssertTrue(boundary.consume(event(12, 0), gateOpen: true).radial)
        XCTAssertTrue(boundary.consume(event(13, 0.9), gateOpen: true).radial)
    }
    @MainActor func testPanelDoesNotTakeFocusAndClampsOnSecondaryScreen() {
        let screen = NSRect(x: -800, y: -300, width: 380, height: 340)
        XCTAssertTrue(screen.contains(RadialMenuOverlay.position(size: NSSize(width: 500, height: 500), point: NSPoint(x: -1000, y: 10), screen: screen)))
        let panel = PassiveHostPanel(contentRect: .zero, styleMask: [.nonactivatingPanel], backing: .buffered, defer: false)
        XCTAssertFalse(panel.canBecomeKey); XCTAssertFalse(panel.canBecomeMain)
    }
}

#if MI_SYNTHETIC_BACKEND
@MainActor private func radialNext(_ stream: InputSubscription) async throws -> InputEventInfo {
    let value = await stream.nextEvent()
    return try XCTUnwrap(value)
}
@MainActor final class RadialMenuBridgeTests: XCTestCase {
    @MainActor struct Fixture {
        let store: Store
        let core: MicroInputCore.MicroInput
        let executor: RecordingHostActionExecutor
        let stream: InputSubscription
        let directory: URL
        let preferences: UserDefaults
        let suite: String
        func close() async {
            stream.close(); await store.shutdown(); preferences.removePersistentDomain(forName: suite)
            try? FileManager.default.removeItem(at: directory)
        }
        func feed(_ input: SyntheticInputCase, count: Int = 1) async throws {
            try await core.emitSyntheticInputCase(inputCase: input)
            for _ in 0..<count { let event = try await radialNext(stream); _ = store.consumeHostInput(event) }
        }
    }
    func fixture(dictation: (any DictationDriving)? = nil, executor override: (any HostActionExecuting)? = nil) async throws -> Fixture {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let suite = directory.appendingPathComponent("radial-preferences").path, executor = RecordingHostActionExecutor()
        let preferences = try XCTUnwrap(UserDefaults(suiteName: suite))
        let core = MicroInputCore.MicroInput.syntheticRadialDevice()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: directory.appendingPathComponent("workbench.json"), actionExecutor: override ?? executor, dictationDriver: dictation, radialPreferences: preferences)
        await store.loadWorkbench(); await store.installCodingAgentPresets(); await store.setHostExecutionEnabled(true)
        let qwen = try XCTUnwrap(store.hostProfiles.first { $0.name == "千问办公" })
        await store.setHostActiveProfile(qwen.id.text)
        try await core.setSyntheticDiscovery(state: .reconnectedDevice)
        await store.refresh(); await store.connect("synthetic-reconnected-interface-0"); await store.readIdentity(); await store.readDeviceConfig(); store.showLocalDraft()
        let old = store.hostInputTask; old?.cancel(); await old?.value; store.hostInputTask = nil
        let stream = try await core.subscribeVerifiedInput()
        store.readInputBoundary()
        XCTAssertEqual(store.radialReadiness, .ready, store.status)
        return Fixture(store: store, core: core, executor: executor, stream: stream, directory: directory, preferences: preferences, suite: suite)
    }
    func testActualBridgeKeepsContinuousPositionsAndOldFourWayWhenDisabled() async throws {
        let f = try await fixture()
        XCTAssertFalse(f.store.radialMenu.enabled)
        try await f.core.emitSyntheticInputCase(inputCase: .radialEast)
        let east = try await radialNext(f.stream)
        XCTAssertEqual(east.radialPosition?.angleTurns, 0); XCTAssertEqual(east.bindingId, "AG18")
        try await f.core.emitSyntheticInputCase(inputCase: .radialEast)
        let repeated = try await radialNext(f.stream)
        XCTAssertEqual(repeated.kind, .unknownNotification); XCTAssertNotNil(repeated.radialPosition)
        try await f.core.emitSyntheticInputCase(inputCase: .radialMiddle)
        let middle = try await radialNext(f.stream)
        XCTAssertEqual(middle.radialPosition?.distance, 0.4)
        try await f.core.emitSyntheticInputCase(inputCase: .radialCenter)
        let center = try await radialNext(f.stream)
        XCTAssertEqual(center.kind, .released); XCTAssertEqual(center.radialPosition?.distance, 0)
        XCTAssertTrue(f.executor.receipts.isEmpty)
        await f.close()
    }
    func testFullInputPathConfirmsOnlyOnceAndCenterCancels() async throws {
        let f = try await fixture()
        var receipts: [HostActionReceipt] = []
        let watch = f.store.$lastHostActionReceipt.sink { if let value = $0 { receipts.append(value) } }
        f.store.setRadialMenuEnabled(true)
        try await f.feed(.radialSouthEast); XCTAssertNil(f.store.radialMenu.session)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        XCTAssertEqual(f.store.radialMenu.session?.selected, 1)
        try await f.feed(.encoderClick, count: 2); await f.store.hostExecutionTask?.value
        XCTAssertEqual(receipts.filter { $0.actionType == "cheat_sheet" }.count, 1)
        try await f.feed(.encoderClick, count: 2); await f.store.hostExecutionTask?.value
        XCTAssertEqual(receipts.filter { $0.actionType == "cheat_sheet" }.count, 1)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast); try await f.feed(.radialCenter)
        XCTAssertNil(f.store.radialMenu.session)
        XCTAssertEqual(receipts.filter { $0.actionType == "cheat_sheet" }.count, 1)
        XCTAssertEqual(f.core.hostExecutionsPending(), 0)
        watch.cancel(); await f.close()
    }
    func testOrdinaryKeyCancelsMenuAndStillExecutesOriginalBinding() async throws {
        let f = try await fixture(); f.store.setRadialMenuEnabled(true)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        try await f.core.emitSyntheticBindingEvent(bindingId: "AG00", pressed: true)
        let event = try await radialNext(f.stream); _ = f.store.consumeHostInput(event)
        await f.store.hostExecutionTask?.value
        XCTAssertNil(f.store.radialMenu.session)
        XCTAssertEqual(f.executor.receipts.map(\.bindingId), ["AG00"])
        await f.close()
    }
    func testRouteChangeConsumesMenuConfirmationWithoutFallingBackToBinding() async throws {
        let f = try await fixture(); f.store.setRadialMenuEnabled(true)
        let replacementIndex = try XCTUnwrap(f.store.hostProfiles.firstIndex(where: { $0.name == "Antigravity" }))
        let replacement = f.store.hostProfiles[replacementIndex]
        f.store.selectProfile(replacementIndex); f.store.slot = 15
        let action = try XCTUnwrap(f.store.catalog.first(where: { $0.id == "app" }))
        let saved = await f.store.saveAction(action)
        XCTAssertTrue(saved, f.store.status)
        let restarted = f.store.hostInputTask
        restarted?.cancel(); await restarted?.value; f.store.hostInputTask = nil
        f.store.readInputBoundary()
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        XCTAssertNotNil(f.store.radialMenu.session)
        _ = try await f.core.setHostActiveProfile(profileId: replacement.id.text)
        try f.store.refreshRouting()
        XCTAssertNotNil(f.store.resolveHostAction("ENC_CLK"), "ordinary fallback must be executable for this regression")
        try await f.feed(.encoderClick, count: 2); await f.store.hostExecutionTask?.value
        XCTAssertNil(f.store.radialMenu.session)
        XCTAssertTrue(f.executor.receipts.isEmpty)
        XCTAssertNil(f.store.lastHostActionReceipt)
        await f.close()
    }
    func testReadinessRequiresSnapshotAndActualRadialMapping() async throws {
        let core = MicroInputCore.MicroInput.syntheticConfigWriter()
        try await core.setSyntheticDiscovery(state: .reconnectedDevice)
        _ = try await core.listDevices()
        try await core.connect(id: "synthetic-reconnected-interface-0")
        let watermark = try JSONDecoder().decode(InputWatermark.self, from: Data(core.hostInputWatermark().utf8))
        let session = try XCTUnwrap(watermark.session_id)
        XCTAssertEqual(core.radialMenuInputReadiness(sessionId: session), .stale)
        _ = try await core.readIdentity(); _ = try await core.readConfig()
        XCTAssertEqual(core.radialMenuInputReadiness(sessionId: session), .unverifiedMapping)
        await core.disconnect()
        XCTAssertEqual(core.radialMenuInputReadiness(sessionId: session), .disconnected)
        await core.shutdown()
    }
    func testCancelledPredecessorCannotConsumeReplacementRadialQueue() async throws {
        let executor = RadialGatedExecutor()
        let f = try await fixture(executor: executor)
        f.store.setRadialMenuEnabled(true)
        let state = try XCTUnwrap(f.store.routing)
        let action = #"{"action_type":"open_app","bundle_id":"test.app"}"#
        var sheets = 0
        let watch = f.store.$lastHostActionReceipt.sink { if $0?.actionType == "cheat_sheet" { sheets += 1 } }
        XCTAssertTrue(f.store.enqueueHostAction(json: action, binding: "A", profile: state.profile_id ?? "", state: state))
        for _ in 0..<100 where executor.pending["A"] == nil { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertNotNil(executor.pending["A"])
        let old = try XCTUnwrap(f.store.cancelHostExecution())
        XCTAssertTrue(f.store.enqueueHostAction(json: action, binding: "B", profile: state.profile_id ?? "", state: state))
        let replacement = try XCTUnwrap(f.store.hostExecutionTask)
        for _ in 0..<100 where executor.pending["B"] == nil { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertNotNil(executor.pending["B"])
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        try await f.feed(.encoderClick, count: 2)
        XCTAssertEqual(f.store.pendingHostActions.count, 1)
        executor.pending.removeValue(forKey: "A")?.resume()
        await old.value
        XCTAssertEqual(sheets, 0)
        XCTAssertEqual(f.store.pendingHostActions.count, 1)
        XCTAssertNotNil(f.store.hostExecutionTask)
        XCTAssertNil(f.store.lastHostActionReceipt)
        XCTAssertEqual(f.core.hostExecutionsPending(), 1)
        executor.pending.removeValue(forKey: "B")?.resume()
        await replacement.value
        XCTAssertEqual(sheets, 1)
        XCTAssertTrue(f.store.pendingHostActions.isEmpty)
        XCTAssertEqual(f.core.hostExecutionsPending(), 0)
        watch.cancel(); await f.close()
    }
    func testCancelledPredecessorCannotClearReplacementOverflow() async throws {
        let executor = RadialGatedExecutor()
        let f = try await fixture(executor: executor)
        let state = try XCTUnwrap(f.store.routing)
        let action = #"{"action_type":"open_app","bundle_id":"test.app"}"#
        XCTAssertTrue(f.store.enqueueHostAction(json: action, binding: "A", profile: "profile", state: state))
        for _ in 0..<100 where executor.pending["A"] == nil { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertNotNil(executor.pending["A"])
        let old = try XCTUnwrap(f.store.cancelHostExecution())
        XCTAssertTrue(f.store.enqueueHostAction(json: action, binding: "B", profile: "profile", state: state))
        let replacement = try XCTUnwrap(f.store.hostExecutionTask)
        for _ in 0..<100 where executor.pending["B"] == nil { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertNotNil(executor.pending["B"])
        for index in 0..<33 { f.store.enqueueHostAction(json: action, binding: "queued-\(index)", profile: "profile", state: state) }
        XCTAssertTrue(f.store.overflowDraining)
        executor.pending.removeValue(forKey: "A")?.resume()
        await old.value
        XCTAssertTrue(f.store.overflowDraining)
        XCTAssertNotNil(f.store.hostExecutionTask)
        executor.pending.removeValue(forKey: "B")?.resume()
        await replacement.value
        XCTAssertFalse(f.store.overflowDraining)
        XCTAssertTrue(f.store.pendingHostActions.isEmpty)
        await f.close()
    }
    func testCancellingOverflowOwnerReleasesItsGateAndRefreshesInputCutoff() async throws {
        let executor = RadialGatedExecutor()
        let f = try await fixture(executor: executor)
        let state = try XCTUnwrap(f.store.routing)
        let action = #"{"action_type":"open_app","bundle_id":"test.app"}"#
        XCTAssertTrue(f.store.enqueueHostAction(json: action, binding: "A", profile: "profile", state: state))
        for _ in 0..<100 where executor.pending["A"] == nil { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertNotNil(executor.pending["A"])
        for index in 0..<33 { f.store.enqueueHostAction(json: action, binding: "queued-\(index)", profile: "profile", state: state) }
        XCTAssertTrue(f.store.overflowDraining)
        try await f.core.emitSyntheticInputCase(inputCase: .radialEast)
        _ = try await radialNext(f.stream)
        let watermark = try JSONDecoder().decode(InputWatermark.self, from: Data(f.core.hostInputWatermark().utf8))
        XCTAssertGreaterThan(watermark.ingress_sequence, f.store.inputBoundary.cutoff)
        let old = try XCTUnwrap(f.store.cancelHostExecution())
        XCTAssertFalse(f.store.overflowDraining)
        XCTAssertTrue(f.store.dispatchOpen)
        XCTAssertEqual(f.store.inputBoundary.cutoff, watermark.ingress_sequence)
        XCTAssertTrue(f.store.enqueueHostAction(json: action, binding: "B", profile: "profile", state: state))
        let replacement = try XCTUnwrap(f.store.hostExecutionTask)
        for _ in 0..<100 where executor.pending["B"] == nil { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertNotNil(executor.pending["B"])
        executor.pending.removeValue(forKey: "A")?.resume()
        await old.value
        XCTAssertNotNil(f.store.hostExecutionTask)
        XCTAssertFalse(f.store.overflowDraining)
        executor.pending.removeValue(forKey: "B")?.resume()
        await replacement.value
        XCTAssertEqual(f.store.lastHostActionReceipt?.bindingId, "B")
        await f.close()
    }
    func testFiveSecondSelectionUsesConfirmationTimeForQueueAdmission() async throws {
        let f = try await fixture(); f.store.setRadialMenuEnabled(true)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        try await Task.sleep(for: .seconds(5))
        try await f.feed(.encoderClick, count: 2); await f.store.hostExecutionTask?.value
        XCTAssertEqual(f.store.lastHostActionReceipt?.actionType, "cheat_sheet")
        await f.close()
    }
    func testDisabledSlotConfirmationDoesNotRunEncoderBinding() async throws {
        let f = try await fixture(); f.store.setRadialMenuEnabled(true)
        try await f.feed(.radialCenter); try await f.feed(.radialEast)
        XCTAssertNil(f.store.radialMenu.session?.intent)
        try await f.feed(.encoderClick, count: 2); await f.store.hostExecutionTask?.value
        XCTAssertTrue(f.executor.receipts.isEmpty); XCTAssertNil(f.store.lastHostActionReceipt)
        XCTAssertNotNil(f.store.radialMenu.session)
        await f.close()
    }
    func testDictationReleaseStillStopsWhileMenuIsEnabled() async throws {
        let dictation = RadialDictationDriver()
        let f = try await fixture(dictation: dictation); f.store.setRadialMenuEnabled(true)
        try await f.core.emitSyntheticBindingEvent(bindingId: "AG06", pressed: true)
        let press = try await radialNext(f.stream); _ = f.store.consumeHostInput(press)
        for _ in 0..<100 where dictation.starts == 0 { try await Task.sleep(for: .milliseconds(1)) }
        XCTAssertEqual(dictation.starts, 1)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        XCTAssertNil(f.store.radialMenu.session)
        try await f.core.emitSyntheticBindingEvent(bindingId: "AG06", pressed: false)
        let release = try await radialNext(f.stream); _ = f.store.consumeHostInput(release)
        await f.store.pushToTalk.waitUntilStopped()
        XCTAssertEqual(dictation.stops, 1)
        XCTAssertTrue(f.store.pushToTalk.cleanupConfirmed)
        await f.close()
    }
    func testPendingClearGapAndSleepCancelWithoutActions() async throws {
        let f = try await fixture(); f.store.setRadialMenuEnabled(true)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        f.store.clearPendingActions(); XCTAssertNil(f.store.radialMenu.session)
        try await f.feed(.radialSouthEast); XCTAssertNil(f.store.radialMenu.session)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        f.store.systemWillSleep(); XCTAssertNil(f.store.radialMenu.session)
        XCTAssertTrue(f.executor.receipts.isEmpty); XCTAssertNil(f.store.lastHostActionReceipt)
        await f.close()
        let g = try await fixture(); g.store.setRadialMenuEnabled(true)
        try await g.feed(.radialCenter); try await g.feed(.radialSouthEast)
        let gap = InputEventInfo(sessionId: try XCTUnwrap(g.store.inputBoundary.session), sequence: 100, ingressSequence: 100,
            radialCenter: nil, radialPosition: nil, timestampMs: 0, kind: .inputStreamGap, logicalSwitch: nil, bindingId: nil,
            physicalControl: nil, keycapGroup: nil, actionRole: nil, lostCount: 1, verification: "test", actionEligible: false)
        XCTAssertFalse(g.store.consumeHostInput(gap)); XCTAssertNil(g.store.radialMenu.session)
        XCTAssertTrue(g.executor.receipts.isEmpty); XCTAssertTrue(g.store.inputNeedsResume)
        await g.close()
    }
    func testMissingCurrentProfileDisablesCheatSheetSlot() async throws {
        let f = try await fixture()
        let route = try XCTUnwrap(f.store.routing)
        f.store.routing = HostRoutingState(document_id: route.document_id, document_revision: route.document_revision,
            host_state_revision: route.host_state_revision, profile_id: "missing", profile_name: nil,
            execution_enabled: true, auto_follow_enabled: false, manual_override: false,
            command_execution_enabled: false, app_profile_links: [:], keys: [])
        let slot = f.store.radialSlots()[1]
        XCTAssertNil(slot.intent); XCTAssertEqual(slot.unavailableReason, "当前配置不可用")
        await f.close()
    }
    func testReadinessAndQueuedIntentRejectSessionChangesAndDrain() async throws {
        let f = try await fixture(); f.store.setRadialMenuEnabled(true)
        let session = try XCTUnwrap(f.store.inputBoundary.session)
        XCTAssertEqual(f.core.radialMenuInputReadiness(sessionId: session + 1), .disconnected)
        let drain = try f.core.beginDeviceDrain()
        XCTAssertEqual(f.core.radialMenuInputReadiness(sessionId: session), .paused)
        f.core.abandonDeviceDrain(token: drain)
        XCTAssertEqual(f.core.radialMenuInputReadiness(sessionId: session), .ready)
        try await f.feed(.radialCenter); try await f.feed(.radialSouthEast)
        let frozen = try XCTUnwrap(f.store.radialMenu.session)
        await f.store.setHostExecutionEnabled(false)
        f.store.enqueueRadialIntent(.cheatSheet, session: frozen, ingress: UInt64.max)
        await f.store.hostExecutionTask?.value
        XCTAssertNil(f.store.lastHostActionReceipt)
        XCTAssertNil(f.store.radialMenu.session)
        await f.close()
    }

    func testConfigurablePaseoRadialPromptAndSubmitSlotsDispatchToActiveDesktopAgents() async throws {
        let f = try await fixture()
        f.store.setRadialMenuEnabled(true)
        f.store.applyPaseoRadialPreset()

        // Seed cross-agent navigation history so Back (slot 0) and Forward (slot 4) are testable
        _ = await f.store.executeHostAction(
            actionJSON: #"{"action_type":"open_app","bundle_id":"com.google.antigravity"}"#,
            bindingId: "AG00",
            profileId: "p-seed"
        )
        _ = await f.store.executeHostAction(
            actionJSON: #"{"action_type":"open_app","bundle_id":"cn.qwenwork.desktop.mac"}"#,
            bindingId: "AG00",
            profileId: "p-seed"
        )

        for (agentName, expectedAdapter, expectedBundle) in [
            ("Antigravity", "antigravity", "com.google.antigravity"),
            ("千问办公", "qwenwork", "cn.qwenwork.desktop.mac"),
            ("Cindy", "cindy", "com.xd.cindy"),
            ("Codex", "codex", "com.openai.codex")
        ] {
            let profile = try XCTUnwrap(f.store.hostProfiles.first { $0.name == agentName })
            await f.store.setHostActiveProfile(profile.id.text)
            let slots = f.store.radialSlots()
            XCTAssertEqual(slots.count, 8)
            XCTAssertEqual(slots[1].intent, .prompt(text: "Run the tests and fix any failures", adapter: expectedAdapter, submit: false))
            XCTAssertEqual(slots[2].intent, .prompt(text: "Continue", adapter: expectedAdapter, submit: false))
            XCTAssertEqual(slots[3].intent, .prompt(text: "Commit this work", adapter: expectedAdapter, submit: false))
            XCTAssertEqual(slots[5].intent, .prompt(text: "Review your diff for bugs", adapter: expectedAdapter, submit: false))
            XCTAssertEqual(slots[6].intent, .submit(adapter: expectedAdapter))
            XCTAssertEqual(slots[7].intent, .prompt(text: "Summarize where things stand and what's left", adapter: expectedAdapter, submit: false))

            // Confirm slot 1 (SouthEast -> Test prompt, paste-only no_newline=true)
            try await f.feed(.radialCenter)
            try await f.feed(.radialSouthEast)
            try await f.feed(.encoderClick, count: 2)
            await f.store.hostExecutionTask?.value

            let receipt = try XCTUnwrap(f.store.lastHostActionReceipt)
            XCTAssertEqual(receipt.actionType, "paste_text")
            XCTAssertEqual(receipt.target, expectedBundle)
            XCTAssertEqual(receipt.textByteLen, "Run the tests and fix any failures".utf8.count)
            XCTAssertTrue(receipt.detail.contains("no_newline=true"))
            XCTAssertFalse(receipt.detail.contains("auto_submit=true"))
        }

        // Custom per-slot override with explicit submit=true on slot 1
        f.store.setRadialSlotConfig(
            at: 1,
            .prompt(label: "Custom + Submit", text: "Fix lint warnings\r\n", adapter: "codex", submit: true)
        )
        try await f.feed(.radialCenter)
        try await f.feed(.radialSouthEast)
        try await f.feed(.encoderClick, count: 2)
        await f.store.hostExecutionTask?.value

        let explicitSubmitReceipt = try XCTUnwrap(f.store.lastHostActionReceipt)
        XCTAssertEqual(explicitSubmitReceipt.actionType, "paste_text")
        XCTAssertEqual(explicitSubmitReceipt.target, "com.openai.codex")
        XCTAssertEqual(explicitSubmitReceipt.textByteLen, "Fix lint warnings".utf8.count)
        XCTAssertTrue(explicitSubmitReceipt.detail.contains("auto_submit=true"))

        await f.close()
    }
}
@MainActor private final class RadialDictationDriver: DictationDriving, DictationRecording {
    var starts = 0, stops = 0
    var simulated: Bool { true }
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart { starts += 1; return .started(self) }
    func stop() async -> (Bool, String) { stops += 1; return (true, "synthetic stopped") }
    func isStopped() async -> Bool { stops > 0 }
}
#endif

#if MI_SYNTHETIC_BACKEND
@MainActor private final class RadialGatedExecutor: HostActionExecuting {
    var pending: [String: CheckedContinuation<Void, Never>] = [:]
    func execute(actionJSON: String, bindingId: String, profileId: String) async -> HostActionReceipt {
        await withCheckedContinuation { pending[bindingId] = $0 }
        return HostActionReceipt(timestampMs: 0, profileId: profileId, bindingId: bindingId, actionType: "open_app",
            target: "test.app", textByteLen: nil, outcome: "simulated", detail: "released")
    }
}
#endif
