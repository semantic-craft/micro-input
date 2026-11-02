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

extension MacroEditorTests {
    @MainActor func testSharedMacroBindingEditUnlinkAndGuardedDeleteThroughRealBridge() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        let saved = await store.saveMacro(template)
        XCTAssertTrue(saved)
        let id = try XCTUnwrap(store.macros.first?.id)
        store.slot = 1; await store.bindMacro(id)
        XCTAssertEqual(store.macros.first?.references.count, 2)
        var changed = template; changed.name = "Shared revision"
        let result = await store.saveMacro(changed, id: id)
        XCTAssertTrue(result)
        XCTAssertEqual(store.draft.revision, 3)
        XCTAssertEqual(store.draft.edits.values.filter { $0.name == "Shared revision" }.count, 2)
        await store.deleteMacro(id)
        XCTAssertEqual(store.draft.revision, 3)
        XCTAssertTrue(store.status.contains("logical-0"))
        let re = Store(backend: MicroInputCore.MicroInput(), documentURL: url)
        await re.loadWorkbench()
        XCTAssertEqual(re.macros.first?.id, id)
        XCTAssertEqual(re.macros.first?.references.count, 2)
        await re.bindMacro(nil); re.slot = 1; await re.bindMacro(nil)
        XCTAssertEqual(re.macros.first?.references.count, 0)
        await re.deleteMacro(id)
        XCTAssertTrue(re.macros.isEmpty)
        await re.undoDraft()
        XCTAssertEqual(re.macros.first?.id, id)
        await re.undoDraft()
        XCTAssertEqual(re.macros.first?.references.count, 1)
    }
    func testEditorLocalUndoRestoresRemovedAndReorderedSteps() throws {
        let data = Data(#"{"id":"macro","name":"Local","category":"普通宏","detail":"local","payload":{"type":"local_macro","macro_sequence":{"format_version":1,"events":[{"type":"press","key":"a"},{"type":"release","key":"a"}]}}}"#.utf8)
        let action = try JSONDecoder().decode(CatalogAction.self, from: data)
        var editor = try XCTUnwrap(MacroEditorDraft(action: action))
        editor.insert(kind: "delay", at: 1)
        editor.move(index: 1, by: 1)
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "release", "delay"])
        editor.restore()
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "delay", "release"])
        editor.restore()
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "release"])
        editor.restore(redo: true)
        XCTAssertEqual(editor.steps.map(\.kind), ["press", "delay", "release"])
        editor.update(index: 1, milliseconds: "-1")
        XCTAssertThrowsError(try editor.action(named: "Local"))
    }
}
