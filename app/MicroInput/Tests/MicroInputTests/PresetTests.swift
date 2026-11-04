import XCTest
@testable import MicroInput
import MicroInputCore

final class PresetTests: XCTestCase {
    @MainActor func testPresetImportPreviewIsolationAndRestartThroughRealBridge() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("workbench.json")
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        await store.assign("save")
        let original = try Data(contentsOf: url)
        let text = try await store.presetText(scope: "layer")
        let summary = try store.inspectPreset(text)
        XCTAssertEqual(summary.profiles, 1); XCTAssertEqual(summary.layers, 1)
        XCTAssertEqual(try Data(contentsOf: url), original)
        let imported = await store.importPreset(text)
        XCTAssertTrue(imported)
        XCTAssertEqual(store.snapshot?.source, "LocalDraft")
        XCTAssertEqual(store.snapshot?.profiles.count, 1)
        XCTAssertEqual(store.draft.edits.values.first?.id, "save")
        XCTAssertFalse(store.draft.can_undo)
        XCTAssertEqual(try Data(contentsOf: url), original)
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        XCTAssertEqual(reopened.draft.edits.values.first?.id, "save")
        let installed = await reopened.importPreset(try reopened.starterPreset())
        XCTAssertTrue(installed)
        XCTAssertEqual(reopened.profile?.name, "Coding 起步")
        XCTAssertEqual(reopened.draft.edits.count, 2)
        let before = reopened.profile?.id.text
        let bad = await reopened.importPreset("{broken")
        XCTAssertFalse(bad)
        XCTAssertEqual(reopened.profile?.id.text, before)
        XCTAssertNil(reopened.connectedID)
        XCTAssertTrue(reopened.devices.isEmpty)
    }
}
