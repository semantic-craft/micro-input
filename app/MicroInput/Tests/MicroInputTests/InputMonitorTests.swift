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
    @MainActor func testMonitorPresentationUsesRealBridgeAndClearsOldSessionOnRestart() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(backend: core)
        await store.connect("synthetic-interface-0")
        let monitor = InputMonitorState()
        let task = Task { await monitor.listen(store: store) }
        for _ in 0..<100 where !monitor.status.contains("正在监听") { try await Task.sleep(nanoseconds: 1_000_000) }
        try await core.emitSyntheticInput(count: 1)
        for _ in 0..<100 where monitor.count == 0 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(monitor.count, 1)
        XCTAssertEqual(monitor.lastEvent?.kind, .unknownNotification)
        XCTAssertTrue(monitor.status.contains("格式待核实"))
        await store.suspend()
        await task.value
        XCTAssertTrue(monitor.status.contains("已停止"))
        await monitor.listen(store: store)
        XCTAssertEqual(monitor.count, 0)
        XCTAssertNil(monitor.lastEvent)
        XCTAssertEqual(monitor.status, "连接设备后持续监听")
    }
}
#endif
