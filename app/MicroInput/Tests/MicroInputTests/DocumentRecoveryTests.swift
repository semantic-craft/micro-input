import XCTest
@testable import MicroInput
import MicroInputCore

final class DocumentRecoveryTests: XCTestCase {
    @MainActor func testCorruptStartupRecoversLegacyCopyAndSaveAsThroughRealBridge() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true)
        let url=dir.appendingPathComponent("workbench.json"), source=dir.appendingPathComponent("legacy.backup"), core=MicroInputCore.MicroInput()
        let config=try JSONSerialization.jsonObject(with:Data(try await core.loadDemo().utf8))
        let legacy=try JSONSerialization.data(withJSONObject:["schema_version":1,"config":config,"edits":[:],"revision":7])
        try legacy.write(to:source); try Data("{broken".utf8).write(to:url)
        let store=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:core,documentURL:url); await store.loadWorkbench()
        XCTAssertFalse(store.documentReady)
        let files=try await store.recoveryFiles(); XCTAssertTrue(files.contains(source.path))
        let preview=try await store.inspectRecovery(path:source.path)
        XCTAssertTrue(preview.can_restore); XCTAssertEqual(preview.schema_version,1)
        let recovered = await store.recoverDocument(token:preview.token); XCTAssertTrue(recovered)
        XCTAssertTrue(store.documentReady); XCTAssertEqual(store.draft.loaded_format,2)
        await store.renameProfile(name:"Recovered local")
        let before=store.draft.document_id
        let saved = await store.saveDocumentAs(); XCTAssertTrue(saved); XCTAssertNotEqual(store.draft.document_id,before)
        XCTAssertFalse(store.draft.can_undo)
        let reopened=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:url); await reopened.loadWorkbench()
        XCTAssertEqual(reopened.profile?.name,"Recovered local")
        XCTAssertEqual(try Data(contentsOf:url),Data("{broken".utf8)); XCTAssertEqual(try Data(contentsOf:source),legacy)
        XCTAssertNil(store.connectedID); XCTAssertTrue(store.devices.isEmpty)
    }
    @MainActor func testFutureVersionCopyAndChangedPreviewAreProtectedThroughRealBridge() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at:dir) }
        try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true)
        let url=dir.appendingPathComponent("workbench.json")
        let bytes=Data("{\"schema_version\":18446744073709551615,\"future\":true}".utf8)
        try bytes.write(to:url)
        let store=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:url); await store.loadWorkbench()
        let preview=try await store.inspectRecovery(path:url.path)
        XCTAssertFalse(preview.can_restore); XCTAssertEqual(preview.schema_version.map(String.init),"18446744073709551615")
        let restored=await store.recoverDocument(token:preview.token); XCTAssertFalse(restored)
        let copy=dir.appendingPathComponent("original.copy")
        try await store.copyRecoveryOriginal(token:preview.token,destination:copy.path)
        XCTAssertEqual(try Data(contentsOf:copy),bytes)
        try Data("changed".utf8).write(to:url)
        do { try await store.copyRecoveryOriginal(token:preview.token,destination:dir.appendingPathComponent("stale.copy").path); XCTFail("Changed preview must be rejected") } catch {}
        XCTAssertFalse(store.documentReady)
        XCTAssertFalse(FileManager.default.fileExists(atPath:dir.appendingPathComponent("workbench.active.json").path))
    }

}
