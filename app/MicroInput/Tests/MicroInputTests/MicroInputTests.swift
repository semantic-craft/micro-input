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
        XCTAssertTrue(store.sessionStrip.simpleLighting.glowOnWorking)
        XCTAssertEqual(store.sessionStrip.simpleLighting.ambientColor, "OceanBlue")
        XCTAssertEqual(store.sessionStrip.simpleLighting.workingColor, "EmeraldGreen")

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

        for (dirName, expectedSlot) in [("North", 16), ("South", 17), ("West", 18), ("East", 19)] {
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

        store.dispatch("status_menu", "ResumeAutoFollow")
        XCTAssertFalse(store.sidebar.isProfileLocked)
        XCTAssertFalse(store.statusMenu.isProfileLocked)
        XCTAssertNil(store.statusMenu.lockedProfileId)
        XCTAssertTrue(store.statusMenu.autoFollowEnabled)

        store.dispatch("status_menu", ["SetProfileLocked": true])
        XCTAssertTrue(store.statusMenu.isProfileLocked)
        store.dispatch("status_menu", ["SetProfileLocked": false])
        XCTAssertFalse(store.statusMenu.isProfileLocked)

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
