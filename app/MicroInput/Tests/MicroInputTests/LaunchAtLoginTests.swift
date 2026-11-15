import AppKit
import Carbon
import SwiftUI
import XCTest
@testable import MicroInputUI

@MainActor
private final class RecordingLaunchAtLoginDriver: LaunchAtLoginDriver {
    var simulatedStatus: LaunchAtLoginDriverStatus = .notRegistered
    var statusAfterRegister: LaunchAtLoginDriverStatus = .enabled
    var statusAfterUnregister: LaunchAtLoginDriverStatus = .notRegistered
    var nextError: Error?
    private(set) var registerCount = 0
    private(set) var unregisterCount = 0
    private(set) var openSettingsCount = 0

    func currentStatus() -> LaunchAtLoginDriverStatus {
        simulatedStatus
    }

    func register() throws {
        registerCount += 1
        if let nextError {
            self.nextError = nil
            throw nextError
        }
        simulatedStatus = statusAfterRegister
    }

    func unregister() throws {
        unregisterCount += 1
        if let nextError {
            self.nextError = nil
            throw nextError
        }
        simulatedStatus = statusAfterUnregister
    }

    func openSystemSettingsLoginItems() {
        openSettingsCount += 1
    }
}

final class LaunchAtLoginTests: XCTestCase {
    private static let prodBundleId = LaunchAtLoginPolicy.productionBundleIdentifier
    private static let prodBundlePath = "/Applications/Micro Input.app"

