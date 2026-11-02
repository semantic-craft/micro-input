import XCTest
@testable import MicroInput
import MicroInputCore
#if MI_SYNTHETIC_BACKEND
final class ApplyPreparationTests: XCTestCase {
    @MainActor func testRealBridgePreviewsOneDeviceIntentAndBacksUpWithoutUSBOrDocumentChanges() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let core=MicroInputCore.MicroInput.syntheticDiscovery(),url=dir.appendingPathComponent("workbench.json")
        let store=Store(backend:core,documentURL:url);await store.loadWorkbench()
        try await core.setSyntheticDiscovery(state:.twoDevices);try await core.setSyntheticConfig(scenario:1)
        await store.refresh();store.selectDevice("synthetic-interface-0");await store.connect("synthetic-interface-0")
        await store.readIdentity();await store.readDeviceConfig();await store.createDraftFromSnapshot()
        await store.assign("device-key")
        let revision=store.draft.revision,requests=await core.syntheticRequests().count
        await store.previewApply();let preview=try XCTUnwrap(store.applyPreview)
        XCTAssertEqual(preview.logical_control,"logical-0");XCTAssertEqual(preview.after,"KC_B")
        XCTAssertFalse(preview.apply_available);XCTAssertFalse(preview.restore_available)
        XCTAssertTrue(preview.unavailable_reason.contains("UNKNOWN-08/09"))
        await store.prepareApplyBackup()
        let prepared=try XCTUnwrap(store.preparedApplyBackup)
        XCTAssertEqual(prepared.status,"prepared-not-sent")
        XCTAssertTrue(FileManager.default.fileExists(atPath:prepared.backup_path+"/manifest.json"))
        XCTAssertEqual(store.draft.revision,revision)
        let after=await core.syntheticRequests().count;XCTAssertEqual(after,requests)
        await store.cancelApplyPreview();XCTAssertNil(store.applyPreview)
        XCTAssertEqual(store.draft.revision,revision)
        await core.shutdown()
    }
    @MainActor func testReadOnlyAndStaleSnapshotsNeverPrepareThroughBridge() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let core=MicroInputCore.MicroInput.syntheticDiscovery(),store=Store(backend:core,documentURL:dir.appendingPathComponent("workbench.json"))
        await store.loadWorkbench();await store.previewApply();XCTAssertNil(store.applyPreview)
        try await core.setSyntheticDiscovery(state:.twoDevices);try await core.setSyntheticConfig(scenario:1)
        await store.refresh();store.selectDevice("synthetic-interface-0");await store.connect("synthetic-interface-0")
        await store.readIdentity();await store.readDeviceConfig();await store.createDraftFromSnapshot();await store.assign("device-key")
        await store.previewApply();let preview=try XCTUnwrap(store.applyPreview)
        await store.renameProfile(name:"changed revision")
        do {_ = try await core.prepareApplyBackup(token:preview.token);XCTFail("stale revision prepared")}catch {}
        await core.disconnect()
        do {_ = try await core.previewApply();XCTFail("offline cache prepared")}catch {}
        XCTAssertEqual(store.draft.revision,2)
        await core.shutdown()
    }
}
#endif
