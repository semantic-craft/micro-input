import XCTest
import MicroInputCore
@testable import MicroInput

private final class SuspendedSessionBackend: Backend {
    var reads: [CheckedContinuation<String, Error>] = []
    var connections = 0
    var disconnects = 0
    func loadDemo() async throws -> String { try await MicroInputCore.MicroInput().loadDemo() }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws { connections += 1 }
    func readConfig() async throws -> String { try await withCheckedThrowingContinuation { reads.append($0) } }
    func disconnect() async { disconnects += 1 }
    func inputMonitoringStatus() async -> Bool { false }
}
final class SessionLifecycleTests: XCTestCase {
    @MainActor func testCancelReadAllowsReconnectBeforeOldCallbackAndPreservesDraft() async throws {
        let backend = SuspendedSessionBackend()
        let store = Store(backend: backend)
        await store.loadDemo()
        let local = store.snapshot?.raw.text
        await store.connect("first")
        let old = Task { await store.readDeviceConfig() }
        while backend.reads.isEmpty { await Task.yield() }
        await store.suspend()
        XCTAssertFalse(store.busy)
        guard !store.busy else { backend.reads[0].resume(throwing: BridgeError.Disconnected); await old.value; return }
        await store.connect("second")
        XCTAssertEqual(store.connectedID, "second")
        let current = Task { await store.readDeviceConfig() }
        while backend.reads.count < 2 { await Task.yield() }
        backend.reads[0].resume(throwing: BridgeError.Disconnected)
        await old.value
        XCTAssertTrue(store.busy, "An old callback must not clear the new generation's activity")
        XCTAssertEqual(store.connectedID, "second")
        backend.reads[1].resume(throwing: BridgeError.Timeout)
        await current.value
        XCTAssertFalse(store.busy)
        XCTAssertEqual(store.snapshot?.raw.text, local)
        XCTAssertEqual(backend.connections, 2)
    }
}

#if MI_SYNTHETIC_BACKEND
extension SessionLifecycleTests {
    @MainActor func testEndedBridgeSessionClearsStoreAndReconnectRequiresNewIdentity() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(backend: core)
        await store.loadDemo()
        let local = store.snapshot?.raw.text
        await store.connect("synthetic-interface-0")
        await store.readIdentity()
        let first = store.identity?.sessionId
        await core.disconnect()
        for _ in 0..<100 where store.connectedID != nil { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertNil(store.connectedID)
        XCTAssertNil(store.identity)
        XCTAssertTrue(store.stale)
        XCTAssertEqual(store.snapshot?.raw.text, local)
        await store.connect("synthetic-interface-0")
        XCTAssertNil(store.identity)
        await store.readIdentity()
        XCTAssertNotEqual(store.identity?.sessionId, first)
        await store.suspend()
    }
}
#endif
