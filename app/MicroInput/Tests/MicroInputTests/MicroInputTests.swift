import AppKit
import Foundation
import MicroInputCoreFFI
import SwiftUI
import XCTest
@testable import MicroInputUI

final class MicroInputTests: XCTestCase {
    private func makeIsolatedTempDir(_ name: String) throws -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mi-swift-test-\(name)-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    @MainActor
    func testCoreStoreInitialPresetsAndSidebarSwitch() throws {
        let dir = try makeIsolatedTempDir("presets")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertEqual(store.model.sidebarRows.count, 7)
        XCTAssertEqual(store.model.selectedDisplayName, "Antigravity")
        XCTAssertEqual(store.model.keycapCells.count, 13)
        XCTAssertNil(store.model.protectedLock)

        let cindyRow = try XCTUnwrap(store.model.sidebarRows.first(where: { $0.rawName == "Cindy" }))
        XCTAssertTrue(cindyRow.isProtectedOai)
        store.dispatch("sidebar", ["SelectProfile": cindyRow.profileId])
        XCTAssertEqual(store.model.selectedDisplayName, "Cindy")
        XCTAssertTrue(store.model.isProtectedLayer)
        XCTAssertEqual(store.model.protectedLock?.isLocked, true)

        store.dispatch("canvas", "UnlockBySwitchingOrCreatingCustomLayer")
        XCTAssertFalse(store.model.isProtectedLayer)
        XCTAssertNil(store.model.protectedLock)
    }

    @MainActor
    func testCanvasSelectionWideKeyAndInspectorBinding() throws {
        let dir = try makeIsolatedTempDir("canvas")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        store.dispatch("canvas", ["SelectLogicalSlot": 10])
        XCTAssertEqual(store.model.selectedLogicalSlot, 10)
        XCTAssertEqual(store.model.bottomRowLayout, "split")
        XCTAssertEqual(store.model.keycapCells.count, 13)

        store.dispatch("canvas", ["SetBottomRowLayout": "merged"])
        XCTAssertEqual(store.model.bottomRowLayout, "merged")
        XCTAssertEqual(store.model.keycapCells.count, 12)

        store.dispatch("canvas", ["SetBottomRowLayout": "split"])
        store.dispatch("canvas", ["SelectLogicalSlot": 11])
        store.dispatch("inspector", "BindAppleFnKey")
        XCTAssertEqual(store.model.editableInspector?.isAppleFnBound, true)

        store.dispatch("inspector", [
            "ApplyKeycapIcon": [
                "keycap": "Mic",
                "bind_default_action": false
            ]
        ])
        XCTAssertEqual(store.model.editableInspector?.selectedKeycap, "Mic")

        store.dispatch("inspector", "UnassignSelectedSlot")
        XCTAssertEqual(store.model.editableInspector?.boundActionName, "未绑定")
    }

    @MainActor
    func testMacroAndLightingModalLifecycle() throws {
        let dir = try makeIsolatedTempDir("modals")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        store.dispatch("canvas", ["SelectLogicalSlot": 6])

        store.dispatch("editor", ["OpenMacroEditor": ["macro_id": NSNull()]])
        guard case .macroEditor(let draftMacro) = store.model.activeModal else {
            return XCTFail("Expected macro editor modal to open")
        }
        XCTAssertEqual(draftMacro.steps.count, 0)

        store.dispatch("editor", ["MacroSetName": "构建并测试"])
        store.dispatch("editor", ["MacroAddTapKey": ["key": "b", "delay_ms": 20]])
        guard case .macroEditor(let updatedMacro) = store.model.activeModal else {
            return XCTFail("Expected macro editor modal to remain open")
        }
        XCTAssertEqual(updatedMacro.name, "构建并测试")
        XCTAssertEqual(updatedMacro.steps.count, 3)

        store.dispatch("editor", ["MacroSave": ["bind_to_selected_slot": true]])
        XCTAssertNil(store.model.activeModal)
        XCTAssertEqual(store.model.editableInspector?.boundActionName, "构建并测试")
        XCTAssertEqual(store.model.editableInspector?.macros.count, 1)

        store.dispatch("editor", "OpenLightingEditor")
        guard case .lightingEditor = store.model.activeModal else {
            return XCTFail("Expected lighting editor modal to open")
        }
        store.dispatch("editor", "CloseModal")
        XCTAssertNil(store.model.activeModal)
    }

    func testInstanceLockRejectsDuplicateUntilReleased() throws {
        let dir = try makeIsolatedTempDir("lock")
        defer { try? FileManager.default.removeItem(at: dir) }

        let first = dir.path.withCString { mi_core_acquire_instance_lock($0) }
        XCTAssertNotNil(first)
        defer {
            if let first {
                mi_core_release_instance_lock(first)
            }
        }

        let second = dir.path.withCString { mi_core_acquire_instance_lock($0) }
        XCTAssertNil(second)
    }

    @MainActor
    func testReconnectComparisonAndForegroundAppNullHandling() throws {
        let dir = try makeIsolatedTempDir("reconnect")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        store.dispatchRaw([
            "domain": "foreground_app_changed",
            "action": ["bundle_id": NSNull()]
        ])
        XCTAssertEqual(store.model.sidebarRows.count, 7)

        store.dispatch("device", "OpenReconnectComparison")
        guard case .reconnectComparison = store.model.activeModal else {
            return XCTFail("Expected reconnect comparison modal to open")
        }
        store.dispatch("device", ["ResolveComparison": "KeepLocal"])
        XCTAssertNil(store.model.activeModal)
    }
}
