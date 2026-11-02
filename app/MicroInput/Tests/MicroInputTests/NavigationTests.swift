import XCTest
@testable import MicroInput
import MicroInputCore
private final class SpyBackend: Backend {
    let core = MicroInputCore.MicroInput()
    var reads = 0
    var connects = 0
    var fail = false
    func loadDemo() async throws -> String { try await core.loadDemo() }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws { connects += 1 }
    func readConfig() async throws -> String { reads += 1; throw NSError(domain: "协议未验证", code: 1) }
    func disconnect() async {}
    func inputMonitoringStatus() async -> Bool { false }
}
final class NavigationTests: XCTestCase {
    @MainActor func testRustBridgeAndNavigationNeverRequestsDevice() async throws {
        let backend = SpyBackend(); let store = Store(backend: backend)
        await store.loadDemo()
        XCTAssertEqual(store.snapshot?.source, "Demo")
        XCTAssertEqual(store.snapshot?.profiles.count, 2)
        XCTAssertEqual(store.layer?.keys.count, 13)
        XCTAssertEqual(store.layer?.encoders.count, 3)
        store.selectProfile(1)
        XCTAssertEqual(store.profile?.id.text, "42")
        XCTAssertEqual(store.profile?.index, 1)
        store.selectProfile(0); store.selectLayer(1)
        XCTAssertEqual(store.layer?.name, "辅助层")
        XCTAssertEqual(backend.reads, 0); XCTAssertEqual(backend.connects, 0)
        XCTAssertTrue(store.layer?.keys[1].bindingLabel.contains("Unknown") == true)
    }
    @MainActor func testFailedDeviceReadRetainsDemoProvenance() async {
        let backend = SpyBackend(); let store = Store(backend: backend)
        await store.loadDemo(); await store.connect("synthetic")
        XCTAssertEqual(store.snapshot?.source, "Demo")
        XCTAssertTrue(store.stale)
        XCTAssertTrue(store.status.contains("读取失败"))
        XCTAssertEqual(backend.reads, 1)
        store.received([])
        XCTAssertNil(store.connectedID)
        XCTAssertEqual(store.status, "设备已断开")
    }
}
private final class DelayedBackend: Backend {
    var release: CheckedContinuation<Void, Never>?
    var reads = 0
    func loadDemo() async throws -> String { "{}" }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws { await withCheckedContinuation { release = $0 } }
    func readConfig() async throws -> String { reads += 1; return "{}" }
    func disconnect() async {}
    func inputMonitoringStatus() async -> Bool { false }
}
extension NavigationTests {
    @MainActor func testSuspendInvalidatesDelayedConnect() async {
        let backend = DelayedBackend(); let store = Store(backend: backend)
        let task = Task { await store.connect("delayed") }
        while backend.release == nil { await Task.yield() }
        await store.suspend()
        backend.release?.resume(); await task.value
        XCTAssertNil(store.connectedID); XCTAssertEqual(backend.reads, 0)
        XCTAssertEqual(store.status, "会话已断开")
    }
    func testUnknownLargeIntegersAreExact() throws {
        for number in ["9007199254740993", "18446744073709551615", "-9007199254740993"] {
            let value = try JSONDecoder().decode(JSONValue.self, from: Data(number.utf8))
            XCTAssertEqual(value.text, number)
        }
    }
}
extension NavigationTests {
    @MainActor func testRemovalInvalidatesDelayedConnect() async {
        let backend = DelayedBackend(); let store = Store(backend: backend)
        let task = Task { await store.connect("delayed") }
        while backend.release == nil { await Task.yield() }
        store.received([])
        backend.release?.resume(); await task.value
        XCTAssertNil(store.connectedID); XCTAssertEqual(backend.reads, 0)
        XCTAssertEqual(store.status, "设备已断开")
    }
}
extension NavigationTests {
    func testProtocolFailureHasReadableStatus() {
        XCTAssertEqual(readableError(BridgeError.Failed(message: "协议未验证：UNKNOWN-04 fs schema")), "协议未验证，设备读取待确认")
    }
}
