import XCTest
@testable import MicroInput
import MicroInputCore

@MainActor final class ProtectedHostLayerTests: XCTestCase {
    func testProtectedOaiLayerDetection() {
        let normal = Layer(
            id: .string("layer-normal"),
            name: "Normal Layer",
            color: .integer(100),
            keys: (0..<13).map { _ in JSONValue.string("KC_A") },
            encoders: (0..<3).map { _ in JSONValue.string("KC_B") },
            joystick: Joystick(kind: .string("sectors"), sectors: []),
            lights: .null,
            is_protected: false
        )
        XCTAssertFalse(normal.isProtectedOaiLayer)

        let protectedKey = Layer(
            id: .string("layer-key"),
            name: "Key Layer",
            color: .integer(100),
            keys: [JSONValue.string("KV_OAI_AG00")] + (1..<13).map { _ in JSONValue.string("KC_A") },
            encoders: (0..<3).map { _ in JSONValue.string("KC_B") },
            joystick: Joystick(kind: .string("sectors"), sectors: []),
            lights: .null,
            is_protected: true
        )
        XCTAssertTrue(protectedKey.isProtectedOaiLayer)

        let protectedEncoder = Layer(
            id: .string("layer-enc"),
            name: "Encoder Layer",
            color: .integer(100),
            keys: (0..<13).map { _ in JSONValue.string("KC_A") },
            encoders: [JSONValue.string("KV_OAI_ENC_CLK"), JSONValue.string("KC_B"), JSONValue.string("KC_B")],
            joystick: Joystick(kind: .string("sectors"), sectors: []),
            lights: .null,
            is_protected: nil as Bool?
        )
        XCTAssertTrue(protectedEncoder.isProtectedOaiLayer)
    }

    func testProtectedLayerSelectionLocksUIAndDisablesEdits() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let core = MicroInput()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: dir.appendingPathComponent("workbench.json"))
        await store.loadWorkbench()

        XCTAssertFalse(store.isProtectedLayerSelected)
        XCTAssertTrue(store.canEditCurrentLayer)

        await store.createProtectedOaiLayer(name: "ChatGPT")
        guard let protectedIndex = store.profile?.layers.firstIndex(where: { $0.isProtectedOaiLayer }) else {
            XCTFail("Protected layer not found in profile")
            return
        }

        store.selectLayer(protectedIndex)
        XCTAssertTrue(store.isProtectedLayerSelected)
        XCTAssertFalse(store.canEditCurrentLayer)
        XCTAssertNil(store.selectedBinding)
        XCTAssertFalse(store.showingLighting)

        await store.duplicateLayer(name: "ChatGPT Copy")
        XCTAssertTrue(store.status.contains("受保护"))

        do {
            _ = try await store.presetText(scope: "layer")
            XCTFail("Exporting protected layer must throw")
        } catch {
        }

        let action = CatalogAction(id: "search", name: "Search", category: "快捷键", detail: "test", payload: [:])
        let saved = await store.saveAction(action)
        XCTAssertFalse(saved)
        XCTAssertTrue(store.status.contains("受保护"))

        let savedSettings = await store.saveControlSettings(mode: "logical_sectors", sectors: "4")
        XCTAssertFalse(savedSettings)
        XCTAssertNotNil(store.controlSettingsError)
    }

    func testMultiProfileOwnershipHandoff() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let core = MicroInput()
        let ownership = NativeOwnershipController(transport: nil)
        let store = Store(nativeOwnership: ownership, backend: core, documentURL: dir.appendingPathComponent("workbench.json"))
        await store.loadWorkbench()
        await store.installCodingAgentPresets()

        XCTAssertTrue(store.isProtectedHostProfile("cindy"))
        XCTAssertTrue(store.isProtectedHostProfile("codex"))
        XCTAssertFalse(store.isProtectedHostProfile("antigravity"))
        XCTAssertFalse(store.isProtectedHostProfile("zcode"))
        XCTAssertFalse(store.isProtectedHostProfile("dsh"))
        XCTAssertFalse(store.isProtectedHostProfile("herdr"))

        guard let cindy = store.hostProfiles.first(where: { $0.name == "Cindy" }),
              let ag = store.hostProfiles.first(where: { $0.name == "Antigravity" }) else {
            XCTFail("Agent profiles missing")
            return
        }

        await store.setHostActiveProfile(cindy.id.text)
        XCTAssertEqual(store.hostActiveProfileId, cindy.id.text)

        await store.setHostActiveProfile(ag.id.text)
        XCTAssertEqual(store.hostActiveProfileId, ag.id.text)
    }

    func testCompactHostKeymapStaysUnder1024Bytes() throws {
        let core = MicroInput()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        let compact = try store.compileCompactHostKeymap()
        XCTAssertLessThan(compact.utf8.count, 1024)
        XCTAssertTrue(compact.contains("KV_OAI_AG00"))
    }
}
