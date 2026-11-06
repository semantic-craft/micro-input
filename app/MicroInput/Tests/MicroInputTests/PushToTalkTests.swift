import XCTest
import ApplicationServices
@testable import MicroInput

@MainActor private final class TestRecording: DictationRecording {
    var stops = 0
    var succeeds = true
    var externallyStopped = false
    var holdStop = false
    var stopContinuation: CheckedContinuation<Void, Never>?
    var holdChecks = false
    var checks: [CheckedContinuation<Void, Never>] = []
    func stop() async -> (Bool, String) {
        stops += 1
        if holdStop { await withCheckedContinuation { stopContinuation = $0 } }
        return (succeeds, "stop")
    }
    func isStopped() async -> Bool {
        if holdChecks { await withCheckedContinuation { checks.append($0) } }
        return externallyStopped
    }
}
@MainActor private final class TestDictationDriver: DictationDriving {
    var recording = TestRecording()
    var starts: [String] = []
    var block = false
    var uncertain = false
    var holdStart = false
    var continuation: CheckedContinuation<Void, Never>?
    func start(adapter: String, isHeld: @escaping () -> Bool) async -> DictationStart {
        starts.append(adapter)
        if holdStart { await withCheckedContinuation { continuation = $0 } }
        if block { return .blocked("start rejected") }
        return uncertain ? .uncertain(recording, "unknown") : .started(recording)
    }
}

final class PushToTalkTests: XCTestCase {
    @MainActor private func until(_ condition: () -> Bool) async {
        for _ in 0..<500 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTFail("Condition did not become true")
    }
    @MainActor func testReleaseStopsOriginalTargetOnceAndIgnoresOtherBindings() async {
        let driver = TestDictationDriver()
        let held = PushToTalkController(driver: driver)
        var receipts: [String] = [], finished = 0
        XCTAssertTrue(held.press(binding: "AG10", adapter: "cindy", report: { action, _, _ in receipts.append(action) }, finished: { finished += 1 }))
        XCTAssertFalse(held.press(binding: "AG10", adapter: "codex", report: { _, _, _ in }, finished: {}))
        await until { receipts.count == 1 }
        held.release(binding: "AG11")
        XCTAssertEqual(driver.recording.stops, 0)
        held.release(binding: "AG10"); held.release(binding: "AG10")
        await held.waitUntilStopped()
        XCTAssertEqual(driver.starts, ["cindy"])
        XCTAssertEqual(driver.recording.stops, 1)
        XCTAssertEqual(receipts, ["dictation_start", "dictation_stop"])
        XCTAssertEqual(finished, 1)
    }
    @MainActor func testReleaseDuringStartStopsLateRecordingAndDoesNotReplay() async {
        let driver = TestDictationDriver(); driver.holdStart = true
        let held = PushToTalkController(driver: driver)
        var finished = 0
        held.press(binding: "AG06", adapter: "codex", report: { _, _, _ in }, finished: { finished += 1 })
        await until { driver.continuation != nil }
        held.release(binding: "AG06"); held.cancel()
        driver.continuation?.resume(); driver.continuation = nil
        await held.waitUntilStopped()
        XCTAssertEqual(driver.recording.stops, 1)
        XCTAssertEqual(finished, 1)
    }
    @MainActor func testFailedStartNeverTogglesOnRelease() async {
        let driver = TestDictationDriver(); driver.block = true
        let held = PushToTalkController(driver: driver)
        held.press(binding: "AG06", adapter: "codex", report: { _, _, _ in }, finished: {})
        await held.waitUntilStopped()
        held.release(binding: "AG06"); held.cancel()
        XCTAssertEqual(driver.recording.stops, 0)
        XCTAssertTrue(held.cleanupConfirmed)
    }
    @MainActor func testUncertainStartCleansOriginalRecordingAndFailedStopBlocksNewGesture() async {
        let driver = TestDictationDriver(); driver.uncertain = true; driver.recording.succeeds = false
        let held = PushToTalkController(driver: driver)
        held.press(binding: "AG06", adapter: "codex", report: { _, _, _ in }, finished: {})
        await held.waitUntilStopped()
        XCTAssertEqual(driver.recording.stops, 1)
        XCTAssertFalse(held.cleanupConfirmed)
        XCTAssertFalse(held.press(binding: "AG06", adapter: "cindy", report: { _, _, _ in }, finished: {}))
        let initiallyStopped = await held.verifyStopped(); XCTAssertFalse(initiallyStopped)
        driver.recording.externallyStopped = true
        let finallyStopped = await held.verifyStopped(); XCTAssertTrue(finallyStopped)
        XCTAssertEqual(driver.recording.stops, 1, "Recovery reads state without pressing again")
    }
    @MainActor func testCancellationAndMaximumHoldStopOnce() async {
        let driver = TestDictationDriver()
        let held = PushToTalkController(driver: driver, maximumHold: 10_000_000)
        held.press(binding: "AG06", adapter: "cindy", report: { _, _, _ in }, finished: {})
        await held.waitUntilStopped()
        XCTAssertEqual(driver.recording.stops, 1)
        held.cancel(); XCTAssertEqual(driver.recording.stops, 1)
    }
}

