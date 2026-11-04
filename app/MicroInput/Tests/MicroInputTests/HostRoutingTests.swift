import XCTest
import AppKit
import MicroInputCore
@testable import MicroInput

@MainActor final class HostRoutingTests: XCTestCase {
    func event(_ sequence: UInt64, _ binding: String?, _ center: Bool?, kind: InputKind = .pressed, session: UInt64 = 1) -> InputEventInfo {
        InputEventInfo(sessionId: session, sequence: sequence, ingressSequence: sequence, radialCenter: center, radialPosition: nil, timestampMs: 0, kind: kind, logicalSwitch: nil, bindingId: binding, physicalControl: nil, keycapGroup: nil, actionRole: center == nil ? "momentary_key" : "joystick_sector", lostCount: 0, verification: "test", actionEligible: kind == .pressed)
    }
    func testUnreadPreCutoffNorthRequiresRealCenterBeforeEast() {
        var boundary = HostInputBoundary()
        boundary.advance(InputWatermark(session_id: 1, ingress_sequence: 0))
        XCTAssertFalse(boundary.consume(event(1, nil, true, kind: .unknownNotification), gateOpen: true).pressed)
        // Sole reader has already published north #10, decoder has not consumed it.
        boundary.advance(InputWatermark(session_id: 1, ingress_sequence: 10))
        XCTAssertFalse(boundary.consume(event(10, "AG15", false), gateOpen: true).pressed)
        XCTAssertFalse(boundary.consume(event(11, "AG18", false), gateOpen: true).pressed, "A sector change is not a center release")
        XCTAssertFalse(boundary.consume(event(12, "AG18", true, kind: .released), gateOpen: true).pressed)
        XCTAssertTrue(boundary.consume(event(13, "AG18", false), gateOpen: true).pressed)
    }
    func testJoystickHeldDuringGateAndNewSessionAndGapQuarantine() {
        var boundary = HostInputBoundary()
        boundary.advance(InputWatermark(session_id: 1, ingress_sequence: 0))
        _ = boundary.consume(event(1, nil, true, kind: .unknownNotification), gateOpen: true)
        XCTAssertTrue(boundary.consume(event(2, "AG15", false), gateOpen: true).pressed)
        boundary.advance(InputWatermark(session_id: 1, ingress_sequence: 2))
        XCTAssertFalse(boundary.consume(event(3, "AG18", false), gateOpen: true).pressed)
        _ = boundary.consume(event(4, nil, true, kind: .unknownNotification), gateOpen: false)
        XCTAssertFalse(boundary.consume(event(5, "AG15", false), gateOpen: false).pressed)
        XCTAssertFalse(boundary.consume(event(6, "AG18", false), gateOpen: true).pressed)
        boundary.recovery = true
        XCTAssertFalse(boundary.consume(event(7, "AG01", nil), gateOpen: true).pressed)
        _ = boundary.consume(event(8, "AG01", nil, kind: .released), gateOpen: true)
        XCTAssertTrue(boundary.consume(event(9, "AG01", nil), gateOpen: true).pressed)
        XCTAssertFalse(boundary.consume(event(10, "AG01", nil, session: 2), gateOpen: true).pressed)
    }
    func fixture(executor: HostActionExecuting? = nil) async throws -> (Store, MicroInputCore.MicroInput, URL) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let core = MicroInputCore.MicroInput()
        let store = Store(backend: core, documentURL: directory.appendingPathComponent("workbench.json"), actionExecutor: executor ?? RecordingHostActionExecutor())
        await store.loadWorkbench(); await store.installCodingAgentPresets()
        return (store, core, directory)
    }
    func settle(_ store: Store) async {
        await store.foregroundTask?.value
        while store.mutationDraining { await Task.yield() }
    }

    func testLinksUnmappedAppManualHoldAndRestart() async throws {
        let (store, _, dir) = try await fixture()
        defer { try? FileManager.default.removeItem(at: dir) }
        let a = try XCTUnwrap(store.hostProfiles.first?.id.text)
        let b = store.hostProfiles[1].id.text
        await store.linkApp("test.editor", profileID: b)
        await store.setAutoFollow(true)
        store.foregroundChanged(bundleID: "test.editor"); await settle(store)
        XCTAssertEqual(store.hostActiveProfileId, b)
        store.foregroundChanged(bundleID: "test.unmapped"); await settle(store)
        XCTAssertEqual(store.hostActiveProfileId, b)
        XCTAssertFalse(try XCTUnwrap(store.routing).manual_override)
        await store.setHostActiveProfile(b) // same choice must still pin
        XCTAssertTrue(try XCTUnwrap(store.routing).manual_override)
        await store.linkApp("test.editor", profileID: a)
        store.foregroundChanged(bundleID: "test.editor"); await settle(store)
        XCTAssertEqual(store.hostActiveProfileId, b)
        let reopened = Store(backend: MicroInputCore.MicroInput(), documentURL: dir.appendingPathComponent("workbench.json"))
        await reopened.loadWorkbench()
        XCTAssertTrue(try XCTUnwrap(reopened.routing).manual_override)
        XCTAssertFalse(reopened.hostExecutionEnabled)
        await store.resumeFollowing(); await settle(store)
        XCTAssertEqual(store.hostActiveProfileId, a)
        await store.shutdown(); await reopened.shutdown()
    }
    func testSubmittedSwitchThenUnmappedRetainsCommitAndManualWinsQueue() async throws {
        let (store, core, dir) = try await fixture()
        defer { try? FileManager.default.removeItem(at: dir) }
        let b = store.hostProfiles[1].id.text, d = store.hostProfiles[2].id.text
        await store.linkApp("test.b", profileID: b); await store.setAutoFollow(true)
        var release: CheckedContinuation<Void, Never>?
        let submitted = Task { try await store.hostMutation {
            let state = try XCTUnwrap(store.routing)
            let data = try JSONSerialization.data(withJSONObject: ["document_id":state.document_id,"document_revision":state.document_revision,"host_state_revision":state.host_state_revision,"operation":["kind":"follow","bundle_id":"test.b"]])
            _ = try await core.updateHostRouting(requestJson: String(decoding:data,as:UTF8.self))
            await withCheckedContinuation { release = $0 }
        } }
        while release == nil { await Task.yield() }
        store.foregroundChanged(bundleID: "test.unmapped")
        XCTAssertFalse(store.dispatchOpen)
        release?.resume(); try await submitted.value; await settle(store)
        XCTAssertEqual(store.hostActiveProfileId,b)
        let hold = Task { try await store.hostMutation { await withCheckedContinuation { release = $0 } } }
        release = nil
        while release == nil { await Task.yield() }
        store.foregroundChanged(bundleID: "test.b")
        let manual = Task { await store.setHostActiveProfile(d) }
        await Task.yield(); release?.resume(); try await hold.value; await manual.value; await settle(store)
        XCTAssertEqual(store.hostActiveProfileId,d)
        XCTAssertTrue(try XCTUnwrap(store.routing).manual_override)
        await store.shutdown()
    }
    func testMissingExplicitProfileNoFallbackAndSheetUsesResolver() async throws {
        let (store, _, dir) = try await fixture()
        defer { try? FileManager.default.removeItem(at: dir) }
        let id = store.hostProfiles[1].id.text
        await store.setHostActiveProfile(id)
        store.selectProfile(1); await store.deleteProfile()
        XCTAssertEqual(store.hostActiveProfileId,id)
        XCTAssertNil(store.routing?.profile_name)
        XCTAssertTrue(try XCTUnwrap(store.routing).keys.allSatisfy { !$0.available })
        XCTAssertEqual(store.routing?.keys.filter { $0.binding_id == "AG10" || $0.binding_id == "AG11" }.count,1)
        await store.shutdown()
    }
    func testOneExecutorOverflowAndMutationDiscardPendingActions() async throws {
        let executor = HeldHostExecutor()
        let (store, _, dir) = try await fixture(executor: executor)
        defer { try? FileManager.default.removeItem(at: dir) }
        await store.setHostExecutionEnabled(true)
        let state = try XCTUnwrap(store.routing)
        let action = #"{"action_type":"open_app","bundle_id":"test.app"}"#
        store.enqueueHostAction(json:action,binding:"device-first",profile:state.profile_id ?? "",state:state)
        while executor.release == nil { await Task.yield() }
        let ui = Task { await store.openDesktopAgent(DesktopAgent.all[0]) }
        await Task.yield()
        for n in 0..<33 { store.enqueueHostAction(json:action,binding:"pending-\(n)",profile:state.profile_id ?? "",state:state) }
        XCTAssertTrue(store.overflowDraining)
        XCTAssertTrue(store.pendingHostActions.isEmpty)
        XCTAssertEqual(executor.started,["device-first"])
        executor.release?.resume(); executor.release = nil
        await store.hostExecutionTask?.value; await ui.value
        XCTAssertFalse(store.overflowDraining)
        XCTAssertEqual(store.lastHostActionReceipt?.profileId,state.profile_id)
        store.enqueueHostAction(json:action,binding:"fresh",profile:state.profile_id ?? "",state:state)
        while executor.release == nil { await Task.yield() }
        store.enqueueHostAction(json:action,binding:"obsolete",profile:state.profile_id ?? "",state:state)
        await store.createProfile(name:"Changed document")
        XCTAssertTrue(store.pendingHostActions.isEmpty)
        executor.release?.resume(); executor.release = nil
        await store.hostExecutionTask?.value
        XCTAssertEqual(executor.started,["device-first","fresh"])
        await store.shutdown()
    }
    func testExpiredQueuedActionIsDiscarded() async throws {
        let executor = HeldHostExecutor()
        let (store, _, dir) = try await fixture(executor: executor)
        defer { try? FileManager.default.removeItem(at: dir) }
        await store.setHostExecutionEnabled(true)
        let state = try XCTUnwrap(store.routing)
        let action = #"{"action_type":"open_app","bundle_id":"test.app"}"#
        store.enqueueHostAction(json:action,binding:"running",profile:"original",state:state)
        while executor.release == nil { await Task.yield() }
        store.pendingHostActions.append(PendingHostAction(json:action,binding:"expired",profile:"original",state:state,deviceSession:nil,admitted:ContinuousClock.now.advanced(by:.seconds(-2)),requiresEnabled:true,done:nil))
        executor.release?.resume(); executor.release = nil
        await store.hostExecutionTask?.value
        XCTAssertEqual(executor.started,["running"])
        await store.shutdown()
    }
    func testNativeNoticeTimerAndSheetPreserveForeground() async throws {
        let (store, _, dir) = try await fixture()
        defer { try? FileManager.default.removeItem(at: dir) }
        let frontmost = NSWorkspace.shared.frontmostApplication?.processIdentifier
        store.overlay.showNotice(name:"S5 native notice",mode:"自动跟随")
        let notice = try XCTUnwrap(NSApp.windows.first { $0 is PassiveHostPanel && $0.isVisible })
        XCTAssertFalse(notice.isKeyWindow)
        XCTAssertEqual(NSWorkspace.shared.frontmostApplication?.processIdentifier,frontmost)
        try await Task.sleep(for:.milliseconds(1650))
        XCTAssertFalse(notice.isVisible)
        store.overlay.showSheet()
        let sheet = try XCTUnwrap(NSApp.windows.first { $0 is PassiveHostPanel && $0.isVisible })
        XCTAssertFalse(sheet.isKeyWindow)
        XCTAssertEqual(NSWorkspace.shared.frontmostApplication?.processIdentifier,frontmost)
        XCTAssertTrue(sheet.collectionBehavior.contains(.fullScreenAuxiliary))
        for screen in NSScreen.screens {
            sheet.setFrame(HostOverlay.position(size:NSSize(width:400,height:610),screen:screen.visibleFrame),display:true)
            XCTAssertTrue(screen.visibleFrame.contains(sheet.frame))
        }
        store.overlay.hide()
        XCTAssertFalse(sheet.isVisible)
        await store.shutdown()
    }
    func testPanelClampedOnSmallSecondaryDisplay() {
        let frame = HostOverlay.position(size: NSSize(width:400,height:610), screen:NSRect(x:-500,y:-200,width:300,height:400))
        XCTAssertTrue(NSRect(x:-500,y:-200,width:300,height:400).contains(frame))
        let panel = PassiveHostPanel(contentRect:.zero,styleMask:[.nonactivatingPanel],backing:.buffered,defer:false)
        XCTAssertFalse(panel.canBecomeKey); XCTAssertFalse(panel.canBecomeMain)
    }
}

@MainActor private final class HeldHostExecutor: HostActionExecuting {
    var release: CheckedContinuation<Void,Never>?
    var started: [String] = []
    func execute(actionJSON:String,bindingId:String,profileId:String) async -> HostActionReceipt {
        started.append(bindingId)
        await withCheckedContinuation { release = $0 }
        return HostActionReceipt(timestampMs:0,profileId:profileId,bindingId:bindingId,actionType:"open_app",target:"test.app",textByteLen:nil,outcome:"simulated",detail:"held executor completed")
    }
}
