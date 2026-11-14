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
}
