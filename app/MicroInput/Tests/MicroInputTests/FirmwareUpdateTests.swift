import AppKit
import SwiftUI
import XCTest
@testable import MicroInputUI

final class FirmwareUpdateTests: XCTestCase {
    func testUnknownVersionAndPortSelectionNeverInventDeviceState() {
        let model = FirmwareSliceModel(json: [
            "current_version": NSNull(), "selected_port": NSNull(),
            "downloaded": true, "device_reserved": true,
            "ports": [["path": "/dev/cu.usbmodem-test", "manufacturer": "Espressif"]]
        ])
        XCTAssertEqual(model.currentVersionLabel, "未知（尚未从设备读取）")
        XCTAssertNil(model.selectedPort)
        XCTAssertFalse(model.canInstall)
        XCTAssertEqual(model.channel, "stable")
        XCTAssertFalse(model.canCheck)
        XCTAssertFalse(model.canDownload)
    }

    func testBusyReservationAndStalePortPreventInstallationAndTermination() {
        var model = FirmwareSliceModel(json: [
            "channel": "prerelease", "current_version": "0.6.3",
            "version_status": "候选版本较旧", "release": ["tag": "v0.7.0-rc.16", "prerelease": true, "notes": "Firmware notes"],
            "downloaded": true, "digest_verified": true, "device_reserved": true,
            "selected_port": "/dev/cu.usbmodem-test",
            "ports": [["path": "/dev/cu.usbmodem-test"]], "progress": 0.25,
            "backup_path": "/tmp/firmware-backup"
        ])
        XCTAssertTrue(model.canInstall)
        XCTAssertEqual(model.versionStatus, "候选版本较旧")
        XCTAssertTrue(model.prerelease)
        XCTAssertTrue(model.digestVerified)
        XCTAssertEqual(model.progress, 0.25)
        model.busy = true
        XCTAssertFalse(model.canInstall)
        XCTAssertTrue(model.preventsTermination)
        model.deviceReserved = false
        XCTAssertFalse(model.preventsTermination)
        model.busy = false
        model.deviceReserved = true
        model.ports = []
        XCTAssertFalse(model.canInstall)
    }

    @MainActor
    func testFirmwareSettingsRenderWithoutDispatchingAnyAction() {
        var actions = 0
        let model = FirmwareSliceModel(json: [
            "phase": "flash_verified", "message": "写入校验通过；运行版本尚待核对",
            "device_reserved": true, "backup_path": "/tmp/firmware-backup"
        ])
        let view = FirmwareUpdateSettingsSection(model: model) { _ in actions += 1 }
        let host = NSHostingView(rootView: view.frame(width: 680))
        host.layoutSubtreeIfNeeded()
        XCTAssertGreaterThan(host.fittingSize.height, 200)
        XCTAssertEqual(actions, 0)
        XCTAssertFalse(model.canInstall)
    }

    @MainActor
    func testConnectKeyboardControlRendersWhenDisconnectedWithoutTriggeringSideEffects() {
        var connectClicks = 0
        let disconnected = FirmwareSliceModel(json: [
            "current_version": NSNull(),
            "device_reserved": false,
            "phase": "idle"
        ])
        let view = FirmwareUpdateSettingsSection(
            model: disconnected,
            onConnectKeyboard: { connectClicks += 1 }
        ) { _ in }
        let host = NSHostingView(rootView: view.frame(width: 680))
        host.layoutSubtreeIfNeeded()
        XCTAssertGreaterThan(host.fittingSize.height, 120)
        XCTAssertEqual(connectClicks, 0)
    }

