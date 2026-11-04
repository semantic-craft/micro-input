import XCTest
@testable import MicroInput
import MicroInputCore

final class ControlSettingsTests: XCTestCase {
    @MainActor func testLogicalSlotsModeValidationUndoAndReopenThroughRealBridge() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        let configured = await store.saveControlSettings(mode: "logical_sectors", sectors: "5")
        XCTAssertTrue(configured)
        store.slot = 13; await store.assign("search")
        store.slot = 14; await store.assign("save")
        store.slot = 20; await store.assign("review")
        let sectorKey = store.slotKey
        let switched = await store.saveControlSettings(mode: "source", sectors: "1")
        XCTAssertTrue(switched)
        XCTAssertEqual(store.layer?.joystick.sectors.count, 5)
        XCTAssertEqual(store.draft.edits[sectorKey]?.id, "review")
        await store.undoDraft()
        XCTAssertEqual(store.currentControlSettings?.joystick_mode, "logical_sectors")
        await store.undoDraft(redo: true)
        let revision = store.draft.revision
        let invalid = await store.saveControlSettings(mode: "source", sectors: "-1")
        XCTAssertFalse(invalid)
        XCTAssertTrue(store.controlSettingsError?.contains("活动扇区数") == true)
        XCTAssertEqual(store.draft.revision, revision)
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        XCTAssertEqual(reopened.currentControlSettings?.active_sectors, 1)
        XCTAssertEqual(reopened.draft.edits[sectorKey]?.id, "review")
        XCTAssertEqual(reopened.draft.edits[reopened.key(13)]?.id, "search")
        XCTAssertEqual(reopened.draft.edits[reopened.key(14)]?.id, "save")
        XCTAssertNil(reopened.connectedID)
        XCTAssertTrue(reopened.devices.isEmpty)
    }
}
