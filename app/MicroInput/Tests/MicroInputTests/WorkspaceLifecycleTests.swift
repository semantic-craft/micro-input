import AppKit
import XCTest
@testable import MicroInputUI

final class WorkspaceLifecycleTests: XCTestCase {
    @MainActor
    func testSleepNotificationReachesRustCore() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("mi-sleep-test-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: directory) }
        let center = NotificationCenter()
        let store = CoreStore(isolatedDirectory: directory.path, workspaceNotificationCenter: center)
        let before = store.footer.statusMessage
        let revision = store.header.documentRevision

        center.post(name: NSWorkspace.willSleepNotification, object: nil)

        XCTAssertNotEqual(store.footer.statusMessage, before, "Sleep notification must retire the Rust session")
        XCTAssertEqual(store.header.documentRevision, revision)

        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        store.pollHardware()
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        store.pollHardware()
        XCTAssertFalse(store.header.deviceConnected)
        XCTAssertEqual(store.header.documentRevision, revision)
        store.stopEventLoops()
        let afterStop = store.footer.statusMessage
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        XCTAssertEqual(store.footer.statusMessage, afterStop)
    }

    @MainActor
    func testNotificationCenterDeliversRepeatedEventsUntilStopped() {
        let center = NotificationCenter()
        var events: [WorkspaceLifecycle.Event] = []
        let lifecycle = WorkspaceLifecycle(center: center) { events.append($0) }
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        XCTAssertEqual(events, [.willSleep, .didWake, .didWake])

        lifecycle.stop()
        lifecycle.stop()
        center.post(name: NSWorkspace.willSleepNotification, object: nil)
        center.post(name: NSWorkspace.didWakeNotification, object: nil)
        XCTAssertEqual(events, [.willSleep, .didWake, .didWake])
    }

    @MainActor
    func testStoppedOrDeinitializedObserverRejectsQueuedCallbacks() {
        let center = CapturingNotificationCenter()
        var events: [WorkspaceLifecycle.Event] = []
        var lifecycle: WorkspaceLifecycle? = WorkspaceLifecycle(center: center) { events.append($0) }
        weak let weakLifecycle = lifecycle
        let callbacks = center.callbacks
        lifecycle?.stop()
        callbacks.forEach { $0(Notification(name: NSWorkspace.willSleepNotification)) }
        XCTAssertTrue(events.isEmpty)
        XCTAssertEqual(center.removedCount, 2)

        lifecycle = nil
        XCTAssertNil(weakLifecycle)
        callbacks.forEach { $0(Notification(name: NSWorkspace.didWakeNotification)) }
        XCTAssertTrue(events.isEmpty)

        var another: WorkspaceLifecycle? = WorkspaceLifecycle(center: center) { events.append($0) }
        weak let weakAnother = another
        another = nil
        XCTAssertNil(weakAnother)
        XCTAssertEqual(center.removedCount, 4)
        center.callbacks.forEach { $0(Notification(name: NSWorkspace.didWakeNotification)) }
        XCTAssertTrue(events.isEmpty)
    }
}

private final class CapturingNotificationCenter: NotificationCenter, @unchecked Sendable {
    var callbacks: [@Sendable (Notification) -> Void] = []
    var removedCount = 0

    override func addObserver(
        forName name: NSNotification.Name?,
        object obj: Any?,
        queue: OperationQueue?,
        using block: @escaping @Sendable (Notification) -> Void
    ) -> NSObjectProtocol {
        callbacks.append(block)
        return NSObject()
    }

    override func removeObserver(_ observer: Any) {
        removedCount += 1
    }
}