import MicroInputCore

extension PushToTalkTests {
    @MainActor private func fixture() async throws -> (Store, TestDictationDriver, URL) {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        let driver = TestDictationDriver()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: dir.appendingPathComponent("draft.json"),
            actionExecutor: RecordingHostActionExecutor(), dictationDriver: driver)
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        let profile = try XCTUnwrap(store.hostProfiles.first { $0.name == "千问办公" })
        await store.setHostActiveProfile(profile.id.text); await store.setHostExecutionEnabled(true)
        store.inputBoundary.advance(InputWatermark(session_id: 1, ingress_sequence: 0))
        return (store, driver, dir)
    }
    private func event(_ sequence: UInt64, _ kind: InputKind, session: UInt64 = 1) -> InputEventInfo {
        InputEventInfo(sessionId: session, sequence: sequence, ingressSequence: sequence, radialCenter: nil, radialPosition: nil,
            timestampMs: 0, kind: kind, logicalSwitch: "logical-6", bindingId: "AG06", physicalControl: nil,
            keycapGroup: nil, actionRole: "momentary_key", lostCount: 0, verification: "test", actionEligible: kind == .pressed)
    }
    @MainActor func testQueuedPressReleasedBeforeExecutionDoesNotStart() async throws {
        let (store, driver, dir) = try await fixture()
        defer { try? FileManager.default.removeItem(at: dir) }
        store.consumeHostInput(event(1, .pressed)); store.consumeHostInput(event(2, .released))
        await store.hostExecutionTask?.value
        XCTAssertTrue(driver.starts.isEmpty)
        XCTAssertEqual(store.inFlightHostActions, 0)
        await store.shutdown()
    }
    @MainActor func testInputReleaseBypassesClosedGateAndRejectsForeignSession() async throws {
        let (store, driver, dir) = try await fixture()
        defer { try? FileManager.default.removeItem(at: dir) }
        store.consumeHostInput(event(1, .pressed))
        await until { !driver.starts.isEmpty }
        XCTAssertEqual(store.inFlightHostActions, 1)
        store.mutationDraining = true
        store.consumeHostInput(event(2, .released, session: 2))
        XCTAssertEqual(driver.recording.stops, 0)
        store.consumeHostInput(event(3, .released))
        await store.pushToTalk.waitUntilStopped()
        XCTAssertEqual(driver.recording.stops, 1)
        XCTAssertEqual(store.inFlightHostActions, 0)
        store.mutationDraining = false
        await store.shutdown()
    }
    @MainActor func testLifecycleBoundariesStopHeldRecordingAndReleaseLease() async throws {
        for cause in ["profile", "document", "pause", "sleep", "disconnect", "gap", "overflow", "foreground", "shutdown"] {
            let (store, driver, dir) = try await fixture()
            defer { try? FileManager.default.removeItem(at: dir) }
            store.consumeHostInput(event(1, .pressed))
            await until { !driver.starts.isEmpty }
            switch cause {
            case "profile": await store.setHostActiveProfile(store.hostProfiles.first { $0.name == "Codex" }?.id.text)
            case "document": await store.createProfile(name: "User profile")
            case "pause": await store.setHostExecutionEnabled(false)
            case "sleep": store.systemWillSleep()
            case "disconnect": await store.suspend()
            case "gap": XCTAssertFalse(store.consumeHostInput(event(2, .inputStreamGap)))
            case "foreground": store.foregroundChanged(bundleID: "test.other")
            case "overflow":
                let state = try XCTUnwrap(store.routing)
                for index in 0..<33 {
                    store.enqueueHostAction(json: "{}", binding: "test-\(index)", profile: state.profile_id ?? "", state: state)
                }
            default: await store.shutdown()
            }
            await store.pushToTalk.waitUntilStopped()
            XCTAssertEqual(driver.recording.stops, 1, cause)
            XCTAssertEqual(store.inFlightHostActions, 0, cause)
            await store.shutdown()
        }
    }
}

