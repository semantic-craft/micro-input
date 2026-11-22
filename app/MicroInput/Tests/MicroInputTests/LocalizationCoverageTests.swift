import AppKit
import SwiftUI
import XCTest
@testable import MicroInputUI

final class LocalizationCoverageTests: XCTestCase {
    func testEverySwiftTranslationCallSiteExistsInEnTableWithoutResidualCJK() throws {
        let sourcesDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("Sources")

        let enumerator = try XCTUnwrap(
            FileManager.default.enumerator(at: sourcesDir, includingPropertiesForKeys: nil)
        )

        let directPattern = try NSRegularExpression(pattern: #"(?:\.tr|\btr)\(\s*"((?:\\.|[^"\\])*)""#)
        let ternaryPattern = try NSRegularExpression(
            pattern: #"(?:\.tr|\btr)\([^?\n]*\?\s*"((?:\\.|[^"\\])*)"\s*:\s*"((?:\\.|[^"\\])*)""#
        )
        let hanPattern = try NSRegularExpression(pattern: #"\p{Han}"#)

        var extractedKeys = Set<String>()
        for case let fileURL as URL in enumerator where fileURL.pathExtension == "swift" {
            if fileURL.lastPathComponent == "Localization.swift" {
                continue
            }
            let content = try String(contentsOf: fileURL, encoding: .utf8)
            let nsRange = NSRange(content.startIndex..<content.endIndex, in: content)

            for match in directPattern.matches(in: content, range: nsRange) {
                if let r = Range(match.range(at: 1), in: content) {
                    extractedKeys.insert(String(content[r]))
                }
            }
            for match in ternaryPattern.matches(in: content, range: nsRange) {
                if let r1 = Range(match.range(at: 1), in: content) {
                    extractedKeys.insert(String(content[r1]))
                }
                if let r2 = Range(match.range(at: 2), in: content) {
                    extractedKeys.insert(String(content[r2]))
                }
            }
        }

        XCTAssertGreaterThan(extractedKeys.count, 250)

        let missing = extractedKeys.filter { L10n.enTable[$0] == nil }.sorted()
        XCTAssertTrue(missing.isEmpty, "Missing L10n.enTable entries for keys: \(missing)")

        for (zhKey, enValue) in L10n.enTable {
            XCTAssertFalse(enValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, "Empty English value for \(zhKey)")
            let range = NSRange(enValue.startIndex..<enValue.endIndex, in: enValue)
            XCTAssertNil(
                hanPattern.firstMatch(in: enValue, range: range),
                "Residual CJK characters in English translation for '\(zhKey)': '\(enValue)'"
            )
        }

        for dest in WorkbenchDestination.allCases {
            let enTitle = L10n.destinationTitle(dest.rawValue, .en)
            let range = NSRange(enTitle.startIndex..<enTitle.endIndex, in: enTitle)
            XCTAssertNil(hanPattern.firstMatch(in: enTitle, range: range), "Destination \(dest) missing English title")
        }

        for reason in [
            HostGateReason.qaVerifyMode,
            .unbundledExecutable,
            .ephemeralBuildBundle(bundlePath: "/tmp/build/MicroInput.app"),
            .wrongBundleIdentifier(actual: "com.example.wrong"),
            .disabledByEnvironment(variable: "MI_DISABLE_LOGIN_ITEM"),
            .missingInfoPlistKey(key: "SUFeedURL"),
            .translocatedOrMountedVolume(bundlePath: "/Volumes/Test/Micro Input.app"),
            .missingDriver,
            .unsupportedOS,
        ] {
            let enText = reason.localizedDescription(.en)
            let range = NSRange(enText.startIndex..<enText.endIndex, in: enText)
            XCTAssertNil(hanPattern.firstMatch(in: enText, range: range), "HostGateReason \(reason) has CJK in English: \(enText)")
            XCTAssertFalse(reason.localizedDescription(.zhHans).isEmpty)
        }
    }

    @MainActor
    func testLiveLocaleSwitchingSystemSyncTypedFieldsAndRenderIsolation() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("mi-i18n-live-\(UUID().uuidString)")
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
        XCTAssertEqual(store.header.uiLanguage, .system)
        XCTAssertEqual(store.locale, .zhHans)
        XCTAssertEqual(store.inspector.inspectorLayerName, "基础层")

        store.syncSystemLocale(["en-US", "zh-Hans-CN"])
        XCTAssertEqual(store.locale, .en)
        XCTAssertEqual(store.inspector.inspectorLayerName, "Base Layer")
        XCTAssertTrue(store.sidebar.sidebarRows.contains(where: { $0.displayName == "Other Apps" }))
        XCTAssertEqual(DesktopAgent.named("Other Apps")?.symbol, "keyboard")

        XCTAssertNil(store.footer.lastExportedDiagnosticsPath)
        store.dispatch("device", "ExportDiagnostics")
        let exportedPath = try XCTUnwrap(store.footer.lastExportedDiagnosticsPath)
        XCTAssertTrue(FileManager.default.fileExists(atPath: exportedPath))
        XCTAssertTrue(store.footer.statusMessage.hasPrefix("Exported diagnostic report: "))

        store.setUiLanguage(.zhHans)
        XCTAssertEqual(store.header.uiLanguage, .zhHans)
        XCTAssertEqual(store.locale, .zhHans)
        XCTAssertEqual(store.inspector.inspectorLayerName, "基础层")

        store.setUiLanguage(.en)
        XCTAssertEqual(store.header.uiLanguage, .en)
        XCTAssertEqual(store.locale, .en)
        XCTAssertEqual(store.inspector.editableInspector?.actionCategoryFilter, "All")

        store.dispatch("canvas", ["SelectLogicalSlot": 16])
        store.dispatch("inspector", ["SetJoystickMode": "mouse"])
        XCTAssertEqual(store.canvas.joystickMode, "mouse")
        XCTAssertEqual(store.statusMenu.cheatSheetPreview?.joystickMode, "mouse")
        store.dispatch("inspector", ["SetJoystickMode": "logical_sectors"])
        XCTAssertEqual(store.canvas.joystickMode, "logical_sectors")
        store.dispatch("canvas", ["SelectLogicalSlot": 6])

        let workbench = NSHostingView(rootView: ClassicWorkbenchView(store: store).frame(width: 1180, height: 740))
        workbench.layoutSubtreeIfNeeded()

        for query in ["t", "te", "ter", "term", ""] {
            let invalidated = trackSliceInvalidations(on: store) {
                store.dispatch("inspector", ["SetActionSearchQuery": query])
            }
            workbench.layoutSubtreeIfNeeded()
            XCTAssertEqual(
                invalidated,
                ["inspector"],
                "English inspector action search must invalidate only store.inspector, got \(invalidated)"
            )
        }

        let reloaded = CoreStore(isolatedDirectory: dir.path)
        XCTAssertEqual(reloaded.header.uiLanguage, .en)
        XCTAssertEqual(reloaded.locale, .en)
    }
}
