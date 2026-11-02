import XCTest
@testable import MicroInput
import MicroInputCore
#if MI_SYNTHETIC_BACKEND
final class ReconnectComparisonTests: XCTestCase {
    @MainActor func testRealBridgeComparesCachedSnapshotCancelsAndAdoptsDeviceAcrossRestart() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let url=dir.appendingPathComponent("workbench.json"), core=MicroInputCore.MicroInput.syntheticDiscovery()
        let store=Store(backend:core,documentURL:url)
        await store.loadWorkbench();try await core.setSyntheticDiscovery(state:.twoDevices);try await core.setSyntheticConfig(scenario:1)
        await store.refresh();store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0");await store.readIdentity();await store.readDeviceConfig();await store.createDraftFromSnapshot()
        await store.renameProfile(name:"Local only name")
        let beforeRevision=store.draft.revision
        try await core.setSyntheticConfig(scenario:4)
        await store.readDeviceConfig()
        let count=await core.syntheticRequests().count
        await store.compareSnapshot()
        XCTAssertNotNil(store.comparison);XCTAssertFalse(store.comparison?.can_merge ?? true)
        XCTAssertEqual(store.comparison?.rows.first(where:{$0.result=="冲突"})?.local?.text,"Local only name")
        await store.resolveComparison("cancel")
        XCTAssertEqual(store.draft.structure_changes.count,1);XCTAssertEqual(store.draft.revision,beforeRevision);XCTAssertTrue(store.draft.can_undo)
        await store.compareSnapshot();await store.resolveComparison("device")
        XCTAssertEqual(store.profile?.name,"Synthetic external edit");XCTAssertEqual(store.draft.revision,beforeRevision+1)
        XCTAssertFalse(store.draft.can_undo);XCTAssertTrue(store.draft.structure_changes.isEmpty)
        let finalCount=await core.syntheticRequests().count;XCTAssertEqual(finalCount,count)
        let reopened=Store(backend:MicroInputCore.MicroInput(),documentURL:url);await reopened.loadWorkbench()
        XCTAssertEqual(reopened.profile?.name,"Synthetic external edit");XCTAssertEqual(reopened.draft.revision,beforeRevision+1)
        await core.shutdown()
    }
    @MainActor func testChangedCachedSnapshotRejectsOldDecisionAndWholeLocalChoiceReopensWithDiff() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let url=dir.appendingPathComponent("workbench.json"),core=MicroInputCore.MicroInput.syntheticDiscovery()
        let store=Store(backend:core,documentURL:url);await store.loadWorkbench()
        try await core.setSyntheticDiscovery(state:.twoDevices);try await core.setSyntheticConfig(scenario:1)
        await store.refresh();store.selectDevice("synthetic-interface-0");await store.connect("synthetic-interface-0")
        await store.readIdentity();await store.readDeviceConfig();await store.createDraftFromSnapshot()
        await store.renameProfile(name:"Whole local choice")
        await store.compareSnapshot();let old=try XCTUnwrap(store.comparison),revision=store.draft.revision
        try await core.setSyntheticConfig(scenario:4);_ = try await core.readConfig()
        do {_ = try await core.resolveComparison(token:old.token,choice:"device");XCTFail("Stale comparison was accepted")}catch {}
        XCTAssertEqual(store.draft.revision,revision)
        await store.compareSnapshot();await store.resolveComparison("local")
        XCTAssertEqual(store.profile?.name,"Whole local choice");XCTAssertFalse(store.draft.can_undo)
        XCTAssertEqual(store.draft.revision,revision+1)
        XCTAssertEqual(store.draft.structure_changes,["配置改名：Synthetic external edit → Whole local choice"])
        let reopened=Store(backend:MicroInputCore.MicroInput(),documentURL:url);await reopened.loadWorkbench()
        XCTAssertEqual(reopened.profile?.name,"Whole local choice");XCTAssertEqual(reopened.draft.structure_changes,store.draft.structure_changes)
        await core.shutdown()
    }

}
#endif