#if MI_SYNTHETIC_BACKEND
extension PushToTalkTests {
    @MainActor func testMergedWideKeyStopsOnlyAfterLastSwitchReleases() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        let driver = TestDictationDriver()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: dir.appendingPathComponent("draft.json"),
            actionExecutor: RecordingHostActionExecutor(), dictationDriver: driver)
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        let index = try XCTUnwrap(store.snapshot?.profiles.firstIndex { $0.name == "千问办公" })
        store.selectProfile(index); store.slot = 10
        let saved = await store.saveAction(CatalogAction(id: "wide-talk", name: "按住说话", category: "Agent", detail: "test",
            payload: ["type": .string("agent_command"), "adapter": .string("cindy"), "command": .string("push-to-talk")]))
        XCTAssertTrue(saved, store.status)
        await store.setHostActiveProfile(store.profile?.id.text); await store.setHostExecutionEnabled(true)
        try await core.setSyntheticDiscovery(state: .reconnectedDevice); try await core.setSyntheticConfig(scenario: 1)
        await store.refresh(); store.selectDevice("synthetic-reconnected-interface-0")
        await store.connect("synthetic-reconnected-interface-0"); await store.readIdentity(); await store.readDeviceConfig()
        await until { store.inputBoundary.session != nil }
        try await core.emitSyntheticBindingEvent(bindingId: "AG10", pressed: true)
        try await core.emitSyntheticBindingEvent(bindingId: "AG11", pressed: true)
        await until { driver.starts.count == 1 }
        try await core.emitSyntheticBindingEvent(bindingId: "AG10", pressed: false)
        try await Task.sleep(nanoseconds: 25_000_000)
        XCTAssertEqual(driver.recording.stops, 0)
        try await core.emitSyntheticBindingEvent(bindingId: "AG11", pressed: false)
        await until { driver.recording.stops == 1 }
        XCTAssertEqual(driver.starts, ["cindy"])
        await store.shutdown()
    }
    @MainActor func testUnknownStopPreventsDeviceApplyAfterLeaseDrains() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let core = MicroInputCore.MicroInput.syntheticConfigWriter()
        let driver = TestDictationDriver(); driver.recording.succeeds = false
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: dir.appendingPathComponent("draft.json"),
            actionExecutor: RecordingHostActionExecutor(), dictationDriver: driver)
        await store.loadWorkbench()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        await store.refresh(); store.selectDevice("synthetic-interface-0"); await store.connect("synthetic-interface-0")
        await store.readIdentity(); await store.readDeviceConfig(); await store.createDraftFromSnapshot()
        await store.setHostExecutionEnabled(true)
        await store.assign("device-key"); await store.previewApply(); await store.prepareApplyBackup()
        let token = try XCTUnwrap(store.preparedApplyBackup?.token, store.status)
        store.startHeldDictation(adapter: "cindy", binding: "AG06", profile: store.hostActiveProfileId ?? "")
        await until { !driver.starts.isEmpty }
        let before = await core.syntheticRequests().count
        await store.performDeviceOperation(token: token)
        XCTAssertEqual(driver.recording.stops, 1)
        XCTAssertFalse(store.pushToTalk.cleanupConfirmed)
        let after = await core.syntheticRequests().count
        XCTAssertEqual(after, before, "Unknown dictation cleanup must not permit a device write")
        XCTAssertTrue(store.status.contains("听写"), store.status)
        XCTAssertEqual(store.operationResult?.phase, "NotSent")
        XCTAssertFalse(store.hostOperationPaused, "A rejected write must not leave the routing pause stuck")
        driver.recording.externallyStopped = true
        await store.verifyDictationStopped()
        XCTAssertFalse(store.dictationNeedsAttention)
        XCTAssertTrue(store.dispatchOpen)
        await store.shutdown()
    }
}
#endif