    func testV070HardwareSliceModelDecodesFullPartialAndLegacyJson() {
        let v070 = V070HardwareSliceModel(json: [
            "supported": true,
            "busy": false,
            "synced": true,
            "active_profile": NSNumber(value: 2),
            "active_layer": NSNumber(value: 4),
            "battery_percent": NSNumber(value: 87),
            "is_charging": false,
            "host_sleep_status": "awake",
            "status_message": "已从设备同步 0.7.0 状态",
            "last_error": NSNull(),
            "auto_layer_sync_enabled": true,
            "preferred_device_serial": "CM2-SN-001",
            "mouse": [
                "preset": "fast",
                "delay_ms": 80,
                "interval_ms": 15,
                "move_delta": 12,
                "max_speed": 20,
                "time_to_max": 25,
                "wheel_delay_ms": 90,
                "wheel_interval_ms": 60,
                "wheel_delta": 2,
                "wheel_max_speed": 12,
                "wheel_time_to_max": 35,
                "stick_speed": 4,
                "stick_regulator": 18,
                "stick_scroll": 120,
                "deadzone": NSNumber(value: 0.25),
            ],
            "pm_config": [
                "auto_sleep": true,
                "host_sleep": false,
                "sleep_after_min": 45,
            ],
            "pm_state": [
                "state": "active",
                "floor": "usb",
                "base_floor": "idle",
                "line_powered": true,
                "auto_sleep": true,
                "sleep_after_s": 2700,
                "idle_s": 12,
                "standby_s": 0,
                "credit_s": 180,
                "usb_host_hold": true,
                "holds": [
                    ["owner": "usb.host", "floor": "usb", "age_ms": 3400]
                ],
            ],
            "device_name": [
                "name": "MyMicro",
                "factory_name": "MicroInput",
                "max_length": 15,
            ],
            "sa_rgb": [
                "sa": 7057,
                "color_hex": "#FFB040",
                "brightness": NSNumber(value: 0.8),
                "speed": 0.4,
                "effect": 4,
            ],
        ])

        XCTAssertTrue(v070.supported)
        XCTAssertTrue(v070.synced)
        XCTAssertEqual(v070.activeProfile, 2)
        XCTAssertEqual(v070.activeLayer, 4)
        XCTAssertEqual(v070.batteryPercent, 87)
        XCTAssertEqual(v070.isCharging, false)
        XCTAssertNil(v070.lastError)
        XCTAssertTrue(v070.autoLayerSyncEnabled)
        XCTAssertEqual(v070.preferredDeviceSerial, "CM2-SN-001")
        XCTAssertEqual(v070.mouse.preset, "fast")
        XCTAssertEqual(v070.mouse.moveDelta, 12)
        XCTAssertEqual(v070.mouse.stickScroll, 120)
        XCTAssertEqual(v070.mouse.deadzone, 0.25, accuracy: 0.0001)
        XCTAssertEqual(v070.pmConfig.hostSleep, false)
        XCTAssertEqual(v070.pmConfig.sleepAfterMin, 45)
        XCTAssertTrue(v070.pmState.linePowered)
        XCTAssertTrue(v070.pmState.usbHostHold)
        XCTAssertEqual(v070.pmState.holds.first?.owner, "usb.host")
        XCTAssertEqual(v070.deviceName.name, "MyMicro")
        XCTAssertEqual(v070.deviceName.factoryName, "MicroInput")
        XCTAssertEqual(v070.saRgb.sa, 7057)
        XCTAssertEqual(v070.saRgb.colorHex, "#FFB040")
        XCTAssertEqual(v070.saRgb.brightness, 0.8, accuracy: 0.0001)
        XCTAssertEqual(v070.saRgb.effect, 4)

        let partial = V070HardwareSliceModel(json: [
            "supported": false,
            "active_layer": true,
            "battery_percent": NSNull(),
            "mouse": ["deadzone": false],
            "sa_rgb": ["sa": NSNull()],
        ])
        XCTAssertFalse(partial.supported)
        XCTAssertFalse(partial.autoLayerSyncEnabled)
        XCTAssertFalse(partial.sessionSaRgbSyncEnabled)
        XCTAssertNil(partial.preferredDeviceSerial)
        XCTAssertNil(partial.activeLayer)
        XCTAssertNil(partial.batteryPercent)
        XCTAssertEqual(partial.mouse.deadzone, 0.15, accuracy: 0.0001)
        XCTAssertNil(partial.saRgb.sa)

        let legacyDefault = V070HardwareSliceModel(json: [:])
        XCTAssertFalse(legacyDefault.supported)
        XCTAssertNotEqual(legacyDefault, v070)
    }