    func testLaunchAtLoginPolicyBlocksIsolatedEphemeralUnbundledAndWrongBundleId() {
        XCTAssertNil(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: Self.prodBundlePath,
                environment: [:],
                arguments: ["/Applications/Micro Input.app/Contents/MacOS/MicroInput"]
            )
        )

        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: Self.prodBundlePath,
                environment: [:],
                arguments: ["MicroInput", "--verify"]
            ),
            .qaVerifyMode
        )

        for envKey in ["MI_QA_SCENARIO", "MI_SYNTHETIC_SCENARIO", "MI_DOCUMENT_DIR", "MI_HOST_AUDIT_LOG"] {
            XCTAssertEqual(
                LaunchAtLoginPolicy.evaluate(
                    bundleIdentifier: Self.prodBundleId,
                    bundlePath: Self.prodBundlePath,
                    environment: [envKey: "1"]
                ),
                .qaVerifyMode,
                "Expected \(envKey) to gate LaunchAtLoginPolicy with .qaVerifyMode"
            )
        }

        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: "com.semanticcraft.microinput.qa.offline",
                bundlePath: Self.prodBundlePath,
                environment: [:]
            ),
            .qaVerifyMode
        )

        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: Self.prodBundlePath,
                environment: ["MI_DISABLE_LOGIN_ITEM": "1"]
            ),
            .disabledByEnvironment(variable: "MI_DISABLE_LOGIN_ITEM")
        )

        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: "/tmp/MicroInput",
                environment: [:]
            ),
            .unbundledExecutable
        )

        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: "/Volumes/Micro Input/Micro Input.app",
                environment: [:]
            ),
            .translocatedOrMountedVolume(bundlePath: "/Volumes/Micro Input/Micro Input.app")
        )

        let ephemeralPath = "/var/folders/ab/cd/T/target/MicroInput.app"
        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: ephemeralPath,
                environment: [:]
            ),
            .ephemeralBuildBundle(bundlePath: ephemeralPath)
        )

        XCTAssertNil(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: "/Applications/Micro Input.app/",
                environment: [:]
            )
        )

        let symlinkContainer = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent(".mi-symlink-check-\(UUID().uuidString)")
        let ephemeralTargetDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("mi-ephemeral-\(UUID().uuidString)/target/MicroInput.app")
        defer {
            try? FileManager.default.removeItem(at: symlinkContainer)
            try? FileManager.default.removeItem(at: ephemeralTargetDir.deletingLastPathComponent().deletingLastPathComponent())
        }
        try? FileManager.default.createDirectory(at: symlinkContainer, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: ephemeralTargetDir, withIntermediateDirectories: true)
        let symlinkApp = symlinkContainer.appendingPathComponent("Micro Input.app")
        try? FileManager.default.createSymbolicLink(at: symlinkApp, withDestinationURL: ephemeralTargetDir)
        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: Self.prodBundleId,
                bundlePath: symlinkApp.path,
                environment: [:]
            ),
            .ephemeralBuildBundle(bundlePath: symlinkApp.path)
        )

        XCTAssertEqual(
            LaunchAtLoginPolicy.evaluate(
                bundleIdentifier: "com.example.other",
                bundlePath: Self.prodBundlePath,
                environment: [:]
            ),
            .wrongBundleIdentifier(actual: "com.example.other")
        )
    }

    @MainActor
    func testLaunchAtLoginControllerIdempotenceApprovalCleanupAndErrorHandling() {
        let driver = RecordingLaunchAtLoginDriver()
        let controller = LaunchAtLoginController(
            bundleIdentifier: Self.prodBundleId,
            bundlePath: Self.prodBundlePath,
            environment: [:],
            arguments: [],
            driver: driver
        )

        XCTAssertEqual(controller.state, .disabled)
        XCTAssertFalse(controller.isToggleOn)
        XCTAssertFalse(controller.isUnavailable)

        controller.setEnabled(false)
        XCTAssertEqual(driver.unregisterCount, 0)

        controller.setEnabled(true)
        XCTAssertEqual(driver.registerCount, 1)
        XCTAssertEqual(controller.state, .enabled)
        XCTAssertTrue(controller.isToggleOn)

        controller.setEnabled(true)
        XCTAssertEqual(driver.registerCount, 1)

        controller.setEnabled(false)
        XCTAssertEqual(driver.unregisterCount, 1)
        XCTAssertEqual(controller.state, .disabled)
        XCTAssertFalse(controller.isToggleOn)

        driver.statusAfterRegister = .requiresApproval
        controller.setEnabled(true)
        XCTAssertEqual(driver.registerCount, 2)
        XCTAssertEqual(controller.state, .requiresApproval)
        XCTAssertTrue(controller.isToggleOn)

        controller.openLoginItemsSettings()
        XCTAssertEqual(driver.openSettingsCount, 1)

        controller.setEnabled(false)
        XCTAssertEqual(driver.unregisterCount, 2)
        XCTAssertEqual(controller.state, .disabled)

        struct MockLoginError: LocalizedError {
            var errorDescription: String? { "Operation not permitted" }
        }
        driver.nextError = MockLoginError()
        controller.setEnabled(true)
        XCTAssertEqual(driver.registerCount, 3)
        XCTAssertEqual(controller.state, .disabled)
        XCTAssertEqual(controller.lastError, "Operation not permitted")

        driver.statusAfterRegister = .enabled
        controller.setEnabled(true)
        XCTAssertEqual(controller.state, .enabled)
        XCTAssertNil(controller.lastError)

        let gatedDriver = RecordingLaunchAtLoginDriver()
        let gated = LaunchAtLoginController(
            bundleIdentifier: Self.prodBundleId,
            bundlePath: "/var/folders/xx/target/MicroInput.app",
            environment: [:],
            arguments: [],
            driver: gatedDriver
        )
        XCTAssertTrue(gated.isUnavailable)
        gated.setEnabled(true)
        gated.openLoginItemsSettings()
        XCTAssertEqual(gatedDriver.registerCount, 0)
        XCTAssertEqual(gatedDriver.openSettingsCount, 0)

        let notFoundDriver = RecordingLaunchAtLoginDriver()
        notFoundDriver.simulatedStatus = .notFound
        let notFoundController = LaunchAtLoginController(
            bundleIdentifier: Self.prodBundleId,
            bundlePath: Self.prodBundlePath,
            environment: [:],
            arguments: [],
            driver: notFoundDriver
        )
        XCTAssertEqual(notFoundController.state, .disabled)
        XCTAssertFalse(notFoundController.isUnavailable)
        notFoundController.setEnabled(true)
        XCTAssertEqual(notFoundDriver.registerCount, 1)
        XCTAssertEqual(notFoundController.state, .enabled)

        notFoundDriver.simulatedStatus = .notRegistered
        notFoundController.setEnabled(true)
        XCTAssertEqual(notFoundDriver.registerCount, 2)
        XCTAssertEqual(notFoundController.state, .enabled)

        let missingDriverController = LaunchAtLoginController(
            bundleIdentifier: Self.prodBundleId,
            bundlePath: Self.prodBundlePath,
            environment: [:],
            arguments: [],
            driver: nil
        )
        XCTAssertEqual(missingDriverController.state, .unavailable(reason: .missingDriver))
        XCTAssertTrue(missingDriverController.isUnavailable)

        let fwSlice = FirmwareSliceModel(json: [
            "ports": [["path": "/dev/cu.usbmodem-test-101"]]
        ])
        XCTAssertEqual(fwSlice.ports.first?.product, "USB 设备")
        XCTAssertEqual(L10n.tr(fwSlice.ports.first?.product ?? "", .en), "USB Device")
        XCTAssertEqual(DesktopAgent.displayName("自定义方案", locale: .en), "Custom Profile")
        XCTAssertEqual(DesktopAgent.displayName("新建方案", locale: .en), "New Profile")
    }

    func testLaunchOriginDetectorAppleEventCliEnvAndSilentBootMatrix() {
        XCTAssertFalse(
            LaunchOriginDetector.wasLaunchedAtLogin(
                arguments: ["MicroInput"],
                environment: [:],
                currentAppleEvent: nil
            )
        )
        XCTAssertTrue(
            LaunchOriginDetector.wasLaunchedAtLogin(
                arguments: ["MicroInput", "--launched-at-login"],
                environment: [:],
                currentAppleEvent: nil
            )
        )
        XCTAssertTrue(
            LaunchOriginDetector.wasLaunchedAtLogin(
                arguments: ["MicroInput"],
                environment: ["MI_LAUNCHED_AT_LOGIN": "1"],
                currentAppleEvent: nil
            )
        )

        let loginEvent = NSAppleEventDescriptor.appleEvent(
            withEventClass: LaunchOriginDetector.openApplicationEventClass,
            eventID: LaunchOriginDetector.openApplicationEventID,
            targetDescriptor: nil,
            returnID: AEReturnID(kAutoGenerateReturnID),
            transactionID: AETransactionID(kAnyTransactionID)
        )
        loginEvent.setParam(
            NSAppleEventDescriptor(enumCode: LaunchOriginDetector.launchedAsLogInItemCode),
            forKeyword: LaunchOriginDetector.propDataKeyword
        )
        XCTAssertTrue(LaunchOriginDetector.isLaunchedAtLoginAppleEvent(loginEvent))
        XCTAssertTrue(
            LaunchOriginDetector.wasLaunchedAtLogin(
                arguments: ["MicroInput"],
                environment: [:],
                currentAppleEvent: loginEvent
            )
        )

        let regularOpenEvent = NSAppleEventDescriptor.appleEvent(
            withEventClass: LaunchOriginDetector.openApplicationEventClass,
            eventID: LaunchOriginDetector.openApplicationEventID,
            targetDescriptor: nil,
            returnID: AEReturnID(kAutoGenerateReturnID),
            transactionID: AETransactionID(kAnyTransactionID)
        )
        XCTAssertFalse(LaunchOriginDetector.isLaunchedAtLoginAppleEvent(regularOpenEvent))
        XCTAssertFalse(
            LaunchOriginDetector.wasLaunchedAtLogin(
                arguments: ["MicroInput"],
                environment: [:],
                currentAppleEvent: regularOpenEvent
            )
        )

        XCTAssertTrue(
            LaunchOriginDetector.shouldStartSilently(
                isVerifyMode: false,
                launchedAtLogin: true,
                silentAtLoginPref: true
            )
        )
        XCTAssertFalse(
            LaunchOriginDetector.shouldStartSilently(
                isVerifyMode: true,
                launchedAtLogin: true,
                silentAtLoginPref: true
            )
        )
        XCTAssertFalse(
            LaunchOriginDetector.shouldStartSilently(
                isVerifyMode: false,
                launchedAtLogin: false,
                silentAtLoginPref: true
            )
        )
        XCTAssertFalse(
            LaunchOriginDetector.shouldStartSilently(
                isVerifyMode: false,
                launchedAtLogin: true,
                silentAtLoginPref: false
            )
        )
    }

    @MainActor
    func testSilentAtLoginPreferencePersistsAcrossCoreStoreReload() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-login-pref-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }

        let wbURL = dir.appendingPathComponent("workbench.json")
        do {
            let seed = CoreStore(isolatedDirectory: dir.path)
            seed.dispatch("status_menu", ["RequestPresetExport": "Document"])
            seed.dispatch("status_menu", "ConfirmPresetExport")
            let presetText = try XCTUnwrap(seed.model.lastExportedPreset)
            let presetObj = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(presetText.utf8)) as? [String: Any])
            let docObj = try XCTUnwrap(presetObj["document"] as? [String: Any])
            try JSONSerialization.data(withJSONObject: docObj).write(to: wbURL)
        }

        let store = CoreStore(isolatedDirectory: dir.path)
        XCTAssertTrue(store.header.silentAtLogin)

        store.setSilentAtLogin(false)
        XCTAssertFalse(store.header.silentAtLogin)

        let reloaded = CoreStore(isolatedDirectory: dir.path)
        XCTAssertFalse(reloaded.header.silentAtLogin)
    }
}
