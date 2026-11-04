import XCTest
import MicroInputCore
@testable import MicroInput

final class MenuFavoritesTests: XCTestCase {
    @MainActor func testDefaultsAndStalePreferencesResolveAgainstCurrentProfiles() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: directory.appendingPathComponent("workbench.json"), actionExecutor: RecordingHostActionExecutor())
        await store.loadWorkbench()
        await store.installCodingAgentPresets()
        let profiles = store.hostProfiles
        XCTAssertEqual(MenuFavorites.resolve(ids: [], profiles: profiles).map(\.name), MenuFavorites.defaults)
        let zcode = try XCTUnwrap(profiles.first { $0.name == "ZCode" })
        let resolved = MenuFavorites.resolve(ids: ["deleted", zcode.id.text, zcode.id.text], profiles: profiles)
        XCTAssertEqual(resolved.map(\.name), ["ZCode", "Antigravity", "DeepSeek Coder (DSH)", "Cindy"])
        await store.shutdown()
    }
    func testSelectingAlreadyPinnedProfileSwapsPositions() {
        XCTAssertEqual(MenuFavorites.replacing(ids: ["ag", "dsh", "cindy", "codex"], at: 0, with: "cindy"), ["cindy", "dsh", "ag", "codex"])
        XCTAssertEqual(MenuFavorites.replacing(ids: ["ag", "dsh", "cindy", "codex"], at: 3, with: "qwen"), ["ag", "dsh", "cindy", "qwen"])
        XCTAssertEqual(MenuFavorites.replacing(ids: ["ag"], at: 3, with: "qwen"), ["ag"])
    }
}