extension PushToTalkTests {
    @MainActor func testStartDeadlineReleasesWaitersButBlocksUntilLateTargetIsCleaned() async {
        let driver = TestDictationDriver(); driver.holdStart = true
        let held = PushToTalkController(driver: driver, operationTimeout: 10_000_000)
        var completed = false
        held.press(binding: "AG06", adapter: "cindy", report: { _, _, _ in }, finished: { completed = true })
        await held.waitUntilStopped()
        XCTAssertTrue(completed, "The start driver has not returned, but lifecycle waiters must be released")
        XCTAssertNotNil(driver.continuation)
        XCTAssertFalse(held.cleanupConfirmed)
        XCTAssertFalse(held.press(binding: "AG06", adapter: "codex", report: { _, _, _ in }, finished: {}))
        driver.continuation?.resume(); driver.continuation = nil
        await until { held.cleanupConfirmed }
        XCTAssertEqual(driver.recording.stops, 1)
    }
    @MainActor func testStopDeadlineLeavesUnknownAndNeverRetriesToggle() async {
        let driver = TestDictationDriver(); driver.recording.holdStop = true
        let held = PushToTalkController(driver: driver, operationTimeout: 10_000_000)
        held.press(binding: "AG06", adapter: "cindy", report: { _, _, _ in }, finished: {})
        await until { !driver.starts.isEmpty }
        held.cancel(); await held.waitUntilStopped()
        XCTAssertFalse(held.cleanupConfirmed)
        XCTAssertEqual(driver.recording.stops, 1)
        driver.recording.stopContinuation?.resume(); driver.recording.stopContinuation = nil
        driver.recording.externallyStopped = true
        let confirmed = await held.verifyStopped()
        XCTAssertTrue(confirmed)
        XCTAssertEqual(driver.recording.stops, 1)
    }
    @MainActor func testStaleReleaseCannotEndNewGesture() async {
        let held = PushToTalkController(driver: TestDictationDriver())
        held.press(binding: "AG06", adapter: "cindy", ingress: 20, report: { _, _, _ in }, finished: {})
        held.release(binding: "AG06", ingress: 19)
        XCTAssertTrue(held.isActive)
        held.release(binding: "AG06", ingress: 21)
        await held.waitUntilStopped()
    }
    @MainActor func testQueuedUIToggleRechecksRouteAfterAwaitingStopAndRejectsUnknownCleanup() async throws {
        for cause in ["profile", "unknown", "cancel"] {
            let (store, driver, dir) = try await fixture()
            defer { try? FileManager.default.removeItem(at: dir) }
            let executor = try XCTUnwrap(store.actionExecutor as? RecordingHostActionExecutor)
            driver.recording.holdStop = true
            driver.recording.succeeds = cause != "unknown"
            store.consumeHostInput(event(1, .pressed))
            await until { !driver.starts.isEmpty }
            let state = try XCTUnwrap(store.routing)
            store.enqueueHostAction(json: #"{"action_type":"agent_command","adapter":"cindy","command":"push-to-talk"}"#,
                binding: "ui-AG06", profile: state.profile_id ?? "", state: state)
            await until { driver.recording.stopContinuation != nil }
            let execution = try XCTUnwrap(store.hostExecutionTask)
            var change: Task<Void, Never>?
            if cause == "cancel" { store.cancelHostExecution() }
            if cause == "profile" {
                change = Task { await store.setHostActiveProfile(store.hostProfiles.first { $0.name == "Codex" }?.id.text) }
                await until { store.mutationDraining }
            }
            driver.recording.stopContinuation?.resume(); driver.recording.stopContinuation = nil
            await execution.value; await change?.value
            XCTAssertTrue(executor.receipts.isEmpty, cause)
            await store.shutdown()
        }
    }
}

extension PushToTalkTests {
    @MainActor func testLateRecheckCannotClearNewUnknownRecording() async {
        let driver = TestDictationDriver(); driver.uncertain = true; driver.recording.succeeds = false
        let held = PushToTalkController(driver: driver)
        held.press(binding: "AG06", adapter: "cindy", report: { _, _, _ in }, finished: {})
        await held.waitUntilStopped()
        let old = driver.recording; old.holdChecks = true; old.externallyStopped = true
        let first = Task { await held.verifyStopped() }
        let second = Task { await held.verifyStopped() }
        await until { old.checks.count == 2 }
        old.checks[0].resume()
        let firstResult = await first.value; XCTAssertTrue(firstResult)
        driver.recording = TestRecording(); driver.recording.succeeds = false
        held.press(binding: "AG06", adapter: "codex", report: { _, _, _ in }, finished: {})
        await held.waitUntilStopped()
        XCTAssertFalse(held.cleanupConfirmed)
        old.checks[1].resume()
        let secondResult = await second.value; XCTAssertFalse(secondResult)
        XCTAssertFalse(held.cleanupConfirmed)
    }
}

