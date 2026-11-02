import XCTest
@testable import MicroInput
import MicroInputCore

final class ActionLibraryTests: XCTestCase {
    @MainActor func testFavoriteSearchAssignmentAndUndoAcrossRealBridgeAndRestart() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let core = MicroInputCore.MicroInput(), store = Store(backend: core, documentURL: url)
        await store.loadWorkbench()
        let revision = store.draft.revision
        await store.setFavorite("app", favorite: true)
        await store.searchActions(query: "TextEdit", category: "应用", favoritesOnly: true)
        XCTAssertEqual(store.library.entries.map { $0.action.id }, ["app"])
        XCTAssertEqual(store.draft.revision, revision)
        XCTAssertTrue(store.draft.edits.isEmpty)
        XCTAssertFalse(store.draft.can_undo)
        await store.assign("app")
        XCTAssertEqual(store.draft.edits[store.slotKey]?.payload["bundle_id"]?.text, "com.apple.TextEdit")
        XCTAssertEqual(store.actionCapabilities[store.slotKey]?.target, "com.apple.TextEdit")
        XCTAssertEqual(store.actionCapabilities[store.slotKey]?.execution_available, false)
        await store.undoDraft()
        XCTAssertTrue(store.draft.edits.isEmpty)
        let reopened = Store(backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        await reopened.searchActions(query: "", category: "应用", favoritesOnly: true)
        XCTAssertEqual(reopened.library.entries.map { $0.action.id }, ["app"])
        XCTAssertTrue(reopened.draft.edits.isEmpty)
        XCTAssertNil(reopened.connectedID)
        XCTAssertTrue(reopened.devices.isEmpty)
    }
}
