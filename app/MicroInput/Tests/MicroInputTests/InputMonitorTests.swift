import XCTest
import MicroInputCore
@testable import MicroInput

#if MI_SYNTHETIC_BACKEND
final class InputMonitorTests: XCTestCase {
    @MainActor func testIdleNotificationCrossesRealBridgeAndRemainsUnverified() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.connect(id: "synthetic-interface-0")
        let subscription = try await core.subscribeInput()
        try await core.emitSyntheticInput(count: 1)
        let event = await subscription.nextEvent()
        XCTAssertEqual(event?.kind, .unknownNotification)
        XCTAssertNil(event?.logicalSwitch)
        XCTAssertNil(event?.physicalControl)
        XCTAssertNil(event?.keycapGroup)
        XCTAssertNil(event?.actionRole)
        XCTAssertEqual(event?.verification, "input-schema-unverified")
        XCTAssertEqual(event?.actionEligible, false)
        let requests = await core.syntheticRequests()
        XCTAssertEqual(requests, [])
        subscription.close()
        let closed = await subscription.nextEvent()
        XCTAssertNil(closed)
        await core.disconnect()
    }
    @MainActor func testCandidateBindingsCrossRealBridgeWithoutPhysicalOrActionClaims() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.connect(id: "synthetic-interface-0")
        let subscription = try await core.subscribeInput()
        try await core.emitSyntheticInputCase(inputCase: .candidateBindings)
        let expectedBindings = ["AG00", "AG00", "AG13", "AG13", "AG11", "AG10", "AG10", "AG11"]
        let expectedKinds: [InputKind] = [.candidatePressed, .candidateReleased, .candidatePressed, .candidateReleased, .candidatePressed, .candidatePressed, .candidateReleased, .candidateReleased]
        for index in expectedBindings.indices {
            let event = await subscription.nextEvent()
            XCTAssertEqual(event?.bindingId, expectedBindings[index])
            XCTAssertEqual(event?.kind, expectedKinds[index])
            XCTAssertEqual(event?.sequence, UInt64(index + 1))
            XCTAssertNil(event?.logicalSwitch)
            XCTAssertNil(event?.physicalControl)
            XCTAssertNil(event?.keycapGroup)
            XCTAssertNil(event?.actionRole)
            XCTAssertEqual(event?.verification, "candidate-dispatch-0.6.2-current-firmware-unverified")
            XCTAssertEqual(event?.actionEligible, false)
        }
        try await core.emitSyntheticInputCase(inputCase: .unsupportedVariants)
        for _ in 0..<5 {
            let unknown = await subscription.nextEvent()
            XCTAssertEqual(unknown?.kind, .unknownNotification)
            XCTAssertNil(unknown?.bindingId)
            XCTAssertEqual(unknown?.verification, "input-schema-unverified")
            XCTAssertEqual(unknown?.actionEligible, false)
        }
        let requests = await core.syntheticRequests()
        XCTAssertEqual(requests, [])
        subscription.close()
        let closed = await subscription.nextEvent()
        XCTAssertNil(closed)
        await core.disconnect()
    }
    @MainActor func testGapStopsDeliveryAndExplicitRebuildDiscardsOldBacklog() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.connect(id: "synthetic-interface-0")
        let subscription = try await core.subscribeInput()
        try await core.emitSyntheticInput(count: 300)
        // Request completion is ordered behind injected notification frames in the same reader.
        _ = try await core.readIdentity()
        let event = await subscription.nextEvent()
        XCTAssertEqual(event?.kind, .inputStreamGap)
        XCTAssertEqual(event?.lostCount, 44)
        let stopped = await subscription.nextEvent()
        XCTAssertNil(stopped)
        let rebuilt = try await core.subscribeInput()
        try await core.emitSyntheticInput(count: 1)
        let fresh = await rebuilt.nextEvent()
        XCTAssertEqual(fresh?.kind, .unknownNotification)
        XCTAssertEqual(fresh?.sequence, 1)
        rebuilt.close(); subscription.close()
        await core.disconnect()
    }
    @MainActor func testClosingSubscriptionWakesPendingRead() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.connect(id: "synthetic-interface-0")
        let subscription = try await core.subscribeInput()
        let task = Task { await subscription.nextEvent() }
        await Task.yield()
        subscription.close()
        let result = await task.value
        XCTAssertNil(result)
        await core.disconnect()
    }
}
#endif

#if MI_SYNTHETIC_BACKEND
extension InputMonitorTests {
    @MainActor func testCandidatePresentationNamesBindingAndSourceWithoutPhysicalIdentity() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        await store.connect("synthetic-interface-0")
        let monitor = InputMonitorState()
        let task = Task { await monitor.listen(store: store) }
        for _ in 0..<100 where !monitor.status.contains("正在监听") { try await Task.sleep(nanoseconds: 1_000_000) }
        try await core.emitSyntheticInputCase(inputCase: .candidateBindings)
        for _ in 0..<100 where monitor.count < 8 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(monitor.count, 8)
        XCTAssertEqual(monitor.eventSummary, "候选绑定 AG11 · 释放（未核实）")
        XCTAssertEqual(monitor.candidateSourceSummary, "Dispatch 0.6.2 候选 · 当前固件输入与物理映射未核实")
        XCTAssertEqual(monitor.lastEvent?.actionEligible, false)
        monitor.stop()
        await task.value
        await store.suspend()
        await monitor.listen(store: store)
        XCTAssertNil(monitor.lastEvent)
        XCTAssertNil(monitor.eventSummary)
        XCTAssertNil(monitor.candidateSourceSummary)
    }
    @MainActor func testMonitorPresentationUsesRealBridgeAndClearsOldSessionOnRestart() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        await store.connect("synthetic-interface-0")
        let monitor = InputMonitorState()
        let task = Task { await monitor.listen(store: store) }
        for _ in 0..<100 where !monitor.status.contains("正在监听") { try await Task.sleep(nanoseconds: 1_000_000) }
        try await core.emitSyntheticInput(count: 1)
        for _ in 0..<100 where monitor.count == 0 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(monitor.count, 1)
        XCTAssertEqual(monitor.lastEvent?.kind, .unknownNotification)
        XCTAssertTrue(monitor.status.contains("物理映射待核实"))
        await store.suspend()
        await task.value
        XCTAssertTrue(monitor.status.contains("已停止"))
        await monitor.listen(store: store)
        XCTAssertEqual(monitor.count, 0)
        XCTAssertNil(monitor.lastEvent)
        XCTAssertEqual(monitor.status, "连接设备后持续监听")
    }
    @MainActor func testVerifiedPresentationWhenIdentityReadEvenWithHostExecutionPaused() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        XCTAssertFalse(store.hostExecutionEnabled)
        XCTAssertEqual(store.identity?.version, "synthetic-0.6.3")

        let monitor = InputMonitorState()
        let task = Task { await monitor.listen(store: store) }
        for _ in 0..<100 where !monitor.status.contains("已验证物理映射") { try await Task.sleep(nanoseconds: 1_000_000) }
        try await core.emitSyntheticInputCase(inputCase: .candidateBindings)
        // WideKeyMerger coalesces AG11+AG10 press/release (8 raw events -> 6 verified events)
        for _ in 0..<100 where monitor.count < 6 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(monitor.count, 6)
        XCTAssertEqual(monitor.eventSummary, "已验证输入 AG10 → logical-10 (row3-wide-2u · 键帽组 wide-bottom-2u) · 释放")
        XCTAssertEqual(monitor.lastEvent?.actionEligible, true)
        monitor.stop()
        await task.value
        await store.shutdown()
    }
}
#endif