    @MainActor
    func testV070HardwareSettingsSectionRendersBothModesWithoutDispatching() {
        var dispatched = 0
        for status in ["awake", "sleeping", "waking"] {
            let unsupported = V070HardwareSliceModel(json: [
                "supported": false,
                "host_sleep_status": status,
                "status_message": "当前版本不支持 0.7.0",
                "last_error": "设备未连接",
            ])
            let host = NSHostingView(
                rootView: V070HardwareSettingsSection(model: unsupported) { _ in dispatched += 1 }
                    .frame(width: 520)
            )
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 60)
        }

        let supported = V070HardwareSliceModel(json: [
            "supported": true,
            "synced": true,
            "active_profile": 1,
            "active_layer": 3,
            "battery_percent": 92,
            "is_charging": true,
            "pm_state": ["state": "active", "floor": "usb", "base_floor": "idle", "usb_host_hold": true],
            "device_name": ["name": "Pad07", "factory_name": "MicroInput", "max_length": 15],
            "sa_rgb": ["sa": 7057, "color_hex": "#33AAFF", "brightness": 0.9, "speed": 0.5, "effect": 0],
        ])
        let supportedHost = NSHostingView(
            rootView: V070HardwareSettingsSection(model: supported) { _ in dispatched += 1 }
                .frame(width: 520)
        )
        supportedHost.layoutSubtreeIfNeeded()
        XCTAssertGreaterThan(supportedHost.fittingSize.height, 400)
        XCTAssertLessThanOrEqual(supportedHost.fittingSize.width, 520.5)
        XCTAssertEqual(dispatched, 0)
    }

    @MainActor
    func testGeneralSettingsWithV070HardwareFitsSupportedWindowWidths() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-v070-ui-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let updater = SoftwareUpdateController(
            bundlePath: "/Applications/Micro Input.app",
            environment: ["MI_DISABLE_UPDATER": "1"]
        )
        for destination in [WorkbenchDestination.general, .device] {
            for (width, height) in [(820.0, 640.0), (1180.0, 740.0)] {
                let root = ClassicWorkbenchView(
                    store: store,
                    navigation: WorkbenchNavigationModel(destination: destination),
                    updater: updater
                )
                let host = NSHostingView(rootView: root.frame(width: width, height: height))
                host.layoutSubtreeIfNeeded()
                XCTAssertGreaterThan(host.fittingSize.height, 200)
                XCTAssertLessThanOrEqual(host.fittingSize.width, width + 0.5)
            }
        }
    }

    @MainActor
    func testV070HardwareCommandPayloadsMatchRustVariantsAndDispatch() throws {
        XCTAssertEqual(V070HardwareCommand.refresh(), "Refresh")

        let selectLayer = V070HardwareCommand.selectLayer(5)["SelectLayer"] as? [String: Any]
        XCTAssertEqual(selectLayer?["layer"] as? Int, 5)

        let joyHw = V070HardwareCommand.applyJoystickHardwareMode("mouse")["ApplyJoystickHardwareMode"] as? [String: Any]
        XCTAssertEqual(joyHw?["mode"] as? String, "mouse")

        let preset = V070HardwareCommand.applyMousePreset("precise")["ApplyMousePreset"] as? [String: Any]
        XCTAssertEqual(preset?["preset"] as? String, "precise")

        let mouseCfg = V070HardwareCommand.applyMouseConfig(
            moveDelta: 10, maxSpeed: 20, wheelDelta: 2, wheelMaxSpeed: 10,
            stickSpeed: 4, stickScroll: 80
        )["ApplyMouseConfig"] as? [String: Any]
        XCTAssertEqual(mouseCfg?["move_delta"] as? Int, 10)
        XCTAssertEqual(mouseCfg?["max_speed"] as? Int, 20)
        XCTAssertEqual(mouseCfg?["wheel_delta"] as? Int, 2)
        XCTAssertEqual(mouseCfg?["wheel_max_speed"] as? Int, 10)
        XCTAssertEqual(mouseCfg?["stick_speed"] as? Int, 4)
        XCTAssertEqual(mouseCfg?["stick_scroll"] as? Int, 80)
        XCTAssertNil(mouseCfg?["deadzone"])

        let deadzoneCmd = V070HardwareCommand.applyJoystickDeadzone(0.2)["ApplyJoystickDeadzone"] as? [String: Any]
        XCTAssertEqual(deadzoneCmd?["deadzone"] as? Double ?? -1, 0.2, accuracy: 0.0001)

        let pmCfg = V070HardwareCommand.applyPmConfig(
            autoSleep: false, hostSleep: true, sleepAfterMin: 30
        )["ApplyPmConfig"] as? [String: Any]
        XCTAssertEqual(pmCfg?["auto_sleep"] as? Bool, false)
        XCTAssertEqual(pmCfg?["host_sleep"] as? Bool, true)
        XCTAssertEqual(pmCfg?["sleep_after_min"] as? Int, 30)

        let rename = V070HardwareCommand.renameDevice(name: "Pad07")["RenameDevice"] as? [String: Any]
        XCTAssertEqual(rename?["name"] as? String, "Pad07")

        let saApply = V070HardwareCommand.applySaRgb(
            sa: 7057, colorHex: "#33AAFF", brightness: 0.75, speed: 0.25, effect: 4
        )["ApplySaRgb"] as? [String: Any]
        XCTAssertEqual(saApply?["sa"] as? Int, 7057)
        XCTAssertEqual(saApply?["color_hex"] as? String, "#33AAFF")
        XCTAssertEqual(saApply?["brightness"] as? Double ?? -1, 0.75, accuracy: 0.0001)
        XCTAssertEqual(saApply?["speed"] as? Double ?? -1, 0.25, accuracy: 0.0001)
        XCTAssertEqual(saApply?["effect"] as? Int, 4)

        let saReset = V070HardwareCommand.resetSaRgb(sa: 7057)["ResetSaRgb"] as? [String: Any]
        XCTAssertEqual(saReset?["sa"] as? Int, 7057)

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-v070-dispatch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let allCommands: [Any] = [
            V070HardwareCommand.refresh(),
            V070HardwareCommand.selectLayer(5),
            V070HardwareCommand.applyJoystickHardwareMode("mouse"),
            V070HardwareCommand.applyMousePreset("precise"),
            V070HardwareCommand.applyMouseConfig(
                moveDelta: 10, maxSpeed: 20, wheelDelta: 2, wheelMaxSpeed: 10,
                stickSpeed: 4, stickScroll: 80
            ),
            V070HardwareCommand.applyJoystickDeadzone(0.2),
            V070HardwareCommand.applyPmConfig(autoSleep: false, hostSleep: true, sleepAfterMin: 30),
            V070HardwareCommand.renameDevice(name: "Pad07"),
            V070HardwareCommand.applySaRgb(sa: 7057, colorHex: "#33AAFF", brightness: 0.75, speed: 0.25, effect: 4),
            V070HardwareCommand.resetSaRgb(sa: 7057),
        ]
        for payload in allCommands {
            store.dispatchV070Hardware(payload)
            XCTAssertNotNil(store.v070Hardware.lastError)
            XCTAssertFalse(store.v070Hardware.busy)
        }
    }

    @MainActor
    func testMouseSettingsSlidersRenderUnusualBoundsWithoutClipping() {
        var dispatched = 0
        let unusualMouse = V070HardwareSliceModel(json: [
            "supported": true,
            "synced": true,
            "active_profile": 1,
            "active_layer": 2,
            "mouse": [
                "preset": "custom",
                "move_delta": 0,
                "max_speed": 300,
                "wheel_delta": 12,
                "wheel_max_speed": 24,
                "stick_speed": 10,
                "stick_scroll": 2600,
                "deadzone": 0.18,
            ],
        ])
        let host = NSHostingView(
            rootView: V070HardwareSettingsSection(model: unusualMouse) { _ in dispatched += 1 }
                .frame(width: 560)
        )
        host.layoutSubtreeIfNeeded()
        XCTAssertGreaterThan(host.fittingSize.height, 400)
        XCTAssertLessThanOrEqual(host.fittingSize.width, 560.5)
        XCTAssertEqual(dispatched, 0)
    }

    func testMouseDraftFirmwareBoundsDeadzoneValidationAndIndependence() {
        var modelA = V070MouseConfigModel()
        modelA.deadzone = 0.15
        var modelB = modelA
        modelB.deadzone = 0.35
        XCTAssertEqual(V070MouseDraft(from: modelA), V070MouseDraft(from: modelB))

        var draft = V070MouseDraft(from: modelA)
        XCTAssertNotNil(draft.validated)
        XCTAssertNil(draft.validationHint)

        draft.moveDelta = "127"
        draft.maxSpeed = "100"
        draft.wheelDelta = "127"
        draft.wheelMaxSpeed = "100"
        draft.stickSpeed = "255"
        draft.stickScroll = "255"
        let maxTuple = draft.validated
        XCTAssertNotNil(maxTuple)
        XCTAssertEqual(maxTuple?.0, 127)
        XCTAssertEqual(maxTuple?.1, 100)
        XCTAssertEqual(maxTuple?.2, 127)
        XCTAssertEqual(maxTuple?.3, 100)
        XCTAssertEqual(maxTuple?.4, 255)
        XCTAssertEqual(maxTuple?.5, 255)

        var badMoveDelta = draft
        badMoveDelta.moveDelta = "128"
        XCTAssertNil(badMoveDelta.validated)
        XCTAssertEqual(badMoveDelta.validationHint, "请确认步进在 1–127、加速上限在 1–100、摇杆参数在 1–255")

        var badMaxSpeed = draft
        badMaxSpeed.maxSpeed = "101"
        XCTAssertNil(badMaxSpeed.validated)

        var badWheelDelta = draft
        badWheelDelta.wheelDelta = "128"
        XCTAssertNil(badWheelDelta.validated)

        var badWheelMax = draft
        badWheelMax.wheelMaxSpeed = "101"
        XCTAssertNil(badWheelMax.validated)

        var badStickSpeed = draft
        badStickSpeed.stickSpeed = "256"
        XCTAssertNil(badStickSpeed.validated)

        var badStickScroll = draft
        badStickScroll.stickScroll = "256"
        XCTAssertNil(badStickScroll.validated)

        var zeroMoveDelta = draft
        zeroMoveDelta.moveDelta = "0"
        XCTAssertNil(zeroMoveDelta.validated)

        XCTAssertTrue(V070MouseDraft.isValidDeadzone(0.0025))
        XCTAssertTrue(V070MouseDraft.isValidDeadzone(0.15))
        XCTAssertTrue(V070MouseDraft.isValidDeadzone(0.95))
        XCTAssertFalse(V070MouseDraft.isValidDeadzone(0.0024))
        XCTAssertFalse(V070MouseDraft.isValidDeadzone(0.9501))
        XCTAssertFalse(V070MouseDraft.isValidDeadzone(.nan))
        XCTAssertFalse(V070MouseDraft.isValidDeadzone(.infinity))
    }

    @MainActor
    func testAutoLayerSyncCommandPayloadAndOfflineToggleWithSerialSummary() throws {
        let onCmd = V070HardwareCommand.setAutoLayerSync(true)["SetAutoLayerSync"] as? [String: Any]
        XCTAssertEqual(onCmd?["enabled"] as? Bool, true)
        let offCmd = V070HardwareCommand.setAutoLayerSync(false)["SetAutoLayerSync"] as? [String: Any]
        XCTAssertEqual(offCmd?["enabled"] as? Bool, false)

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-v070-sync-toggle-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertFalse(store.v070Hardware.autoLayerSyncEnabled)
        store.setV070AutoLayerSync(true)
        XCTAssertTrue(store.v070Hardware.autoLayerSyncEnabled)
        XCTAssertNil(store.v070Hardware.lastError)
        store.setV070AutoLayerSync(false)
        XCTAssertFalse(store.v070Hardware.autoLayerSyncEnabled)
        XCTAssertNil(store.v070Hardware.lastError)

        var dispatched = 0
        let withSerial = V070HardwareSliceModel(json: [
            "supported": false,
            "auto_layer_sync_enabled": true,
            "preferred_device_serial": "CM2-ALPHA",
        ])
        let host = NSHostingView(
            rootView: V070HardwareSettingsSection(model: withSerial) { _ in dispatched += 1 }
                .frame(width: 520)
        )
        host.layoutSubtreeIfNeeded()
        XCTAssertGreaterThan(host.fittingSize.height, 140)
        XCTAssertLessThanOrEqual(host.fittingSize.width, 520.5)
        XCTAssertEqual(dispatched, 0)
    }

    @MainActor
    func testSessionSaRgbSyncCommandToggleAndPrepareForTermination() throws {
        let onCmd = V070HardwareCommand.setSessionSaRgbSync(true)["SetSessionSaRgbSync"] as? [String: Any]
        XCTAssertEqual(onCmd?["enabled"] as? Bool, true)
        let offCmd = V070HardwareCommand.setSessionSaRgbSync(false)["SetSessionSaRgbSync"] as? [String: Any]
        XCTAssertEqual(offCmd?["enabled"] as? Bool, false)

        let decodedOn = V070HardwareSliceModel(json: ["session_sa_rgb_sync_enabled": true])
        XCTAssertTrue(decodedOn.sessionSaRgbSyncEnabled)
        let decodedDefault = V070HardwareSliceModel(json: [:])
        XCTAssertFalse(decodedDefault.sessionSaRgbSyncEnabled)

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-v070-sa-rgb-sync-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertFalse(store.v070Hardware.sessionSaRgbSyncEnabled)
        store.setV070SessionSaRgbSync(true)
        XCTAssertTrue(store.v070Hardware.sessionSaRgbSyncEnabled)
        XCTAssertNil(store.v070Hardware.lastError)
        store.setV070SessionSaRgbSync(false)
        XCTAssertFalse(store.v070Hardware.sessionSaRgbSyncEnabled)
        XCTAssertNil(store.v070Hardware.lastError)
        store.prepareForTermination()
        XCTAssertNil(store.v070Hardware.lastError)
    }

    @MainActor
    func testV070HardwareAndFirmwareSlicesRemainIndependent() throws {
        let parsed = CoreStore.parseProjection([
            "firmware": ["channel": "prerelease", "current_version": "0.7.0-rc.16"],
            "v070_hardware": ["supported": true, "active_layer": 3, "active_joystick_mode": "mouse"],
        ])
        XCTAssertEqual(parsed.firmware.channel, "prerelease")
        XCTAssertEqual(parsed.firmware.currentVersion, "0.7.0-rc.16")
        XCTAssertTrue(parsed.v070Hardware.supported)
        XCTAssertEqual(parsed.v070Hardware.activeLayer, 3)
        XCTAssertEqual(parsed.v070Hardware.activeJoystickMode, "mouse")

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-v070-slice-isolation-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        final class HitCounter: @unchecked Sendable {
            private let lock = NSLock()
            private var value = 0
            func increment() {
                lock.lock()
                value += 1
                lock.unlock()
            }
            var count: Int {
                lock.lock()
                defer { lock.unlock() }
                return value
            }
        }

        let initialFirmware = store.firmware
        let firmwareHits = HitCounter()
        withObservationTracking {
            _ = store.firmware
        } onChange: {
            firmwareHits.increment()
        }
        store.setV070AutoLayerSync(true)
        store.dispatchV070Hardware(V070HardwareCommand.refresh())
        XCTAssertTrue(store.v070Hardware.autoLayerSyncEnabled)
        XCTAssertNotNil(store.v070Hardware.lastError)
        XCTAssertEqual(store.firmware, initialFirmware)
        XCTAssertEqual(firmwareHits.count, 0)

        let v070BeforeFirmwareDispatch = store.v070Hardware
        let v070Hits = HitCounter()
        withObservationTracking {
            _ = store.v070Hardware
        } onChange: {
            v070Hits.increment()
        }
        store.dispatch("firmware", "PrepareDevice")
        XCTAssertEqual(store.firmware.message, "隔离验收不会切换实体设备的引导模式")
        XCTAssertNotEqual(store.firmware, initialFirmware)
        XCTAssertEqual(store.v070Hardware, v070BeforeFirmwareDispatch)
        XCTAssertEqual(v070Hits.count, 0)
    }
}
