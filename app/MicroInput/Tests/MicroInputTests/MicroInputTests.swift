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
        XCTAssertEqual(store.model.sidebarRows.count, 5)
        XCTAssertEqual(store.model.selectedDisplayName, "Antigravity")
        XCTAssertEqual(store.model.keycapCells.count, 13)
        XCTAssertNil(store.model.protectedLock)

        let localCodex = try XCTUnwrap(store.model.sidebarRows.first(where: { $0.displayName == "Codex" }))
        XCTAssertFalse(localCodex.isProtectedOai)
        store.dispatch("sidebar", ["SelectProfile": localCodex.profileId])
        XCTAssertEqual(store.model.selectedDisplayName, "Codex")
        XCTAssertFalse(store.model.isProtectedLayer)

        store.dispatch("sidebar", ["SetShowOtherProfiles": true])
        let codexRow = try XCTUnwrap(store.model.sidebarRows.first(where: { $0.rawName == "Codex" }))
        XCTAssertTrue(codexRow.isProtectedOai)
        store.dispatch("sidebar", ["SelectProfile": codexRow.profileId])
        XCTAssertEqual(store.model.selectedDisplayName, codexRow.displayName)
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

        store.dispatch("editor", ["OpenMultiEditor": ["multi_id": NSNull()]])
        guard case .multiEditor(let multiDraft) = store.activeModal else {
            return XCTFail("Expected multi editor modal to open via Rust FFI")
        }
        XCTAssertEqual(multiDraft.branches.count, 4)
        if let firstAct = multiDraft.availableActions.first {
            store.dispatch("editor", [
                "MultiSetBranch": [
                    "branch": "Tap",
                    "target": ["Action": ["action_id": firstAct.id]],
                ]
            ])
        }
        if let firstMac = multiDraft.availableMacros.first {
            store.dispatch("editor", [
                "MultiSetBranch": [
                    "branch": "Hold",
                    "target": ["Macro": ["action_id": firstMac.id]],
                ]
            ])
        }
        guard case .multiEditor(let updatedMulti) = store.activeModal else {
            return XCTFail("Expected multi editor modal to remain open")
        }
        XCTAssertEqual(updatedMulti.branches.first(where: { $0.branch == "Tap" })?.targetKind, "Action")
        XCTAssertEqual(updatedMulti.branches.first(where: { $0.branch == "Hold" })?.targetKind, "Macro")
        store.dispatch("editor", "CloseModal")
        XCTAssertNil(store.activeModal)

        store.dispatch("device", "OpenApplyPreparation")
        guard case .applyPreparation(let applyPrep) = store.activeModal else {
            return XCTFail("Expected apply preparation modal to open via Rust FFI")
        }
        XCTAssertFalse(applyPrep.documentId.isEmpty)
        store.dispatch("editor", "CloseModal")
        XCTAssertNil(store.activeModal)
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
        XCTAssertEqual(store.model.sidebarRows.count, 5)

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

        mainHosting.layoutSubtreeIfNeeded()
        statusHosting.layoutSubtreeIfNeeded()
        XCTAssertNotNil(store.statusMenu.cheatSheetPreview)

        ViewRenderTelemetry.reset()
        store.dispatch("session_strip", ["TriggerTestLighting": "thinking"])
        store.dispatch("session_strip", ["TriggerTestLighting": "completed"])
        store.dispatch("session_strip", ["TriggerTestLighting": NSNull()])
        store.dispatch("inspector", ["SetActionSearchQuery": "会话"])
        store.dispatch("inspector", ["SetActionSearchQuery": ""])
        store.dispatch("canvas", ["SelectLogicalSlot": 3])
        store.dispatch("overlay", "ToggleCheatSheet")
        XCTAssertTrue(store.overlay.cheatSheetVisible)
        mainHosting.layoutSubtreeIfNeeded()
        statusHosting.layoutSubtreeIfNeeded()
        store.dispatch("overlay", "CloseCheatSheet")
        XCTAssertFalse(store.overlay.cheatSheetVisible)
        mainHosting.layoutSubtreeIfNeeded()
        statusHosting.layoutSubtreeIfNeeded()
        XCTAssertEqual(
            ViewRenderTelemetry.count("StatusMenuPopoverView"),
            0,
            "Status menu must trigger 0 StatusMenuPopoverView renders during live session strip, canvas selection, inspector search, or main overlay cheat-sheet toggles, got \(ViewRenderTelemetry.counts)"
        )
        XCTAssertEqual(
            ViewRenderTelemetry.count("PopoverMiniCheatSheetView"),
            0,
            "CheatSheet popover grid must remain completely static across live session and main overlay updates, got \(ViewRenderTelemetry.counts)"
        )
    }

    @MainActor
    func testDomainSliceIsolationAndIdlePollZeroDecode() throws {
        let dir = try makeIsolatedTempDir("slice-isolation")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        defer { store.stopEventLoops() }

        let nav = WorkbenchNavigationModel()
        let updater = SoftwareUpdateController()
        let launchAtLogin = LaunchAtLoginController()
        let rootView = WorkbenchRootView(
            store: store,
            navigation: nav,
            updater: updater,
            launchAtLogin: launchAtLogin
        )
        let mainHosting = NSHostingView(rootView: rootView)
        mainHosting.frame = NSRect(x: 0, y: 0, width: 1180, height: 740)
        mainHosting.layoutSubtreeIfNeeded()

        store.dispatch("canvas", ["SelectLogicalSlot": 0])
        mainHosting.layoutSubtreeIfNeeded()

        ViewRenderTelemetry.isEnabled = true
        defer { ViewRenderTelemetry.isEnabled = false }

        ViewRenderTelemetry.reset()
        for _ in 0..<5 {
            store.pollHardware()
        }
        mainHosting.layoutSubtreeIfNeeded()
        XCTAssertEqual(
            ViewRenderTelemetry.count("CoreStore.decodeProjection"),
            0,
            "Idle pollHardware() calls must trigger 0 CoreStore.decodeProjection decodes, got \(ViewRenderTelemetry.counts)"
        )
        XCTAssertTrue(
            ViewRenderTelemetry.counts.isEmpty,
            "Idle pollHardware() calls must trigger 0 SwiftUI body evaluations, got \(ViewRenderTelemetry.counts)"
        )

        ViewRenderTelemetry.reset()
        let initialFavorites = store.statusMenu.favorites2x2
        if let alternateProfileId = store.sidebar.sidebarRows.first(where: { $0.id != initialFavorites.first?.profileId })?.id {
            store.dispatch("status_menu", ["PinFavoriteSlot": ["slot": 0, "profile_id": alternateProfileId]])
            XCTAssertNotEqual(store.statusMenu.favorites2x2, initialFavorites)
        }
        let initialLighting = store.statusMenu.lightingEnabled
        store.dispatch("session_strip", ["SetLightingEnabled": !initialLighting])
        XCTAssertEqual(store.statusMenu.lightingEnabled, !initialLighting)
        store.dispatch("status_menu", ["SetRadialMenuEnabled": !store.statusMenu.radialMenuEnabled])
        mainHosting.layoutSubtreeIfNeeded()

        XCTAssertEqual(
            ViewRenderTelemetry.count("WorkbenchRootView"),
            0,
            "Mutating statusMenu must not re-render WorkbenchRootView, got \(ViewRenderTelemetry.counts)"
        )
        XCTAssertEqual(
            ViewRenderTelemetry.count("CreatorMicroStageView"),
            0,
            "Mutating statusMenu must not re-render CreatorMicroStageView, got \(ViewRenderTelemetry.counts)"
        )
        XCTAssertEqual(
            ViewRenderTelemetry.count("CreatorMicroInlineInspectorView"),
            0,
            "Mutating statusMenu must not re-render CreatorMicroInlineInspectorView, got \(ViewRenderTelemetry.counts)"
        )

        ViewRenderTelemetry.reset()
        let beforeSessionStrip = store.sessionStrip
        store.dispatch("session_strip", ["SetAutoOffTimeout": "FiveMinutes"])
        store.dispatch("session_strip", ["SetWorkingColor": "AmberGold"])
        store.dispatch("session_strip", ["TriggerTestLighting": "thinking"])
        store.dispatch("session_strip", ["TriggerTestLighting": NSNull()])
        XCTAssertNotEqual(store.sessionStrip, beforeSessionStrip)
        mainHosting.layoutSubtreeIfNeeded()

        XCTAssertEqual(
            ViewRenderTelemetry.count("CreatorMicroInlineInspectorView"),
            0,
            "Mutating sessionStrip must not re-render CreatorMicroInlineInspectorView, got \(ViewRenderTelemetry.counts)"
        )
        XCTAssertEqual(
            ViewRenderTelemetry.count("WorkbenchRootView"),
            0,
            "Mutating sessionStrip must not re-render WorkbenchRootView, got \(ViewRenderTelemetry.counts)"
        )

        ViewRenderTelemetry.reset()
        let beforeShowOtherProfiles = store.sidebar.showOtherProfiles
        store.dispatch("sidebar", ["SetShowOtherProfiles": !beforeShowOtherProfiles])
        XCTAssertEqual(store.sidebar.showOtherProfiles, !beforeShowOtherProfiles)
        mainHosting.layoutSubtreeIfNeeded()

        XCTAssertEqual(
            ViewRenderTelemetry.count("CreatorMicroBoardView"),
            0,
            "Mutating sidebar.showOtherProfiles must not re-render CreatorMicroBoardView, got \(ViewRenderTelemetry.counts)"
        )
        XCTAssertEqual(
            ViewRenderTelemetry.count("WorkbenchRootView"),
            0,
            "Mutating sidebar.showOtherProfiles must not re-render WorkbenchRootView, got \(ViewRenderTelemetry.counts)"
        )
    }

    @MainActor
    func testSixBoardLayersPreviewAndReassignWithoutActivation() throws {
        let dir = try makeIsolatedTempDir("board-layers")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = CoreStore(isolatedDirectory: dir.path)
        let active = store.sidebar.activeHostProfileId
        XCTAssertEqual(store.sidebar.boardLayers.layers.count, 6)
        XCTAssertEqual(store.sidebar.boardLayers.layers.map(\.displayName), ["Codex", "Antigravity", "DeepSeek Coder (DSH)", "未分配", "未分配", "未分配"])
        XCTAssertEqual(store.sidebar.boardLayers.layers[0].owner, "Codex")
        XCTAssertEqual(store.sidebar.boardLayers.layers[3].owner, "Unassigned")
        for (layer, leds) in [(4, [true,true,false]), (5, [false,true,true]), (6, [true,true,true])] {
            store.dispatch("sidebar", ["PreviewBoardLayer": layer])
            XCTAssertEqual(store.canvas.boardPreviewLayer, layer)
            XCTAssertEqual(store.canvas.touchLedStates, leds)
            XCTAssertEqual(store.sidebar.activeHostProfileId, active)
            XCTAssertNil(store.sidebar.boardLayers.observedLayer)
        }
        let native = try XCTUnwrap(store.sidebar.boardLayers.profileChoices.first(where: { $0.displayName == "Codex" }))
        store.dispatch("sidebar", ["SetBoardLayerProfile": ["layer": 5, "profile_id": native.profileId]])
        XCTAssertEqual(store.sidebar.boardLayers.layers[4].owner, "Codex")
        let assignmentIssue = try XCTUnwrap(store.sidebar.boardLayers.layers[4].assignmentIssue)
        XCTAssertTrue(assignmentIssue.contains("保留 OAI 映射"))
        XCTAssertTrue(assignmentIssue.contains("USB 独占后由 Micro Input 当前方案控制"))
        store.dispatch("sidebar", ["SetBoardLayerProfile": ["layer": 5, "profile_id": NSNull()]])
        XCTAssertNil(store.sidebar.boardLayers.layers[4].profileId)
        store.dispatch("sidebar", "ResetBoardLayerPlan")
        XCTAssertNil(store.sidebar.boardLayers.layers[5].profileId)
        XCTAssertEqual(store.sidebar.activeHostProfileId, active)
        store.dispatch("sidebar", "ProvisionBoardLayers")
        XCTAssertNil(store.sidebar.boardLayers.observedLayer)
        XCTAssertFalse(store.sidebar.boardLayers.applyBlockReason.isEmpty)
    }

    @MainActor
    func testAntigravityIdleBlueAndInProgressLightingConfig() throws {
        let dir = try makeIsolatedTempDir("lighting-ag")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertEqual(store.sidebar.selectedDisplayName, "Antigravity")
        XCTAssertTrue(store.sessionStrip.simpleLighting.lightingEnabled)
        XCTAssertTrue(store.sessionStrip.simpleLighting.autoSessionColor)
        XCTAssertTrue(store.sessionStrip.simpleLighting.onlyGlowOnAttentionOrCompleted)
        XCTAssertFalse(store.sessionStrip.simpleLighting.glowOnWorking)
        XCTAssertEqual(store.sessionStrip.attentionCount, 0)
        XCTAssertEqual(store.sessionStrip.simpleLighting.ambientColor, "OceanBlue")
        XCTAssertEqual(store.sessionStrip.simpleLighting.workingColor, "EmeraldGreen")
        XCTAssertEqual(L10n.tr("仅点亮待响应会话", .en), "Only light sessions needing response")
        XCTAssertEqual(L10n.tr("待审批呼吸并点亮底光，已完成常亮", .en), "Approval breathes with underglow; completed stays solid")
        XCTAssertEqual(L10n.tr("进行中的会话也亮呼吸灯", .en), "Also pulse running sessions")

        store.dispatch("session_strip", ["TriggerTestLighting": "completed"])
        XCTAssertEqual(store.sessionStrip.simpleLighting.manualPreviewPhase, "completed")
        XCTAssertEqual(store.canvas.underglowHex, "#0088FF")

        store.dispatch("session_strip", ["TriggerTestLighting": "thinking"])
        XCTAssertEqual(store.sessionStrip.simpleLighting.manualPreviewPhase, "thinking")
        XCTAssertEqual(store.canvas.underglowHex, "#00FF66")

        store.dispatch("session_strip", ["SetWorkingColor": "AmberGold"])
        XCTAssertEqual(store.sessionStrip.simpleLighting.workingColor, "AmberGold")
        XCTAssertEqual(store.canvas.underglowHex, "#FFAA00")

        store.dispatch("session_strip", ["TriggerTestLighting": NSNull()])
        XCTAssertNil(store.sessionStrip.simpleLighting.manualPreviewPhase)
    }

    @MainActor
    func testSidebarProfileAndSurfaceSwitchDistinctnessAndSearchFocusIsolation() throws {
        let dir = try makeIsolatedTempDir("distinct-switch")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        var seenCanvasProfiles = Set<String>()
        var seenInspectorIdentities = Set<String>()

        for row in store.sidebar.sidebarRows {
            store.dispatch("sidebar", ["SelectProfile": row.profileId])
            XCTAssertEqual(store.sidebar.selectedProfileId, row.profileId)
            XCTAssertEqual(store.canvas.selectedProfileId, row.profileId)
            XCTAssertEqual(store.canvas.selectedDisplayName, row.displayName)
            XCTAssertEqual(store.inspector.selectedProfileId, row.profileId)
            XCTAssertEqual(store.inspector.selectedDisplayName, row.displayName)
            seenCanvasProfiles.insert("\(store.canvas.selectedProfileId ?? ""):\(store.canvas.selectedDisplayName)")
            let lockOwner = store.inspector.protectedLock?.ownerClientName ?? "editable"
            seenInspectorIdentities.insert("\(store.inspector.selectedProfileId ?? ""):\(store.inspector.selectedDisplayName):\(lockOwner)")
        }
        XCTAssertEqual(seenCanvasProfiles.count, store.sidebar.sidebarRows.count)
        XCTAssertEqual(seenInspectorIdentities.count, store.sidebar.sidebarRows.count)

        let agRow = try XCTUnwrap(store.sidebar.sidebarRows.first(where: { $0.rawName == "Antigravity" }))
        store.dispatch("sidebar", ["SelectProfile": agRow.profileId])

        for (surface, expectedSlot) in [("EncoderCcw", 13), ("EncoderCw", 14), ("EncoderPress", 15)] {
            store.dispatch("canvas", ["SelectSurface": surface])
            XCTAssertEqual(store.canvas.selectedSurface, surface)
            XCTAssertEqual(store.inspector.selectedSurface, surface)
            XCTAssertEqual(store.inspector.selectedLogicalSlot, expectedSlot)
        }

        for (dirName, expectedSlot) in [("North", 16), ("South", 18), ("West", 17), ("East", 19)] {
            store.dispatch("canvas", ["SelectJoystickDirection": dirName])
            XCTAssertEqual(store.canvas.selectedSurface, "Joystick")
            XCTAssertEqual(store.canvas.selectedJoystickDirection, dirName)
            XCTAssertEqual(store.inspector.selectedJoystickDirection, dirName)
            XCTAssertEqual(store.inspector.selectedLogicalSlot, expectedSlot)
        }

        store.dispatch("canvas", ["SelectLogicalSlot": 6])
        store.dispatch("inspector", ["SetActionSearchQuery": "保存文件"])
        XCTAssertEqual(store.inspector.selectedLogicalSlot, 6)
        XCTAssertEqual(store.inspector.editableInspector?.actionSearchQuery, "保存文件")
        let firstAction = try XCTUnwrap(store.inspector.editableInspector?.filteredActions.first)

        store.dispatch("device", [
            "SimulateKeyPress": ["binding_id": "AG00"]
        ])
        XCTAssertEqual(store.inspector.selectedLogicalSlot, 6)
        XCTAssertEqual(store.inspector.selectedSurface, "Key6")
        XCTAssertEqual(store.inspector.editableInspector?.actionSearchQuery, "保存文件")

        store.dispatch("inspector", ["AssignCatalogAction": firstAction.actionId])
        XCTAssertEqual(store.inspector.editableInspector?.boundActionId, firstAction.actionId)

        store.dispatch("inspector", ["SetActionSearchQuery": ""])
        store.dispatch("device", [
            "SimulateKeyPress": ["binding_id": "AG00"]
        ])
        XCTAssertEqual(store.inspector.selectedLogicalSlot, 0)
        XCTAssertEqual(store.inspector.selectedSurface, "Key0")
    }

    @MainActor
    func testUpdateLaunchPolicyAndSoftwareUpdateControllerLifecycle() throws {
        let validKey = "lwr8Uv2vLk5q4IOem6pjd3V5/EF9aHb2kx/4pzIfKJc="
        let validFeed = "https://github.com/semantic-craft/micro-input/releases/latest/download/appcast.xml"
        let prodIdentity = AppReleaseIdentity(
            marketingVersion: "0.5.0",
            buildVersion: "5",
            bundleIdentifier: "craft.semantic.micro-input.dev",
            feedURLString: validFeed,
            publicEDKey: validKey,
            automaticallyChecksDefault: true
        )
        XCTAssertEqual(prodIdentity.displayVersionLabel, "v0.5.0 (5)")

        XCTAssertEqual(
            UpdateLaunchPolicy.evaluate(
                identity: prodIdentity,
                bundlePath: "/Applications/Micro Input.app",
                environment: [:]
            ),
            .ready
        )

        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: ["MI_DISABLE_UPDATER": "1"]
        ) {} else {
            XCTFail("Expected MI_DISABLE_UPDATER=1 to isolate updater")
        }

        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: ["MI_SYNTHETIC_SCENARIO": "host-execution"]
        ) {} else {
            XCTFail("Expected MI_SYNTHETIC_SCENARIO to isolate updater")
        }

        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: ["MI_DOCUMENT_DIR": "/tmp/mi-isolated-docs"]
        ) {} else {
            XCTFail("Expected MI_DOCUMENT_DIR to isolate updater")
        }

        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: [:],
            arguments: ["/Applications/Micro Input.app/Contents/MacOS/MicroInput", "--isolated"]
        ) {} else {
            XCTFail("Expected --isolated argument to isolate updater")
        }

        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: [:],
            arguments: ["/Applications/Micro Input.app/Contents/MacOS/MicroInput", "--verify"]
        ) {} else {
            XCTFail("Expected --verify argument to isolate updater")
        }

        for buildBundlePath in [
            "/workspace/micro-input/target/MicroInput.app",
            "/workspace/micro-input/app/MicroInput/.build/release/MicroInput.app",
        ] {
            if case .isolated = UpdateLaunchPolicy.evaluate(
                identity: prodIdentity,
                bundlePath: buildBundlePath,
                environment: [:]
            ) {} else {
                XCTFail("Expected \(buildBundlePath) to isolate updater")
            }
        }

        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: ["XCTestConfigurationFilePath": "/tmp/test.xctestconfiguration"]
        ) {} else {
            XCTFail("Expected XCTestConfigurationFilePath to isolate updater")
        }

        let qaIdentity = AppReleaseIdentity(
            marketingVersion: "0.5.0",
            buildVersion: "5",
            bundleIdentifier: "com.semanticcraft.microinput.qa.99",
            feedURLString: validFeed,
            publicEDKey: validKey
        )
        if case .isolated = UpdateLaunchPolicy.evaluate(
            identity: qaIdentity,
            bundlePath: "/Applications/Micro Input QA.app",
            environment: [:]
        ) {} else {
            XCTFail("Expected QA bundle id to isolate updater")
        }

        for badBundlePath in [
            "/private/var/folders/xx/AppTranslocation/123/d/Micro Input.app",
            "/Volumes/Micro Input/Micro Input.app",
            "/tmp/MicroInput",
        ] {
            if case .unavailable = UpdateLaunchPolicy.evaluate(
                identity: prodIdentity,
                bundlePath: badBundlePath,
                environment: [:]
            ) {} else {
                XCTFail("Expected \(badBundlePath) to mark updater unavailable")
            }
        }

        let insecureFeedIdentity = AppReleaseIdentity(
            marketingVersion: "0.5.0",
            buildVersion: "5",
            bundleIdentifier: "craft.semantic.micro-input.dev",
            feedURLString: "http://example.com/appcast.xml",
            publicEDKey: validKey
        )
        if case .unavailable = UpdateLaunchPolicy.evaluate(
            identity: insecureFeedIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: [:]
        ) {} else {
            XCTFail("Expected non-HTTPS feed URL to mark updater unavailable")
        }

        let badKeyIdentity = AppReleaseIdentity(
            marketingVersion: "0.5.0",
            buildVersion: "5",
            bundleIdentifier: "craft.semantic.micro-input.dev",
            feedURLString: validFeed,
            publicEDKey: "c2hvcnQ="
        )
        if case .unavailable = UpdateLaunchPolicy.evaluate(
            identity: badKeyIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: [:]
        ) {} else {
            XCTFail("Expected short Ed25519 key to mark updater unavailable")
        }

        var factoryCalls = 0
        let fakeDriver = FakeSoftwareUpdateDriver()
        let isolatedController = SoftwareUpdateController(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: ["MI_DISABLE_UPDATER": "1"],
            driverFactory: {
                factoryCalls += 1
                return fakeDriver
            }
        )
        isolatedController.startIfPermitted()
        isolatedController.setAutomaticallyChecks(true)
        isolatedController.checkForUpdates()
        XCTAssertEqual(factoryCalls, 0)
        XCTAssertFalse(isolatedController.canCheckForUpdates)
        XCTAssertFalse(isolatedController.automaticallyChecksForUpdates)

        let readyController = SoftwareUpdateController(
            identity: prodIdentity,
            bundlePath: "/Applications/Micro Input.app",
            environment: [:],
            arguments: [],
            driverFactory: {
                factoryCalls += 1
                return fakeDriver
            }
        )
        XCTAssertEqual(readyController.serviceStatus, .ready)
        XCTAssertEqual(factoryCalls, 0)

        readyController.startIfPermitted()
        readyController.startIfPermitted()
        XCTAssertEqual(factoryCalls, 1)
        XCTAssertEqual(fakeDriver.startCalls, 1)
        XCTAssertTrue(readyController.canCheckForUpdates)
        XCTAssertTrue(readyController.automaticallyChecksForUpdates)

        readyController.setAutomaticallyChecks(false)
        XCTAssertFalse(fakeDriver.automaticallyChecksForUpdates)
        XCTAssertFalse(readyController.automaticallyChecksForUpdates)

        let fixedDate = Date(timeIntervalSince1970: 1_762_000_000)
        fakeDriver.lastUpdateCheckDate = fixedDate
        readyController.checkForUpdates()
        XCTAssertEqual(fakeDriver.checkCalls, 1)
        XCTAssertEqual(readyController.lastUpdateCheckDate, fixedDate)
        XCTAssertTrue(readyController.lastCheckSummary.hasPrefix("上次检查："))

        let nav = WorkbenchNavigationModel()
        XCTAssertEqual(nav.destination, .keyboard)
        nav.openGeneral()
        XCTAssertEqual(nav.destination, .general)

        let dir = try makeIsolatedTempDir("engine-version")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertEqual(store.header.engineVersion, "0.2.0")
    }

    func testUpdateLaunchPolicyResolvesSymlinksRejectsEphemeralBundlesAndWrongBundleId() throws {
        let validFeed = "https://github.com/semantic-craft/micro-input/releases/latest/download/appcast.xml"
        let validKey = Data(repeating: 7, count: 32).base64EncodedString()
        let prodIdentity = AppReleaseIdentity(
            marketingVersion: "0.5.0",
            buildVersion: "5",
            bundleIdentifier: LaunchAtLoginPolicy.productionBundleIdentifier,
            feedURLString: validFeed,
            publicEDKey: validKey
        )

        let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent(".mi-update-symlink-\(UUID().uuidString)")
        let ephemeralTarget = FileManager.default.temporaryDirectory
            .appendingPathComponent("mi-ephemeral-upd-\(UUID().uuidString)/Micro Input.app")
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: ephemeralTarget.deletingLastPathComponent())
        }
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: ephemeralTarget, withIntermediateDirectories: true)
        let symlinkApp = root.appendingPathComponent("Micro Input.app")
        try FileManager.default.createSymbolicLink(at: symlinkApp, withDestinationURL: ephemeralTarget)

        if case .isolated(reason: .ephemeralBuildBundle) = UpdateLaunchPolicy.evaluate(
            identity: prodIdentity,
            bundlePath: symlinkApp.path,
            environment: [:],
            arguments: []
        ) {} else {
            XCTFail("Expected symlink to ephemeral /var/folders or /tmp .app bundle to isolate updater")
        }

        for ephemeralAppPath in [
            "/tmp/Micro Input.app",
            "/private/tmp/Micro Input.app",
            "/var/folders/xx/yy/T/Micro Input.app",
            "/Users/example/work/.scratch/qa-runs/1/Micro Input.app",
        ] {
            if case .isolated(reason: .ephemeralBuildBundle) = UpdateLaunchPolicy.evaluate(
                identity: prodIdentity,
                bundlePath: ephemeralAppPath,
                environment: [:],
                arguments: []
            ) {} else {
                XCTFail("Expected \(ephemeralAppPath) to isolate updater as ephemeralBuildBundle")
            }
        }

        let foreignIdentity = AppReleaseIdentity(
            marketingVersion: "0.5.0",
            buildVersion: "5",
            bundleIdentifier: "com.example.other",
            feedURLString: validFeed,
            publicEDKey: validKey
        )
        XCTAssertEqual(
            UpdateLaunchPolicy.evaluate(
                identity: foreignIdentity,
                bundlePath: "/Applications/Micro Input.app",
                environment: [:],
                arguments: []
            ),
            .unavailable(reason: .wrongBundleIdentifier(actual: "com.example.other"))
        )
    }

    @MainActor
    func testCoreStoreDeinitFromBackgroundAndMainThreadCleansUpSafely() throws {
        final class WeakStoreBox {
            weak var store: CoreStore?
            init(_ store: CoreStore?) { self.store = store }
        }
        final class SendableStoreTransfer: @unchecked Sendable {
            var store: CoreStore?
            init(_ store: CoreStore?) { self.store = store }
        }

        let dir = try makeIsolatedTempDir("corestore-deinit")
        defer { try? FileManager.default.removeItem(at: dir) }

        var mainStore: CoreStore? = CoreStore(isolatedDirectory: dir.path, pollInIsolation: true)
        let mainBox = WeakStoreBox(mainStore)
        XCTAssertNotNil(mainBox.store)
        mainStore = nil
        XCTAssertNil(mainBox.store)

        let bgTransfer = SendableStoreTransfer(CoreStore(isolatedDirectory: dir.path, pollInIsolation: true))
        let bgBox = WeakStoreBox(bgTransfer.store)
        XCTAssertNotNil(bgBox.store)

        let releasedOnBg = XCTestExpectation(description: "Released CoreStore off main thread")
        DispatchQueue.global(qos: .utility).async {
            bgTransfer.store = nil
            releasedOnBg.fulfill()
        }
        wait(for: [releasedOnBg], timeout: 5.0)

        let mainDrained = XCTestExpectation(description: "Main queue processed deferred teardown")
        DispatchQueue.main.async {
            mainDrained.fulfill()
        }
        wait(for: [mainDrained], timeout: 5.0)
        XCTAssertNil(bgBox.store)
    }

    @MainActor
    func testProfileLockAndAccessibilitySlicesRoundtrip() throws {
        let dir = try makeIsolatedTempDir("profile-lock-slices")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = CoreStore(isolatedDirectory: dir.path)

        XCTAssertFalse(store.sidebar.isProfileLocked)
        XCTAssertNil(store.sidebar.lockedProfileId)
        XCTAssertTrue(store.header.accessibilityTrusted)
        XCTAssertTrue(store.statusMenu.accessibilityTrusted)

        let rows = store.sidebar.sidebarRows
        XCTAssertGreaterThanOrEqual(rows.count, 2)
        let firstId = rows[0].profileId
        let secondId = rows[1].profileId

        store.dispatch("sidebar", ["BrowseProfile": firstId])
        store.dispatch("sidebar", ["SetProfileLocked": true])
        XCTAssertTrue(store.sidebar.isProfileLocked)
        XCTAssertTrue(store.statusMenu.isProfileLocked)
        XCTAssertEqual(store.sidebar.lockedProfileId, firstId)
        XCTAssertEqual(store.statusMenu.lockedProfileId, firstId)
        XCTAssertEqual(store.statusMenu.routingModeSummary, "已锁定 · 忽略前台切换与外部干扰")

        store.dispatch("sidebar", ["SelectProfile": secondId])
        XCTAssertTrue(store.sidebar.isProfileLocked)
        XCTAssertEqual(store.sidebar.lockedProfileId, secondId)
        XCTAssertEqual(store.statusMenu.lockedProfileId, secondId)

        store.dispatch("status_menu", ["SetProfileLocked": false])
        XCTAssertFalse(store.sidebar.isProfileLocked)
        XCTAssertFalse(store.statusMenu.isProfileLocked)
        XCTAssertNil(store.statusMenu.lockedProfileId)

        let activeHostBefore = store.sidebar.activeHostProfileId
        let activeNameBefore = store.statusMenu.activeProfileName
        for bundleId in ["com.openai.codex", "com.deepseek.dsh"] {
            store.dispatchRaw([
                "domain": "foreground_app_changed",
                "action": ["bundle_id": bundleId]
            ])
            XCTAssertEqual(store.sidebar.activeHostProfileId, activeHostBefore)
            XCTAssertEqual(store.statusMenu.activeProfileName, activeNameBefore)
        }

        store.dispatch("status_menu", ["SetProfileLocked": true])
        XCTAssertTrue(store.statusMenu.isProfileLocked)
        store.dispatch("status_menu", ["SetProfileLocked": false])
        XCTAssertFalse(store.statusMenu.isProfileLocked)

        store.dispatch("status_menu", ["SetHostExecutionEnabled": false])
        XCTAssertFalse(store.statusMenu.hostExecutionEnabled)
        XCTAssertEqual(store.statusMenu.routingModeSummary, "已暂停软件按键映射")
        store.dispatch("status_menu", ["SetHostExecutionEnabled": true])
        XCTAssertTrue(store.statusMenu.hostExecutionEnabled)
        XCTAssertEqual(store.statusMenu.routingModeSummary, "手动选择方案 · 按键映射已启用")

        store.dispatch("status_menu", "OpenAccessibilitySettings")
        XCTAssertEqual(store.footer.statusMessage, "已打开 macOS 辅助功能权限设置")
        store.dispatch("header", "OpenAccessibilitySettings")
        XCTAssertEqual(store.footer.statusMessage, "已打开 macOS 辅助功能权限设置")
    }

    func testAgentProfileKindMapsCodexVariantsToLiveSessionSlots() {
        for name in ["Codex", "Codex · Micro Input", "Codex · 原生"] {
            let kind = AgentProfileKind.from(profileName: name)
            XCTAssertEqual(kind, .codex)
            XCTAssertTrue(kind.supportsLiveSessionSlots)
        }

        let custom = AgentProfileKind.from(profileName: "Custom")
        XCTAssertEqual(custom, .custom)
        XCTAssertFalse(custom.supportsLiveSessionSlots)

        for name in ["Grok Bot", "GrokBot", "grokbot"] {
            let kind = AgentProfileKind.from(profileName: name)
            XCTAssertEqual(kind, .grokBot)
            XCTAssertFalse(kind.supportsLiveSessionSlots)
        }
        let agent = DesktopAgent.named("Grok Bot")
        XCTAssertEqual(agent?.bundleID, "com.anysphere.sand")
        XCTAssertEqual(agent?.symbol, "waveform")
        XCTAssertNil(DesktopAgent.named("Herdr"))
    }

    @MainActor
    func testGrokBotProfileProjectsThreeKeyStarterLayout() throws {
        let dir = try makeIsolatedTempDir("grokbot-starter")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let grokRow = try XCTUnwrap(store.model.sidebarRows.first(where: { $0.displayName == "Grok Bot" }))
        XCTAssertEqual(grokRow.badgeMonogram, "GB")
        store.dispatch("sidebar", ["SelectProfile": grokRow.profileId])

        let byBinding = Dictionary(uniqueKeysWithValues: store.canvas.keycapCells.map { ($0.bindingId, $0) })
        XCTAssertEqual(byBinding["AG06"]?.keycapId, "Party")
        XCTAssertEqual(byBinding["AG06"]?.isAvailable, true)
        XCTAssertEqual(byBinding["AG11"]?.keycapId, "Mic")
        XCTAssertEqual(byBinding["AG11"]?.isAvailable, true)
        XCTAssertEqual(byBinding["AG12"]?.keycapId, "Play")
        XCTAssertEqual(byBinding["AG12"]?.isAvailable, true)
        for binding in ["AG00", "AG01", "AG02", "AG05", "AG07", "AG08", "AG09", "AG10"] {
            XCTAssertEqual(byBinding[binding]?.isAvailable, false, "Grok Bot \(binding) should remain unbound")
        }
    }

    @MainActor
    func testCodexDialPresetsAndGlyphInference() throws {
        let dir = try makeIsolatedTempDir("codex-dial")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let codexRow = try XCTUnwrap(store.model.sidebarRows.first(where: { $0.displayName == "Codex" }))
        store.dispatch("sidebar", ["SelectProfile": codexRow.profileId])
        store.dispatch("canvas", "UnlockBySwitchingOrCreatingCustomLayer")

        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CodexReasoningEffortAndSpeed"])
        XCTAssertEqual(store.canvas.encoderCcwLabel, "Codex · 降低推理档位")
        XCTAssertEqual(store.canvas.encoderCwLabel, "Codex · 提高推理档位")
        XCTAssertEqual(store.canvas.encoderPressLabel, "Fast")

        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CodexScrollChatAndBottom"])
        XCTAssertEqual(store.canvas.encoderCcwLabel, "Codex · 向上滚动对话")
        XCTAssertEqual(store.canvas.encoderCwLabel, "Codex · 向下滚动对话")
        XCTAssertEqual(store.canvas.encoderPressLabel, "跳到底部")

        XCTAssertEqual(CodexKeycapId(rustVariant: "MindPlus"), .mindPlus)
        XCTAssertEqual(CodexKeycapId(rustVariant: "MindMinus"), .mindMinus)
        XCTAssertEqual(CodexKeycapId(rustVariant: "Fast"), .fast)
        XCTAssertEqual(CodexKeycapId(rustVariant: "Download"), .download)

        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "MousePointerAndScroll"])
        XCTAssertEqual(store.canvas.encoderCcwLabel, "鼠标滚轮向上")
        XCTAssertEqual(store.canvas.encoderCwLabel, "鼠标滚轮向下")
        XCTAssertEqual(store.canvas.encoderPressLabel, "切换档位")
    }

    @MainActor
    func testInspectorJoystickDualModeSwitchAndViewLayout() throws {
        let dir = try makeIsolatedTempDir("joystick-dual-mode")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let agRow = try XCTUnwrap(store.model.sidebarRows.first(where: { $0.displayName == "Antigravity" }))
        store.dispatch("sidebar", ["SelectProfile": agRow.profileId])
        store.dispatch("canvas", ["SelectSurface": "Joystick"])

        XCTAssertEqual(store.inspector.editableInspector?.joystickMode, "logical_sectors")
        XCTAssertFalse(store.inspector.editableInspector?.joystickModeSummary.isEmpty ?? true)
        XCTAssertEqual(store.canvas.joystickModeLabel, "8向摇杆")

        let renderHost = {
            let view = CreatorMicroInlineInspectorView(
                store: store,
                agentKind: .antigravity,
                onOpenFullActionLibrary: {},
                onOpenMacroLibrary: {}
            )
            let host = NSHostingView(rootView: view.frame(width: 340, height: 640))
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 200)
            XCTAssertLessThanOrEqual(host.fittingSize.width, 340.5)
        }
        renderHost()

        store.dispatch("inspector", ["SetJoystickMode": "mouse"])
        XCTAssertEqual(store.inspector.editableInspector?.joystickMode, "mouse")
        XCTAssertEqual(store.canvas.joystickModeLabel, "硬件指针")
        renderHost()

        store.dispatch("inspector", ["SetJoystickMode": "scroll"])
        XCTAssertEqual(store.inspector.editableInspector?.joystickMode, "scroll")
        XCTAssertEqual(store.canvas.joystickModeLabel, "硬件滚轮")

        store.dispatch("inspector", ["SetJoystickMode": "source"])
        XCTAssertEqual(store.inspector.editableInspector?.joystickMode, "source")
        XCTAssertEqual(store.canvas.joystickModeLabel, "8向摇杆")

        store.dispatch("inspector", ["SetJoystickMode": "logical_sectors"])
        XCTAssertEqual(store.inspector.editableInspector?.joystickMode, "logical_sectors")
        XCTAssertEqual(store.canvas.joystickModeLabel, "8向摇杆")
        renderHost()
    }

    @MainActor
    func testPowerStatusAndCandidateDeviceSliceIsolationAndDecoding() throws {
        let withSerial = CandidateDeviceModel(path: "/dev/hid1", product: "Creator Micro 2", serialNumber: "  SN-01 ", displayLabel: "CM2 · SN-01")
        XCTAssertEqual(withSerial.selectorKey, "SN-01")
        XCTAssertEqual(withSerial.id, "SN-01")

        let noSerial = CandidateDeviceModel(path: "IOService:/USB/1", product: "Creator Micro 2", serialNumber: "  ")
        XCTAssertEqual(noSerial.selectorKey, "IOService:/USB/1")

        let badgeJson: [String: Any] = [
            "battery_percent": 88,
            "is_charging": true,
            "line_powered": true,
            "usb_host_hold": true,
            "usb_mode": "usb.host 保持",
            "pm_mode": "idle",
            "host_sleep_status": "awake",
            "summary_label": "88% 充电中 · usb.host 保持 · 待机",
        ]
        let candidatesJson: [[String: Any]] = [
            [
                "path": "IOService:/USB/Alpha",
                "product": "Creator Micro 2",
                "serial_number": "SN-ALPHA",
                "display_label": "Creator Micro 2 (SN-ALPHA)",
                "is_connected": true,
                "is_preferred": true,
            ],
            [
                "path": "IOService:/USB/Beta",
                "product": "Creator Micro 2",
                "serial_number": "SN-BETA",
                "display_label": "Creator Micro 2 (SN-BETA)",
                "is_connected": false,
                "is_preferred": false,
            ],
        ]

        let rootJson: [String: Any] = [
            "header": [
                "engine_version": "0.2.0",
                "power_status": badgeJson,
                "candidate_devices": candidatesJson,
                "preferred_device_serial": "SN-ALPHA",
            ],
            "status_menu": [
                "active_profile_name": "Antigravity",
                "power_status": badgeJson,
                "candidate_devices": candidatesJson,
                "preferred_device_serial": "SN-ALPHA",
            ],
        ]
        let parsed = CoreStore.parseProjection(rootJson)
        let headerSlice = HeaderSliceModel(from: parsed)
        let statusSlice = StatusMenuSliceModel(from: parsed)
        XCTAssertEqual(headerSlice.powerStatus?.summaryLabel, "88% 充电中 · usb.host 保持 · 待机")
        XCTAssertEqual(statusSlice.powerStatus, headerSlice.powerStatus)
        XCTAssertEqual(headerSlice.candidateDevices.count, 2)
        XCTAssertEqual(statusSlice.candidateDevices, headerSlice.candidateDevices)
        XCTAssertEqual(headerSlice.preferredDeviceSerial, "SN-ALPHA")
        XCTAssertEqual(statusSlice.preferredDeviceSerial, "SN-ALPHA")

        let baseSidebar = SidebarSliceModel(from: parsed)
        let baseCanvas = CanvasSliceModel(from: parsed)
        let baseOverlay = OverlaySliceModel(from: parsed)
        let baseFooter = FooterSliceModel(from: parsed)

        var mutated = parsed
        mutated.powerStatus?.summaryLabel = "92% · 跟随宿主"
        mutated.preferredDeviceSerial = "SN-BETA"
        XCTAssertNotEqual(HeaderSliceModel(from: mutated), headerSlice)
        XCTAssertNotEqual(StatusMenuSliceModel(from: mutated), statusSlice)
        XCTAssertEqual(SidebarSliceModel(from: mutated), baseSidebar)
        XCTAssertEqual(CanvasSliceModel(from: mutated), baseCanvas)
        XCTAssertEqual(OverlaySliceModel(from: mutated), baseOverlay)
        XCTAssertEqual(FooterSliceModel(from: mutated), baseFooter)

        let stripHost = NSHostingView(
            rootView: StatusMenuHardwareStrip(
                powerStatus: parsed.powerStatus,
                candidates: parsed.candidateDevices,
                onSelect: { _ in }
            )
            .frame(width: 320)
        )
        stripHost.layoutSubtreeIfNeeded()
        XCTAssertLessThanOrEqual(stripHost.fittingSize.width, 320.5)
        XCTAssertGreaterThan(stripHost.fittingSize.height, 20)

        let narrowStripHost = NSHostingView(
            rootView: StatusMenuHardwareStrip(
                powerStatus: parsed.powerStatus,
                candidates: parsed.candidateDevices,
                onSelect: { _ in }
            )
            .frame(width: 240)
        )
        narrowStripHost.layoutSubtreeIfNeeded()
        XCTAssertLessThanOrEqual(narrowStripHost.fittingSize.width, 240.5)
        XCTAssertGreaterThan(narrowStripHost.fittingSize.height, 44)
    }

    @MainActor
    func testHonestEquatableMultiTargetDtoAndHeaderAuthority() throws {
        let dir = try makeIsolatedTempDir("honest-equatable")
        defer { try? FileManager.default.removeItem(at: dir) }

        let baseMulti = MultiEditorModel(
            multiId: "m-1",
            name: "TapHold",
            termMs: 200,
            branches: [
                MultiBranchModel(
                    branch: "Tap",
                    title: "短按",
                    targetKind: "Action",
                    targetId: "act-1",
                    targetDisplayName: "复制"
                )
            ],
            bindToSelectedSlot: true,
            targetSlot: 0,
            availableActions: [MultiTargetOptionModel(id: "act-1", label: "复制")],
            availableMacros: [MultiTargetOptionModel(id: "mac-1", label: "宏一")],
            referenceSummaries: ["AG00"],
            validationError: nil
        )
        XCTAssertNotEqual(
            baseMulti,
            MultiEditorModel(
                multiId: baseMulti.multiId,
                name: baseMulti.name,
                termMs: baseMulti.termMs,
                branches: baseMulti.branches,
                bindToSelectedSlot: baseMulti.bindToSelectedSlot,
                targetSlot: baseMulti.targetSlot,
                availableActions: [MultiTargetOptionModel(id: "act-2", label: "粘贴")],
                availableMacros: baseMulti.availableMacros,
                referenceSummaries: baseMulti.referenceSummaries,
                validationError: baseMulti.validationError
            )
        )
        XCTAssertNotEqual(
            baseMulti,
            MultiEditorModel(
                multiId: baseMulti.multiId,
                name: baseMulti.name,
                termMs: baseMulti.termMs,
                branches: baseMulti.branches,
                bindToSelectedSlot: baseMulti.bindToSelectedSlot,
                targetSlot: baseMulti.targetSlot,
                availableActions: baseMulti.availableActions,
                availableMacros: [MultiTargetOptionModel(id: "mac-2", label: "宏二")],
                referenceSummaries: baseMulti.referenceSummaries,
                validationError: baseMulti.validationError
            )
        )
        XCTAssertNotEqual(
            baseMulti,
            MultiEditorModel(
                multiId: baseMulti.multiId,
                name: baseMulti.name,
                termMs: baseMulti.termMs,
                branches: baseMulti.branches,
                bindToSelectedSlot: false,
                targetSlot: baseMulti.targetSlot,
                availableActions: baseMulti.availableActions,
                availableMacros: baseMulti.availableMacros,
                referenceSummaries: baseMulti.referenceSummaries,
                validationError: baseMulti.validationError
            )
        )
        XCTAssertNotEqual(
            baseMulti,
            MultiEditorModel(
                multiId: baseMulti.multiId,
                name: baseMulti.name,
                termMs: baseMulti.termMs,
                branches: baseMulti.branches,
                bindToSelectedSlot: baseMulti.bindToSelectedSlot,
                targetSlot: 2,
                availableActions: baseMulti.availableActions,
                availableMacros: baseMulti.availableMacros,
                referenceSummaries: baseMulti.referenceSummaries,
                validationError: baseMulti.validationError
            )
        )
        XCTAssertNotEqual(
            baseMulti,
            MultiEditorModel(
                multiId: baseMulti.multiId,
                name: baseMulti.name,
                termMs: baseMulti.termMs,
                branches: baseMulti.branches,
                bindToSelectedSlot: baseMulti.bindToSelectedSlot,
                targetSlot: baseMulti.targetSlot,
                availableActions: baseMulti.availableActions,
                availableMacros: baseMulti.availableMacros,
                referenceSummaries: ["AG00", "AG01"],
                validationError: baseMulti.validationError
            )
        )

        let baseSlot = SessionSlotModel(
            slotIndex: 0,
            keyBindingId: "AG00",
            mode: "pinned",
            status: "idle",
            kind: "codex",
            agentMonogram: "CX",
            title: "Session 0",
            subtitle: "Ready",
            isUnreadBlueDot: false,
            isWorkingAnimated: false,
            isBoundToTopKey: true,
            ledColorHex: "#14C76B",
            hasAssignedPlace: false
        )
        XCTAssertNotEqual(
            baseSlot,
            SessionSlotModel(
                slotIndex: baseSlot.slotIndex,
                keyBindingId: baseSlot.keyBindingId,
                mode: baseSlot.mode,
                status: baseSlot.status,
                kind: baseSlot.kind,
                agentMonogram: "AG",
                title: baseSlot.title,
                subtitle: baseSlot.subtitle,
                isUnreadBlueDot: baseSlot.isUnreadBlueDot,
                isWorkingAnimated: baseSlot.isWorkingAnimated,
                isBoundToTopKey: baseSlot.isBoundToTopKey,
                ledColorHex: baseSlot.ledColorHex,
                hasAssignedPlace: baseSlot.hasAssignedPlace
            )
        )
        XCTAssertNotEqual(
            baseSlot,
            SessionSlotModel(
                slotIndex: baseSlot.slotIndex,
                keyBindingId: baseSlot.keyBindingId,
                mode: baseSlot.mode,
                status: baseSlot.status,
                kind: baseSlot.kind,
                agentMonogram: baseSlot.agentMonogram,
                title: baseSlot.title,
                subtitle: baseSlot.subtitle,
                isUnreadBlueDot: baseSlot.isUnreadBlueDot,
                isWorkingAnimated: baseSlot.isWorkingAnimated,
                isBoundToTopKey: baseSlot.isBoundToTopKey,
                ledColorHex: baseSlot.ledColorHex,
                hasAssignedPlace: true
            )
        )

        let store = CoreStore(isolatedDirectory: dir.path)
        func makeMultiRoot(_ actions: [[String: String]]) -> [String: Any] {
            [
                "active_modal": [
                    "MultiEditor": [
                        "multi_id": "m-1",
                        "name": "TapHold",
                        "term_ms": 200,
                        "branches": [],
                        "bind_to_selected_slot": true,
                        "target_slot": 0,
                        "available_actions": actions,
                        "available_macros": [["id": "mac-1", "label": "宏一"]],
                        "reference_summaries": ["AG00"],
                    ]
                ]
            ]
        }

        store.commitDomainSlices(from: CoreStore.parseProjection(makeMultiRoot([
            ["id": "act-1", "label": "复制"]
        ])))
        guard case let .multiEditor(firstModal)? = store.activeModal else {
            return XCTFail("Expected multiEditor activeModal after first commit")
        }
        XCTAssertEqual(firstModal.availableActions, [MultiTargetOptionModel(id: "act-1", label: "复制")])

        store.commitDomainSlices(from: CoreStore.parseProjection(makeMultiRoot([
            ["id": "act-1", "label": "复制"],
            ["id": "act-2", "label": "粘贴"],
        ])))
        guard case let .multiEditor(updatedModal)? = store.activeModal else {
            return XCTFail("Expected multiEditor activeModal after available_actions change")
        }
        XCTAssertEqual(
            updatedModal.availableActions,
            [
                MultiTargetOptionModel(id: "act-1", label: "复制"),
                MultiTargetOptionModel(id: "act-2", label: "粘贴"),
            ]
        )

        let headerPower: [String: Any] = [
            "battery_percent": 91,
            "is_charging": false,
            "line_powered": false,
            "usb_host_hold": false,
            "usb_mode": "跟随宿主",
            "pm_mode": "awake",
            "host_sleep_status": "awake",
            "summary_label": "91% · header",
        ]
        let statusMenuPower: [String: Any] = [
            "battery_percent": 12,
            "is_charging": true,
            "line_powered": true,
            "usb_host_hold": true,
            "usb_mode": "usb.host 保持",
            "pm_mode": "sleep",
            "host_sleep_status": "sleep",
            "summary_label": "12% · status_menu",
        ]
        let parsed = CoreStore.parseProjection([
            "header": [
                "engine_version": "0.2.0",
                "accessibility_trusted": true,
                "power_status": headerPower,
                "candidate_devices": [
                    [
                        "path": "/dev/hid-header",
                        "product": "Creator Micro 2",
                        "serial_number": "SN-1",
                        "selector_key": "override-key",
                        "display_label": "Header Device",
                        "is_connected": true,
                        "is_preferred": true,
                    ]
                ],
                "preferred_device_serial": "override-key",
            ],
            "sidebar": [
                "accessibility_trusted": false,
            ],
            "status_menu": [
                "accessibility_trusted": false,
                "power_status": statusMenuPower,
                "candidate_devices": [
                    [
                        "path": "/dev/hid-stale",
                        "product": "Creator Micro 2",
                        "serial_number": "SN-STALE",
                        "selector_key": "stale-key",
                        "display_label": "Stale Device",
                        "is_connected": false,
                        "is_preferred": false,
                    ]
                ],
                "preferred_device_serial": "stale-key",
            ],
            "inspector": [
                "body": [
                    "Editable": [:],
                ]
            ],
        ])

        XCTAssertEqual(parsed.editableInspector?.bottomRowLayout, "split")
        XCTAssertEqual(parsed.candidateDevices.first?.selectorKey, "override-key")
        XCTAssertEqual(parsed.accessibilityTrusted, true)
        XCTAssertEqual(parsed.powerStatus?.summaryLabel, "91% · header")
        XCTAssertEqual(parsed.preferredDeviceSerial, "override-key")

        let headerSlice = HeaderSliceModel(from: parsed)
        let statusSlice = StatusMenuSliceModel(from: parsed)
        XCTAssertEqual(headerSlice.accessibilityTrusted, true)
        XCTAssertEqual(headerSlice.powerStatus?.summaryLabel, "91% · header")
        XCTAssertEqual(headerSlice.preferredDeviceSerial, "override-key")
        XCTAssertEqual(headerSlice.candidateDevices.first?.selectorKey, "override-key")
        XCTAssertEqual(statusSlice.powerStatus?.summaryLabel, "91% · header")
        XCTAssertEqual(statusSlice.preferredDeviceSerial, "override-key")
        XCTAssertEqual(statusSlice.candidateDevices.first?.selectorKey, "override-key")
    }

    @MainActor
    func testEncoderHoldModelSelectionPresetAndJoystickMouseModeRoundtrip() throws {
        let dir = try makeIsolatedTempDir("encoder-hold-joystick-mouse")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = CoreStore(isolatedDirectory: dir.path)

        XCTAssertEqual(store.canvas.encoderHoldLabel, "搜索会话")
        XCTAssertEqual(store.canvas.encoderPressLabel, "搜索会话")

        store.dispatch("canvas", ["SelectSurface": "EncoderHold"])
        XCTAssertEqual(store.canvas.selectedSurface, "EncoderHold")
        XCTAssertEqual(store.inspector.selectedSurface, "EncoderHold")
        XCTAssertEqual(store.inspector.editableInspector?.boundActionId, "agent-search")

        store.dispatch("inspector", "UnassignSelectedSlot")
        XCTAssertEqual(store.canvas.encoderHoldLabel, "未绑定")
        XCTAssertEqual(store.canvas.encoderPressLabel, "搜索会话")

        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "ModelSelectionAndMouse"])
        XCTAssertEqual(store.canvas.encoderHoldLabel, "搜索会话")
        XCTAssertEqual(store.canvas.encoderPressLabel, "搜索会话")

        store.dispatch("canvas", ["SelectSurface": "Joystick"])
        store.dispatch("inspector", ["SetJoystickMode": "mouse"])
        XCTAssertEqual(store.canvas.joystickModeLabel, "硬件指针")
        XCTAssertEqual(store.inspector.editableInspector?.joystickMode, "mouse")
        XCTAssertEqual(store.canvas.encoderHoldLabel, "搜索会话")
    }

    @MainActor
    func testSecurityDefaultsFailClosedAndPresetImportStagesPreview() throws {
        let fallbackParsed = CoreStore.parseProjection([
            "inspector": [
                "body": [
                    "Editable": [:],
                ]
            ],
            "status_menu": [
                "pending_import": [
                    "raw_text": "{}",
                    "name": "Scoped Profile",
                    "scope": "profile",
                    "contains_complete_archive": true,
                    "profile_count": 1,
                    "layer_count": 2,
                    "action_count": 5,
                ],
                "preset_error": "invalid preset",
            ],
        ])
        XCTAssertEqual(
            fallbackParsed.editableInspector?.commandExecutionEnabled,
            false,
            "missing command_execution_enabled must fail closed to false"
        )
        XCTAssertEqual(fallbackParsed.pendingImport?.name, "Scoped Profile")
        XCTAssertEqual(fallbackParsed.pendingImport?.scope, "profile")
        XCTAssertEqual(fallbackParsed.pendingImport?.containsCompleteArchive, true)
        XCTAssertEqual(fallbackParsed.pendingImport?.profileCount, 1)
        XCTAssertEqual(fallbackParsed.pendingImport?.layerCount, 2)
        XCTAssertEqual(fallbackParsed.pendingImport?.actionCount, 5)
        XCTAssertEqual(fallbackParsed.presetError, "invalid preset")

        let statusSlice = StatusMenuSliceModel(from: fallbackParsed)
        XCTAssertEqual(statusSlice.pendingImport?.scope, "profile")
        XCTAssertEqual(statusSlice.presetError, "invalid preset")

        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("micro-input-swift-unit2-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let store = CoreStore(isolatedDirectory: tempDir.path)
        defer { store.stopEventLoops() }
        XCTAssertEqual(store.inspector.editableInspector?.commandExecutionEnabled, false)

        let selectedPid = store.sidebar.selectedProfileId
        store.dispatch("sidebar", [
            "Backup": [
                "ExportPreset": [
                    "profile_id": selectedPid as Any? ?? NSNull(),
                    "layer_id": NSNull(),
                ]
            ]
        ])
        let exported = try XCTUnwrap(store.sidebar.lastExportedPreset)
        store.stagePresetImport(data: Data(exported.utf8))
        let staged = try XCTUnwrap(store.statusMenu.pendingImport)
        XCTAssertEqual(staged.scope, "profile")
        XCTAssertTrue(staged.containsCompleteArchive)
        XCTAssertGreaterThan(staged.profileCount, 0)

        store.dispatch("status_menu", "CancelStagedPresetImport")
        XCTAssertNil(store.statusMenu.pendingImport)

        store.stagePresetImport(data: Data("not-json".utf8))
        XCTAssertNotNil(store.statusMenu.presetError)
        XCTAssertNil(store.statusMenu.pendingImport)

        store.stagePresetImport(data: Data(exported.utf8))
        XCTAssertNil(store.statusMenu.presetError)
        XCTAssertNotNil(store.statusMenu.pendingImport)
        store.dispatch("status_menu", "ConfirmStagedPresetImport")
        XCTAssertNil(store.statusMenu.pendingImport)
    }

    @MainActor
    func testStatusItemIconCautionCachingAndHostCautionSliceIsolation() throws {
        XCTAssertEqual(StatusItemIcon.presentation(hostExecutionEnabled: true, hasCaution: false), .active)
        XCTAssertEqual(StatusItemIcon.presentation(hostExecutionEnabled: false, hasCaution: false), .paused)
        XCTAssertEqual(StatusItemIcon.presentation(hostExecutionEnabled: true, hasCaution: true), .caution)
        XCTAssertEqual(StatusItemIcon.presentation(hostExecutionEnabled: false, hasCaution: true), .caution)

        XCTAssertEqual(
            StatusItemIcon.toolTip(locale: .zhHans, hostExecutionEnabled: true, hostCautionNotice: nil),
            "Micro Input 按键与会话控制"
        )
        XCTAssertEqual(
            StatusItemIcon.toolTip(locale: .en, hostExecutionEnabled: true, hostCautionNotice: nil),
            "Micro Input Keypad & Session Control"
        )
        XCTAssertEqual(
            StatusItemIcon.toolTip(locale: .zhHans, hostExecutionEnabled: false, hostCautionNotice: nil),
            "已暂停软件按键映射，点击菜单栏重新启用"
        )
        XCTAssertEqual(
            StatusItemIcon.toolTip(locale: .en, hostExecutionEnabled: false, hostCautionNotice: nil),
            "Key mapping paused. Click the menu bar icon to re-enable."
        )
        XCTAssertEqual(
            StatusItemIcon.toolTip(locale: .zhHans, hostExecutionEnabled: false, hostCautionNotice: "告警提示"),
            "告警提示"
        )
        XCTAssertEqual(
            StatusItemIcon.toolTip(locale: .en, hostExecutionEnabled: true, hostCautionNotice: "Caution notice"),
            "Caution notice"
        )

        let zhActive = StatusItemIcon.make(locale: .zhHans, presentation: .active)
        let zhActiveAgain = StatusItemIcon.make(locale: .zhHans)
        let zhPaused = StatusItemIcon.make(locale: .zhHans, presentation: .paused)
        let zhPausedAgain = StatusItemIcon.make(locale: .zhHans, presentation: .paused)
        let zhCaution = StatusItemIcon.make(locale: .zhHans, presentation: .caution)
        let zhCautionAgain = StatusItemIcon.make(locale: .zhHans, presentation: .caution)
        let enActive = StatusItemIcon.make(locale: .en, presentation: .active)
        let enPaused = StatusItemIcon.make(locale: .en, presentation: .paused)
        let enCaution = StatusItemIcon.make(locale: .en, presentation: .caution)

        XCTAssertTrue(zhActive === zhActiveAgain)
        XCTAssertTrue(zhPaused === zhPausedAgain)
        XCTAssertTrue(zhCaution === zhCautionAgain)
        XCTAssertTrue(enActive === StatusItemIcon.make(locale: .en))
        XCTAssertTrue(enPaused === StatusItemIcon.make(locale: .en, presentation: .paused))
        XCTAssertTrue(enCaution === StatusItemIcon.make(locale: .en, presentation: .caution))

        let allSix = [zhActive, zhPaused, zhCaution, enActive, enPaused, enCaution]
        for i in 0..<allSix.count {
            for j in (i + 1)..<allSix.count {
                XCTAssertFalse(allSix[i] === allSix[j])
            }
        }
        let descriptions = Set(allSix.compactMap(\.accessibilityDescription))
        XCTAssertEqual(descriptions.count, 6)

        for icon in allSix {
            XCTAssertTrue(icon.isTemplate)
            XCTAssertFalse((icon.accessibilityDescription ?? "").isEmpty)
        }
        XCTAssertNotEqual(zhActive.tiffRepresentation, zhPaused.tiffRepresentation)
        XCTAssertNotEqual(zhActive.tiffRepresentation, zhCaution.tiffRepresentation)
        XCTAssertNotEqual(zhPaused.tiffRepresentation, zhCaution.tiffRepresentation)
        XCTAssertNotEqual(enActive.tiffRepresentation, enPaused.tiffRepresentation)
        XCTAssertNotEqual(enActive.tiffRepresentation, enCaution.tiffRepresentation)
        XCTAssertNotEqual(enPaused.tiffRepresentation, enCaution.tiffRepresentation)

        let baseParsed = CoreStore.parseProjection([
            "header": [
                "engine_version": "0.2.0",
                "can_retry_host_cleanup": false,
            ],
            "status_menu": [
                "active_profile_name": "Antigravity",
                "can_retry_host_cleanup": false,
            ],
        ])
        let baseHeader = HeaderSliceModel(from: baseParsed)
        let baseSidebar = SidebarSliceModel(from: baseParsed)
        let baseCanvas = CanvasSliceModel(from: baseParsed)
        let baseSession = SessionStripSliceModel(from: baseParsed)
        let baseInspector = InspectorSliceModel(from: baseParsed)
        let baseStatusMenu = StatusMenuSliceModel(from: baseParsed)
        let baseOverlay = OverlaySliceModel(from: baseParsed)
        let baseFooter = FooterSliceModel(from: baseParsed)

        let transientParsed = CoreStore.parseProjection([
            "header": [
                "engine_version": "0.2.0",
                "can_retry_host_cleanup": false,
            ],
            "status_menu": [
                "active_profile_name": "Antigravity",
                "host_caution_notice": "上次听写所在窗口或对话已切换，请先确认原录音已停止",
                "can_retry_host_cleanup": false,
            ],
        ])
        let transientStatusMenu = StatusMenuSliceModel(from: transientParsed)
        XCTAssertEqual(transientStatusMenu.hostCautionNotice, "上次听写所在窗口或对话已切换，请先确认原录音已停止")
        XCTAssertFalse(transientStatusMenu.canRetryHostCleanup)
        XCTAssertNotEqual(transientStatusMenu, baseStatusMenu)
        XCTAssertEqual(HeaderSliceModel(from: transientParsed), baseHeader)
        XCTAssertEqual(SidebarSliceModel(from: transientParsed), baseSidebar)
        XCTAssertEqual(CanvasSliceModel(from: transientParsed), baseCanvas)
        XCTAssertEqual(SessionStripSliceModel(from: transientParsed), baseSession)
        XCTAssertEqual(InspectorSliceModel(from: transientParsed), baseInspector)
        XCTAssertEqual(OverlaySliceModel(from: transientParsed), baseOverlay)
        XCTAssertEqual(FooterSliceModel(from: transientParsed), baseFooter)

        let stickyParsed = CoreStore.parseProjection([
            "header": [
                "engine_version": "0.2.0",
                "can_retry_host_cleanup": true,
            ],
            "status_menu": [
                "active_profile_name": "Antigravity",
                "host_caution_notice": "Previous dictation window or chat changed; confirm recording stopped",
                "can_retry_host_cleanup": true,
            ],
        ])
        XCTAssertTrue(HeaderSliceModel(from: stickyParsed).canRetryHostCleanup)
        XCTAssertTrue(StatusMenuSliceModel(from: stickyParsed).canRetryHostCleanup)
        XCTAssertEqual(
            StatusMenuSliceModel(from: stickyParsed).hostCautionNotice,
            "Previous dictation window or chat changed; confirm recording stopped"
        )
        XCTAssertEqual(SidebarSliceModel(from: stickyParsed), baseSidebar)
        XCTAssertEqual(CanvasSliceModel(from: stickyParsed), baseCanvas)
        XCTAssertEqual(SessionStripSliceModel(from: stickyParsed), baseSession)
        XCTAssertEqual(InspectorSliceModel(from: stickyParsed), baseInspector)
        XCTAssertEqual(OverlaySliceModel(from: stickyParsed), baseOverlay)
        XCTAssertEqual(FooterSliceModel(from: stickyParsed), baseFooter)

        let statusOnlyParsed = CoreStore.parseProjection([
            "status_menu": [
                "host_caution_notice": "听写状态回读未确认，请检查原窗口是否仍在录音",
                "can_retry_host_cleanup": true,
            ],
        ])
        XCTAssertTrue(StatusMenuSliceModel(from: statusOnlyParsed).canRetryHostCleanup)
        XCTAssertEqual(
            StatusMenuSliceModel(from: statusOnlyParsed).hostCautionNotice,
            "听写状态回读未确认，请检查原窗口是否仍在录音"
        )

        let dir = try makeIsolatedTempDir("retry-cleanup-dispatch")
        defer { try? FileManager.default.removeItem(at: dir) }
        let store = CoreStore(isolatedDirectory: dir.path)
        defer { store.stopEventLoops() }

        store.commitDomainSlices(from: stickyParsed)
        XCTAssertTrue(store.statusMenu.canRetryHostCleanup)
        XCTAssertTrue(store.header.canRetryHostCleanup)

        let popoverHost = NSHostingView(
            rootView: StatusMenuPopoverView(store: store, onOpenMainWindow: {}).frame(width: 348)
        )
        popoverHost.layoutSubtreeIfNeeded()
        XCTAssertLessThanOrEqual(popoverHost.fittingSize.width, 348.5)
        XCTAssertGreaterThan(popoverHost.fittingSize.height, 260)

        store.dispatch("status_menu", "RetryHostCleanup")
        XCTAssertFalse(store.statusMenu.canRetryHostCleanup)
        XCTAssertNil(store.statusMenu.hostCautionNotice)
        store.dispatch("header", "RetryHostCleanup")
        XCTAssertFalse(store.header.canRetryHostCleanup)
        XCTAssertEqual(L10n.tr("重试清理", .en), "Retry Cleanup")
    }

    @MainActor
    func testLightingEditorPreviewRoundtripOfflineAndLocaleSwitch() throws {
        let dir = try makeIsolatedTempDir("lighting-preview-roundtrip")
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        defer { store.stopEventLoops() }

        XCTAssertFalse(store.sidebar.isProtectedLayer)
        store.dispatch("editor", "OpenLightingEditor")
        guard case .lightingEditor(var lighting) = store.activeModal else {
            return XCTFail("Expected lighting editor modal to open")
        }

        if !lighting.hasLighting {
            store.dispatch("editor", "LightingAddDefault")
            guard case .lightingEditor(let initialized) = store.activeModal else {
                return XCTFail("Expected lighting editor modal after LightingAddDefault")
            }
            lighting = initialized
        }
        XCTAssertTrue(lighting.hasLighting)
        XCTAssertFalse(lighting.previewActive)
        XCTAssertEqual(lighting.previewRemainingSecs, 0)

        store.dispatch("editor", "LightingBeginPreview")
        guard case .lightingEditor(let afterBegin) = store.activeModal else {
            return XCTFail("Expected lighting editor modal to stay open after LightingBeginPreview")
        }
        XCTAssertFalse(afterBegin.previewActive)
        XCTAssertEqual(afterBegin.previewRemainingSecs, 0)
        let zhError = try XCTUnwrap(afterBegin.validationError)
        XCTAssertFalse(zhError.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

        store.setUiLanguage(.en)
        XCTAssertEqual(store.locale, .en)
        XCTAssertEqual(store.tr("在键盘上预览"), "Preview on Pad")
        guard case .lightingEditor(let enModal) = store.activeModal else {
            return XCTFail("Expected lighting editor modal to remain open after switching to English")
        }
        XCTAssertFalse(enModal.previewActive)
        XCTAssertEqual(enModal.previewRemainingSecs, 0)
        let enError = try XCTUnwrap(enModal.validationError)
        XCTAssertFalse(enError.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        XCTAssertNotEqual(enError, zhError)
        XCTAssertTrue(enError.allSatisfy(\.isASCII), "Expected English localized preview error, got \(enError)")
        XCTAssertTrue(enError.localizedCaseInsensitiveContains("preview"))

        store.dispatch("editor", "LightingEndPreview")
        guard case .lightingEditor(let afterEnd) = store.activeModal else {
            return XCTFail("Expected lighting editor modal to remain open after LightingEndPreview")
        }
        XCTAssertNil(afterEnd.validationError)
        XCTAssertFalse(afterEnd.previewActive)
        XCTAssertEqual(afterEnd.previewRemainingSecs, 0)
    }

    func testSessionSlotStatusDetailDecodingAndEquatablePropagation() throws {
        let baseSlotJSON: [String: Any] = [
            "slot_index": 0,
            "key_binding_id": "AG00",
            "mode": "Recent",
            "status": "NeedsInteraction",
            "kind": "NeedsAttention",
            "agent_monogram": "AG",
            "title": "Build Retry Conv",
            "subtitle": "待处理审批/输入",
            "place": ["Antigravity": ["conversation_id": "c-1", "workspace_path": "/ws"]],
            "watermark": "wm-1",
            "is_unread_blue_dot": false,
            "is_working_animated": false,
            "is_bound_to_top_key": true,
            "led_color_hex": "#FFAA00",
            "status_detail": "模型自动重试（第 2 次）：unexpected EOF · 4s"
        ]
        let parsed1 = CoreStore.parseProjection([
            "session_strip": [
                "slots": [baseSlotJSON],
                "attention_count": 1,
                "unread_completed_count": 0,
                "working_count": 0
            ]
        ])
        XCTAssertEqual(parsed1.sessionSlots.count, 1)
        XCTAssertEqual(
            parsed1.sessionSlots[0].statusDetail,
            "模型自动重试（第 2 次）：unexpected EOF · 4s"
        )

        var updatedSlotJSON = baseSlotJSON
        updatedSlotJSON["status_detail"] = "等待模型响应已 1m05s（超 60s 无进展）"
        let parsed2 = CoreStore.parseProjection([
            "session_strip": [
                "slots": [updatedSlotJSON],
                "attention_count": 1,
                "unread_completed_count": 0,
                "working_count": 0
            ]
        ])
        XCTAssertEqual(
            parsed2.sessionSlots[0].statusDetail,
            "等待模型响应已 1m05s（超 60s 无进展）"
        )
        XCTAssertNotEqual(parsed1.sessionSlots[0], parsed2.sessionSlots[0])

        var clearedSlotJSON = baseSlotJSON
        clearedSlotJSON.removeValue(forKey: "status_detail")
        let parsed3 = CoreStore.parseProjection([
            "session_strip": [
                "slots": [clearedSlotJSON],
                "attention_count": 0,
                "unread_completed_count": 0,
                "working_count": 0
            ]
        ])
        XCTAssertNil(parsed3.sessionSlots[0].statusDetail)
        XCTAssertNotEqual(parsed2.sessionSlots[0], parsed3.sessionSlots[0])
    }
}

@MainActor
private final class FakeSoftwareUpdateDriver: SoftwareUpdateDriver {
    var canCheckForUpdates: Bool = true
    var automaticallyChecksForUpdates: Bool = true
    var lastUpdateCheckDate: Date? = nil
    private(set) var startCalls: Int = 0
    private(set) var checkCalls: Int = 0

    func startUpdater() {
        startCalls += 1
    }

    func checkForUpdates() {
        checkCalls += 1
    }
}
