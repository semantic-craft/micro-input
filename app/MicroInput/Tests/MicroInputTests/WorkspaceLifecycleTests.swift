import XCTest
import AppKit
import MicroInputCore
@testable import MicroInput

#if MI_SYNTHETIC_BACKEND
// The bridge boundary is held only to make close/join ordering deterministic.
private final class DelayedSleepBackend: Backend {
    var disconnectRelease: CheckedContinuation<Void, Never>?
    func loadDemo() async throws -> String { try await MicroInputCore.MicroInput().loadDemo() }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws {}
    func readConfig() async throws -> String { throw BridgeError.Disconnected }
    func disconnect() async { await withCheckedContinuation { disconnectRelease = $0 } }
    func shutdown() async {}
    func inputMonitoringStatus() async -> Bool { false }
}
final class WorkspaceLifecycleTests: XCTestCase {
    @MainActor func testSleepRetiresSnapshotAndWakeOnlyRediscoversUntilExplicitReconnect() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.setSyntheticConfig(scenario: 1)
        let store = Store(backend: core, documentURL: directory.appendingPathComponent("workbench.json"))
        await store.loadWorkbench()
        await store.assign("save")
        let revision = store.draft.revision
        await store.startDiscovery()
        store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        let oldSession = store.identity?.sessionId
        await store.readDeviceConfig()
        let oldSnapshot = store.snapshot?.raw.text
        let requests = await core.syntheticRequests()
        let center = NotificationCenter()
        let observer = WorkspaceSleepObserver(store: store, center: center)
        defer { observer.stop() }

        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        XCTAssertNil(store.connectedID, "Sleep must retire UI generation before returning from the notification")
        XCTAssertNil(store.identity)
        XCTAssertNil(store.selectedDeviceID)
        XCTAssertTrue(store.stale)
        XCTAssertFalse(store.busy)
        XCTAssertEqual(store.snapshot?.raw.text, oldSnapshot)
        XCTAssertEqual(store.draft.revision, revision)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        for _ in 0..<200 where store.devices.isEmpty { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(store.devices.count, 2)
        XCTAssertNil(store.selectedDeviceID)
        XCTAssertNil(store.connectedID)
        XCTAssertNil(store.identity)
        XCTAssertTrue(store.stale)
        let afterWake = await core.syntheticRequests()
        XCTAssertEqual(afterWake, requests, "Wake must not open, query identity, read config or write")
        store.showLocalDraft()
        XCTAssertTrue(store.draft.can_undo)
        await store.undoDraft()
        XCTAssertTrue(store.draft.edits.isEmpty)
        store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        XCTAssertNotEqual(store.identity?.sessionId, oldSession)
        await store.shutdown()
    }
    @MainActor func testSleepCancelsBridgeReadAndInputAndDuplicateWakeCannotReconnect() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.setSyntheticConfig(scenario: 1)
        let store = Store(backend: core)
        await store.loadDemo()
        let original = store.snapshot?.raw.text
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        let session = store.identity?.sessionId
        let stream = try await store.subscribeInput()
        let nextInput = Task { await stream.nextEvent() }
        let count = await core.syntheticRequests().count
        let read = Task { await store.readDeviceConfig() }
        for _ in 0..<200 {
            if await core.syntheticRequests().count > count { break }
            try await Task.sleep(nanoseconds: 1_000_000)
        }
        XCTAssertTrue(store.busy)
        let center = NotificationCenter()
        let observer = WorkspaceSleepObserver(store: store, center: center)
        defer { observer.stop() }
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        // Even explicit stale UI actions cannot revive the sleeping session.
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        await store.readDeviceConfig()
        XCTAssertNil(store.connectedID)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        await store.systemDidWake()
        await read.value
        let ended = await nextInput.value
        XCTAssertNil(ended)
        XCTAssertEqual(store.snapshot?.raw.text, original)
        XCTAssertTrue(store.stale)
        XCTAssertFalse(store.busy)
        XCTAssertNil(store.connectedID)
        XCTAssertTrue(store.devices.isEmpty, "Discovery was never explicitly enabled")
        XCTAssertEqual(store.discoveryStatus, "USB 发现尚未开始")
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        XCTAssertNotEqual(store.identity?.sessionId, session)
        let requests = await core.syntheticRequests()
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        await Task.yield()
        XCTAssertEqual(store.connectedID, "synthetic-interface-0")
        let afterDuplicateWake = await core.syntheticRequests()
        XCTAssertEqual(requests, afterDuplicateWake)
        await store.shutdown()
    }

    @MainActor func testQueuedDiscoveryCallbacksCannotRetireANewConnectionAfterInvalidation() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(backend: core)
        await store.refresh()
        store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0")
        // DeviceEvents is the real Rust→Swift callback boundary. Queue an old removal
        // before the lifecycle invalidates it; delivery occurs only after yielding.
        let events = Events(store)
        events.changed(devices: [])
        events.invalidatePending()
        try await Task.sleep(nanoseconds: 10_000_000)
        XCTAssertEqual(store.connectedID, "synthetic-interface-0")
        XCTAssertEqual(store.selectedDeviceID, "synthetic-interface-0")
        events.changed(devices: [])
        for _ in 0..<100 where store.connectedID != nil { await Task.yield() }
        XCTAssertNil(store.connectedID, "A current callback must still retire the removed device")
        await store.shutdown()
    }

    @MainActor func testWakeWaitsForDisconnectAndAnotherSleepInvalidatesPendingWake() async throws {
        let backend = DelayedSleepBackend()
        let store = Store(backend: backend)
        await store.connect("first")
        let center = NotificationCenter()
        let observer = WorkspaceSleepObserver(store: store, center: center)
        defer { observer.stop() }
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        for _ in 0..<100 where backend.disconnectRelease == nil { await Task.yield() }
        let release = try XCTUnwrap(backend.disconnectRelease)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        await Task.yield()
        await store.connect("premature")
        XCTAssertNil(store.connectedID, "Wake must wait for the old close/join")
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        release.resume()
        for _ in 0..<20 { await Task.yield() }
        await store.connect("premature")
        XCTAssertNil(store.connectedID, "The previous wake must not revive a later sleep cycle")
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        await store.systemDidWake()
        await store.connect("explicit")
        XCTAssertEqual(store.connectedID, "explicit")
        await store.shutdown()
    }

    @MainActor func testObserverStopAndDeinitRemoveNotificationEffects() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(backend: core)
        await store.connect("synthetic-interface-0")
        let center = NotificationCenter()
        var observer: WorkspaceSleepObserver? = WorkspaceSleepObserver(store: store, center: center)
        observer?.stop()
        observer?.stop()
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        XCTAssertEqual(store.connectedID, "synthetic-interface-0")
        observer = WorkspaceSleepObserver(store: store, center: center)
        weak let released = observer
        observer = nil
        XCTAssertNil(released, "The notification center must not retain its owner")
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        await Task.yield()
        XCTAssertEqual(store.connectedID, "synthetic-interface-0")
        await store.shutdown()
    }
}
#endif
