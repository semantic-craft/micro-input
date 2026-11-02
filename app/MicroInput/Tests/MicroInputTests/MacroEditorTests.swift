import XCTest
import MicroInputCore
@testable import MicroInput

final class MacroEditorTests: XCTestCase {
    @MainActor func testStructuredMacroEditsPersistThroughRealBridgeAndUndoAsOneChange() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let core = MicroInputCore.MicroInput()
        let store = Store(backend: core, documentURL: url)
        await store.loadWorkbench()
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        var editor = try XCTUnwrap(MacroEditorDraft(action: template))
        editor.insert(kind: "delay", at: 1)
        editor.update(index: 1, milliseconds: "125")
        editor.duplicate(index: 1)
        editor.move(index: 2, by: -1)
        editor.remove(index: 2)
        let edited = try editor.action(named: "My local macro")
        await store.saveAction(edited)
        XCTAssertEqual(store.draft.revision, 1)
        XCTAssertEqual(store.draft.edits[store.slotKey]?.name, "My local macro")
        let reopened = Store(backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        let saved = try XCTUnwrap(reopened.draft.edits[reopened.slotKey])
        let restored = try XCTUnwrap(MacroEditorDraft(action: saved))
        XCTAssertEqual(restored.steps.map(\.kind), ["press", "delay", "release"])
        XCTAssertEqual(restored.steps[1].milliseconds, "125")
        await store.undoDraft()
        XCTAssertNil(store.draft.edits[store.slotKey])
        await store.undoDraft(redo: true)
        XCTAssertEqual(store.draft.edits[store.slotKey]?.name, "My local macro")
        await core.shutdown()
    }
}
