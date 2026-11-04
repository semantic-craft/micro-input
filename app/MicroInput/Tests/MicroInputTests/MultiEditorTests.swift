import XCTest
import MicroInputCore
@testable import MicroInput

final class MultiEditorTests: XCTestCase {
    @MainActor func testFourBranchesUseSharedMacroAndOtherActionsAcrossSaveUndoAndReopen() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        let macro = try XCTUnwrap(store.catalog.first { $0.id == "local-macro" })
        assertSaved( await store.saveMacro(macro))
        let macroID = try XCTUnwrap(store.macros.first?.id)
        let template = try XCTUnwrap(store.catalog.first { $0.id == "local-multi" })
        var editor = try XCTUnwrap(MultiEditorDraft(action: template))
        editor.term = "375"
        editor.branches[.tap] = MultiTarget(actionID: macroID, isMacro: true)
        let childID = UUID().uuidString.lowercased()
        let child = try XCTUnwrap(store.catalog.first { $0.id == "save" })
        editor.branches[.doubleTap] = MultiTarget(actionID: childID, isMacro: false)
        editor.branches[.hold] = MultiTarget(actionID: macroID, isMacro: true)
        editor.branches[.tapHold] = MultiTarget(actionID: childID, isMacro: false)
        store.slot = 1
        let revision = store.draft.revision
        assertSaved( await store.saveMulti(try editor.action(named: "Four gestures"), additions: [childID:child]))
        XCTAssertEqual(store.draft.revision, revision + 1)
        XCTAssertEqual(store.macros.first?.references.filter { $0.parent_action_name == "Four gestures" }.count, 2)
        XCTAssertTrue(store.parameterSummary(store.multis[0].action).contains("双击：保存文件"))
        await store.deleteMacro(macroID)
        XCTAssertTrue(store.status.contains("Four gestures / 单击"))
        XCTAssertEqual(store.draft.revision, revision + 1)
        let multiID = try XCTUnwrap(store.multis.first?.id)
        store.slot = 2; await store.bindMulti(multiID)
        XCTAssertEqual(store.selectedMulti?.references.count, 2)
        editor.branches[.tap] = nil
        assertSaved( await store.saveMulti(try editor.action(named: "Four gestures"), id: multiID))
        await store.undoDraft()
        XCTAssertNotNil(MultiEditorDraft(action: store.multis[0].action)?.branches[.tap])
        await store.undoDraft(redo: true)
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        let restored = try XCTUnwrap(MultiEditorDraft(action: reopened.multis[0].action))
        XCTAssertNil(restored.branches[.tap]); XCTAssertEqual(restored.term, "375")
        XCTAssertEqual(restored.branches[.tapHold]?.actionID, childID)
        XCTAssertEqual(reopened.multis.first?.references.count, 2)
        store.slot = 3
        assertSaved(await store.saveMulti(try editor.action(named: "Library edit"), id: multiID))
        XCTAssertNil(store.draft.edits[store.slotKey])
        XCTAssertEqual(store.multis.first?.references.count, 2)
        editor.term = "-1"; XCTAssertThrowsError(try editor.action(named: "invalid"))
    }
}
private extension XCTestCase {
    func assertSaved(_ value: Bool, file: StaticString = #filePath, line: UInt = #line) { XCTAssertEqual(value, true, file: file, line: line) }
}
