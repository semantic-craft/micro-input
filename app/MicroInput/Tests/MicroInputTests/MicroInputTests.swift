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
        XCTAssertEqual(store.model.sidebarRows.count, 6)
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
        XCTAssertEqual(store.model.sidebarRows.count, 6)

        store.dispatch("device", "OpenReconnectComparison")
        guard case .reconnectComparison = store.model.activeModal else {
            return XCTFail("Expected reconnect comparison modal to open")
        }
        store.dispatch("device", ["ResolveComparison": "KeepLocal"])
        XCTAssertNil(store.model.activeModal)
    }

    @MainActor
    func testRenderInvalidationAndLatencyProfile() throws {
        let dir = try makeIsolatedTempDir("perf")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let mainHosting = NSHostingView(rootView: WorkbenchRootView(store: store))
        mainHosting.frame = NSRect(x: 0, y: 0, width: 1180, height: 740)
        let statusHosting = NSHostingView(rootView: StatusMenuPopoverView(store: store, onOpenMainWindow: {}))
        statusHosting.frame = NSRect(x: 0, y: 0, width: 340, height: 480)
        mainHosting.layoutSubtreeIfNeeded()
        statusHosting.layoutSubtreeIfNeeded()

        ViewRenderTelemetry.isEnabled = true
        defer { ViewRenderTelemetry.isEnabled = false }

        ViewRenderTelemetry.reset()
        let searchStart = DispatchTime.now().uptimeNanoseconds
        for query in ["终", "终端", "终端命", "终端命令", ""] {
            store.dispatch("inspector", ["SetActionSearchQuery": query])
            mainHosting.layoutSubtreeIfNeeded()
            statusHosting.layoutSubtreeIfNeeded()
        }
        let searchElapsedMs = Double(DispatchTime.now().uptimeNanoseconds - searchStart) / 1_000_000.0
        print("PERF_METRIC inspector_search_5_steps ms=\(String(format: "%.2f", searchElapsedMs)) counts=\(ViewRenderTelemetry.counts)")
        XCTAssertEqual(ViewRenderTelemetry.count("CreatorMicroInlineInspectorView"), 5)
        XCTAssertEqual(ViewRenderTelemetry.count("WorkbenchRootView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("ClassicWorkbenchView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("HeaderBar"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("Sidebar"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("CreatorMicroStageView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("CreatorMicroBoardView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("FooterBar"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("StatusMenuPopoverView"), 0)

        ViewRenderTelemetry.reset()
        let slotStart = DispatchTime.now().uptimeNanoseconds
        for slot in [1, 5, 8, 12] {
            store.dispatch("canvas", ["SelectLogicalSlot": slot])
            mainHosting.layoutSubtreeIfNeeded()
            statusHosting.layoutSubtreeIfNeeded()
        }
        let slotElapsedMs = Double(DispatchTime.now().uptimeNanoseconds - slotStart) / 1_000_000.0
        print("PERF_METRIC canvas_slot_select_4_steps ms=\(String(format: "%.2f", slotElapsedMs)) counts=\(ViewRenderTelemetry.counts)")
        XCTAssertEqual(ViewRenderTelemetry.count("CreatorMicroBoardView"), 4)
        XCTAssertEqual(ViewRenderTelemetry.count("CreatorMicroInlineInspectorView"), 4)
        XCTAssertEqual(ViewRenderTelemetry.count("WorkbenchRootView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("ClassicWorkbenchView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("HeaderBar"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("Sidebar"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("CreatorMicroStageView"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("FooterBar"), 0)
        XCTAssertEqual(ViewRenderTelemetry.count("StatusMenuPopoverView"), 0)

        ViewRenderTelemetry.reset()
        let noopStart = DispatchTime.now().uptimeNanoseconds
        store.dispatch("canvas", ["SelectLogicalSlot": 12])
        store.dispatchRaw(["domain": "foreground_app_changed", "action": ["bundle_id": NSNull()]])
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": true]])
        store.pollHardware()
        mainHosting.layoutSubtreeIfNeeded()
        statusHosting.layoutSubtreeIfNeeded()
        let noopElapsedMs = Double(DispatchTime.now().uptimeNanoseconds - noopStart) / 1_000_000.0
        print("PERF_METRIC noop_events_4_steps ms=\(String(format: "%.2f", noopElapsedMs)) counts=\(ViewRenderTelemetry.counts)")
        XCTAssertTrue(
            ViewRenderTelemetry.counts.isEmpty,
            "No-op dispatches and idle hardware polls must trigger 0 JSON decodes and 0 SwiftUI body evaluations, got \(ViewRenderTelemetry.counts)"
        )
    }

    @MainActor
    func testFourHardwareProfileSlotsDefaultCycleAndReassign() throws {
        let dir = try makeIsolatedTempDir("hw-slots")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertEqual(store.sidebar.hardwareSlots.count, 4)
        XCTAssertEqual(store.sidebar.hardwareSlots[0].displayName, "Antigravity")
        XCTAssertEqual(store.sidebar.hardwareSlots[1].displayName, "Antigravity")
        XCTAssertEqual(store.sidebar.hardwareSlots[2].displayName, "Codex")
        XCTAssertEqual(store.sidebar.hardwareSlots[3].displayName, "Deepseek Harness")
        XCTAssertEqual(store.sidebar.activeHardwareSlot, 0)

        store.dispatch("canvas", "CycleTouchProfileSlot")
        XCTAssertEqual(store.sidebar.activeHardwareSlot, 1)
        XCTAssertEqual(store.sidebar.selectedDisplayName, "Antigravity")
        XCTAssertEqual(store.canvas.touchLedStates, [true, false, false])

        store.dispatch("canvas", "CycleTouchProfileSlot")
        XCTAssertEqual(store.sidebar.activeHardwareSlot, 2)
        XCTAssertEqual(store.sidebar.selectedDisplayName, "Codex")
        XCTAssertEqual(store.canvas.touchLedStates, [false, true, false])

        store.dispatch("canvas", "CycleTouchProfileSlot")
        XCTAssertEqual(store.sidebar.activeHardwareSlot, 3)
        XCTAssertEqual(store.sidebar.selectedDisplayName, "Deepseek Harness")
        XCTAssertEqual(store.canvas.touchLedStates, [false, false, true])

        let herdrRow = try XCTUnwrap(store.sidebar.sidebarRows.first(where: { $0.rawName == "Herdr" }))
        store.dispatch("sidebar", [
            "SetHardwareSlotProfile": [
                "slot_index": 3,
                "profile_id": herdrRow.profileId
            ]
        ])
        XCTAssertEqual(store.sidebar.hardwareSlots[3].displayName, "Herdr")
        XCTAssertEqual(store.sidebar.selectedDisplayName, "Herdr")

        store.dispatch("sidebar", "ResetHardwareSlotsToDefault")
        XCTAssertEqual(store.sidebar.hardwareSlots[3].displayName, "Deepseek Harness")
        XCTAssertEqual(store.sidebar.activeHardwareSlot, 0)
        XCTAssertEqual(store.sidebar.selectedDisplayName, "Antigravity")
    }
}