@MainActor final class AXDictationRecordingTests: XCTestCase {
    func testChildrenReadDistinguishesVerifiedLeavesFromMissingOrFailedValues() {
        XCTAssertNotNil(AXDictationRecording.children(result: .noValue, value: nil, validLeaf: true))
        XCTAssertNil(AXDictationRecording.children(result: .noValue, value: nil, validLeaf: false))
        XCTAssertNotNil(AXDictationRecording.children(result: .attributeUnsupported, value: nil, validLeaf: false))
        XCTAssertNil(AXDictationRecording.children(result: .success, value: nil, validLeaf: true))
        XCTAssertNil(AXDictationRecording.children(result: .invalidUIElement, value: nil, validLeaf: true))
        XCTAssertNil(AXDictationRecording.children(result: .cannotComplete, value: nil, validLeaf: true))
        XCTAssertNil(AXDictationRecording.children(result: .apiDisabled, value: nil, validLeaf: true))
    }

    @MainActor private final class Target {
        var window = true
        var control = true
        var uniqueIdle = true
        var presses = 0
        var reads = 0
        var invalidateWindowOnStop = false
        func recording() -> AXDictationRecording {
            AXDictationRecording(access: DictationTargetAccess(
                terminated: { false }, originalWindow: { self.window },
                originalControl: { self.window && self.control }, labels: { ["停止语音输入"] },
                uniqueIdleInWindow: { self.reads += 1; return self.uniqueIdle },
                pressOriginal: {
                    self.presses += 1; self.control = false
                    if self.invalidateWindowOnStop { self.window = false }
                }))
        }
    }
    func testStopReadsReplacementButNeverPressesIt() async {
        let target = Target(), recording = target.recording()
        let result = await recording.stop()
        XCTAssertTrue(result.0)
        XCTAssertEqual(target.presses, 1)
        XCTAssertGreaterThan(target.reads, 0)
        let checked = await recording.isStopped()
        XCTAssertTrue(checked)
        let repeated = await recording.stop()
        XCTAssertFalse(repeated.0)
        XCTAssertEqual(target.presses, 1)
    }
    func testReplacementBeforeStopCannotBeAdopted() async {
        let target = Target(), recording = target.recording()
        target.control = false
        let checked = await recording.isStopped()
        let result = await recording.stop()
        XCTAssertFalse(checked)
        XCTAssertFalse(result.0)
        XCTAssertEqual(target.presses, 0)
        XCTAssertEqual(target.reads, 0)
    }
    func testChangedWindowCannotConfirmOrReadReplacement() async {
        let target = Target(), recording = target.recording()
        target.invalidateWindowOnStop = true
        let result = await recording.stop()
        XCTAssertFalse(result.0)
        XCTAssertEqual(target.presses, 1)
        XCTAssertEqual(target.reads, 0)
    }
    func testAmbiguousOrIncompleteReadbackStaysUnknownUntilVerified() async {
        let target = Target(), recording = target.recording()
        target.uniqueIdle = false
        let result = await recording.stop()
        XCTAssertFalse(result.0)
        let unknown = await recording.isStopped()
        XCTAssertFalse(unknown)
        target.uniqueIdle = true
        let verified = await recording.isStopped()
        XCTAssertTrue(verified)
        XCTAssertEqual(target.presses, 1)
    }
}

extension PushToTalkTests {
    @MainActor func testOwnershipHandoffWaitsForLateVoiceStartAndRejectsUnconfirmedStop() async throws {
        let driver = TestDictationDriver(); driver.holdStart = true; driver.recording.succeeds = false
        let ownership = NativeOwnershipController(transport: nil)
        ownership.setDesiredOwner("micro-input")
        let request = ownership.beginRequest(microDrained: true)
        ownership.receive(NativeOwnershipReply(version: 1, requestId: request.requestId,
            clientInstance: request.clientInstance, generation: request.generation,
            nativeInstance: UUID(), nativeGeneration: 1, state: "released", reason: "fixture",
            inputDrained: true, pttDrained: true, lightingDrained: true, capabilities: ["fenced-release-v1"]), for: request)
        let core = MicroInputCore.MicroInput()
        let store = Store(nativeOwnership: ownership, backend: core, dictationDriver: driver)
        store.routingValid = true; store.hostExecutionEnabled = true
        store.startHeldDictation(adapter: "cindy", binding: "AG10", profile: "fixture")
        await until { driver.continuation != nil }
        ownership.invalidate("peer lost")
        var finished = false
        let drain = Task { let result = await store.drainForNativeOwnership(); finished = true; return result }
        try await Task.sleep(for: .milliseconds(20))
        XCTAssertFalse(finished)
        driver.continuation?.resume(); driver.continuation = nil
        let result = await drain.value
        XCTAssertFalse(result)
        XCTAssertEqual(driver.recording.stops, 1)
        XCTAssertFalse(store.pushToTalk.cleanupConfirmed)
        await store.shutdown()
    }
}
