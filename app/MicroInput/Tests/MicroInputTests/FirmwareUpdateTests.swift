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
        let fullFirmware = FirmwareSliceModel(json: [
            "current_version": "0.7.0-rc.16",
            "v070": [
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
            ],
        ])

        let v070 = fullFirmware.v070
        XCTAssertTrue(v070.supported)
        XCTAssertTrue(v070.synced)
        XCTAssertEqual(v070.activeProfile, 2)
        XCTAssertEqual(v070.activeLayer, 4)
        XCTAssertEqual(v070.batteryPercent, 87)
        XCTAssertEqual(v070.isCharging, false)
        XCTAssertNil(v070.lastError)
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
        XCTAssertNil(partial.activeLayer)
        XCTAssertNil(partial.batteryPercent)
        XCTAssertEqual(partial.mouse.deadzone, 0.15, accuracy: 0.0001)
        XCTAssertNil(partial.saRgb.sa)

        let legacy063 = FirmwareSliceModel(json: ["current_version": "0.6.3"])
        XCTAssertFalse(legacy063.v070.supported)
        XCTAssertNotEqual(legacy063, fullFirmware)
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
        for (width, height) in [(820.0, 640.0), (1180.0, 740.0)] {
            let root = ClassicWorkbenchView(
                store: store,
                navigation: WorkbenchNavigationModel(destination: .general),
                updater: updater
            )
            let host = NSHostingView(rootView: root.frame(width: width, height: height))
            host.layoutSubtreeIfNeeded()
            XCTAssertGreaterThan(host.fittingSize.height, 200)
            XCTAssertLessThanOrEqual(host.fittingSize.width, width + 0.5)
        }
    }

    @MainActor
    func testV070HardwareCommandPayloadsMatchRustVariantsAndDispatch() throws {
        let refresh = V070HardwareCommand.refresh()
        XCTAssertEqual(refresh["V070Hardware"] as? String, "Refresh")

        let selectLayer = V070HardwareCommand.selectLayer(5)["V070Hardware"] as? [String: Any]
        XCTAssertEqual((selectLayer?["SelectLayer"] as? [String: Any])?["layer"] as? Int, 5)

        let preset = V070HardwareCommand.applyMousePreset("fast")["V070Hardware"] as? [String: Any]
        XCTAssertEqual((preset?["ApplyMousePreset"] as? [String: Any])?["preset"] as? String, "fast")

        let mouseCfg = (V070HardwareCommand.applyMouseConfig(
            moveDelta: 10, maxSpeed: 20, wheelDelta: 2, wheelMaxSpeed: 10,
            stickSpeed: 4, stickScroll: 80, deadzone: 0.2
        )["V070Hardware"] as? [String: Any])?["ApplyMouseConfig"] as? [String: Any]
        XCTAssertEqual(mouseCfg?["move_delta"] as? Int, 10)
        XCTAssertEqual(mouseCfg?["max_speed"] as? Int, 20)
        XCTAssertEqual(mouseCfg?["wheel_delta"] as? Int, 2)
        XCTAssertEqual(mouseCfg?["wheel_max_speed"] as? Int, 10)
        XCTAssertEqual(mouseCfg?["stick_speed"] as? Int, 4)
        XCTAssertEqual(mouseCfg?["stick_scroll"] as? Int, 80)
        XCTAssertEqual(mouseCfg?["deadzone"] as? Double ?? -1, 0.2, accuracy: 0.0001)

        let pmCfg = (V070HardwareCommand.applyPmConfig(
            autoSleep: false, hostSleep: true, sleepAfterMin: 30
        )["V070Hardware"] as? [String: Any])?["ApplyPmConfig"] as? [String: Any]
        XCTAssertEqual(pmCfg?["auto_sleep"] as? Bool, false)
        XCTAssertEqual(pmCfg?["host_sleep"] as? Bool, true)
        XCTAssertEqual(pmCfg?["sleep_after_min"] as? Int, 30)

        let rename = (V070HardwareCommand.renameDevice(name: "Pad07")["V070Hardware"] as? [String: Any])?["RenameDevice"] as? [String: Any]
        XCTAssertEqual(rename?["name"] as? String, "Pad07")

        let saApply = (V070HardwareCommand.applySaRgb(
            sa: 7057, colorHex: "#33AAFF", brightness: 0.75, speed: 0.25, effect: 4
        )["V070Hardware"] as? [String: Any])?["ApplySaRgb"] as? [String: Any]
        XCTAssertEqual(saApply?["sa"] as? Int, 7057)
        XCTAssertEqual(saApply?["color_hex"] as? String, "#33AAFF")
        XCTAssertEqual(saApply?["brightness"] as? Double ?? -1, 0.75, accuracy: 0.0001)
        XCTAssertEqual(saApply?["speed"] as? Double ?? -1, 0.25, accuracy: 0.0001)
        XCTAssertEqual(saApply?["effect"] as? Int, 4)

        let saReset = (V070HardwareCommand.resetSaRgb(sa: 7057)["V070Hardware"] as? [String: Any])?["ResetSaRgb"] as? [String: Any]
        XCTAssertEqual(saReset?["sa"] as? Int, 7057)

        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-v070-dispatch-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let store = CoreStore(isolatedDirectory: dir.path)
        let allCommands: [[String: Any]] = [
            V070HardwareCommand.refresh(),
            V070HardwareCommand.selectLayer(5),
            V070HardwareCommand.applyMousePreset("fast"),
            V070HardwareCommand.applyMouseConfig(
                moveDelta: 10, maxSpeed: 20, wheelDelta: 2, wheelMaxSpeed: 10,
                stickSpeed: 4, stickScroll: 80, deadzone: 0.2
            ),
            V070HardwareCommand.applyPmConfig(autoSleep: false, hostSleep: true, sleepAfterMin: 30),
            V070HardwareCommand.renameDevice(name: "Pad07"),
            V070HardwareCommand.applySaRgb(sa: 7057, colorHex: "#33AAFF", brightness: 0.75, speed: 0.25, effect: 4),
            V070HardwareCommand.resetSaRgb(sa: 7057),
        ]
        for payload in allCommands {
            store.dispatch("firmware", payload)
            XCTAssertNotNil(store.firmware.v070.lastError)
            XCTAssertFalse(store.firmware.v070.busy)
        }
    }
}
