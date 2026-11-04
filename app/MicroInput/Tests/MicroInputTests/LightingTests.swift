import XCTest
@testable import MicroInput
import MicroInputCore
private final class EndedPreviewBackend: Backend {
    var phase = "Restored"
    var delayBegin = false
    private let lock = NSLock()
    private var pendingBegin: CheckedContinuation<String, Never>?
    var beginning: Bool { lock.withLock { pendingBegin != nil } }
    func releaseBegin() { lock.withLock { pendingBegin?.resume(returning: "late-token"); pendingBegin = nil } }
    func beginLightingPreview(layerId: String) async throws -> String {
        if delayBegin { return await withCheckedContinuation { continuation in lock.withLock { pendingBegin = continuation } } }
        return "token"
    }
    var failEnd = false
    var endCalls = 0
    var deviceReads = 0
    func loadDemo() async throws -> String { "{}" }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws { XCTFail("unexpected connection") }
    func readConfig() async throws -> String { deviceReads += 1; return "{}" }
    func disconnect() async {}
    func inputMonitoringStatus() async -> Bool { false }
    func lightingPreviewStatus() -> String { "{\"phase\":\"\(phase)\"}" }
    func endLightingPreview(token: String) async throws -> String {
        endCalls += 1
        if failEnd { throw BridgeError.Failed(message: "stop unconfirmed") }
        return lightingPreviewStatus()
    }
}
final class LightingTests: XCTestCase {
    @MainActor func testAutomaticEndUpdatesUIWithoutDevicePolling() async {
        let backend = EndedPreviewBackend()
        let observed = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        observed.lightingPreviewToken = "synthetic-token"
        observed.lightingPreviewPhase = "Active"
        await observed.observeLightingPreview()
        XCTAssertNil(observed.lightingPreviewToken)
        XCTAssertEqual(observed.lightingPreviewPhase,"Restored")
        XCTAssertTrue(observed.lightingPreviewResult.contains("合成灯光已恢复"))
        XCTAssertEqual(backend.endCalls,1)
        XCTAssertEqual(backend.deviceReads,0)
        await observed.endLightingPreview()
        XCTAssertEqual(backend.endCalls,1)
        await observed.shutdown()
    }
    @MainActor func testNativeStopDoesNotClaimRestoration() async {
        let backend = EndedPreviewBackend(); backend.phase = "Stopped"
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        store.lightingPreviewToken = "native-token"
        await store.endLightingPreview()
        XCTAssertEqual(store.lightingPreviewPhase, "Stopped")
        XCTAssertTrue(store.lightingPreviewResult.contains("不自动恢复"))
        XCTAssertEqual(backend.deviceReads, 0)
        await store.shutdown()
    }
    @MainActor func testCloseDuringBeginRetiresLateToken() async throws {
        let backend = EndedPreviewBackend(); backend.delayBegin = true; backend.phase = "Stopped"
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        let core = MicroInputCore.MicroInput()
        store.snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(try await core.loadDemo().utf8))
        let begin = Task { await store.beginLightingPreview() }
        for _ in 0..<100 where !backend.beginning { await Task.yield() }
        XCTAssertTrue(backend.beginning)
        await store.endLightingPreview()
        backend.releaseBegin()
        await begin.value
        XCTAssertNil(store.lightingPreviewToken)
        XCTAssertEqual(backend.endCalls, 1)
        await store.shutdown(); await core.shutdown()
    }
    @MainActor func testOwnershipDrainRetainsLatePreviewWhenStopFails() async throws {
        let backend = EndedPreviewBackend(); backend.delayBegin = true; backend.failEnd = true
        let ownership = NativeOwnershipController(transport: nil)
        ownership.setDesiredOwner("micro-input")
        let request = ownership.beginRequest(microDrained: true)
        ownership.receive(NativeOwnershipReply(version: 1, requestId: request.requestId,
            clientInstance: request.clientInstance, generation: request.generation,
            nativeInstance: UUID(), nativeGeneration: 1, state: "released", reason: "fixture",
            inputDrained: true, pttDrained: true, lightingDrained: true, capabilities: ["fenced-release-v1"]), for: request)
        let store = Store(nativeOwnership: ownership, backend: backend)
        let core = MicroInputCore.MicroInput()
        store.snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(try await core.loadDemo().utf8))
        let begin = Task { await store.beginLightingPreview() }
        for _ in 0..<100 where !backend.beginning { await Task.yield() }
        XCTAssertTrue(backend.beginning)
        ownership.invalidate("connection lost")
        backend.releaseBegin()
        await begin.value
        XCTAssertEqual(store.lightingPreviewToken, "late-token")
        XCTAssertFalse(store.lightingCleanupConfirmed)
        let drained = await store.drainForNativeOwnership()
        XCTAssertFalse(drained)
        backend.failEnd = false; backend.phase = "Stopped"
        let retry = await store.drainForNativeOwnership()
        XCTAssertTrue(retry)
        await store.shutdown(); await core.shutdown()
    }
    @MainActor func testNativePreviewIsClosedWithoutConnection() async {
        let core = MicroInputCore.MicroInput()
        do { _ = try await core.beginLightingPreview(layerId: "missing"); XCTFail("native preview opened") } catch {}
        await core.shutdown()
    }
    #if MI_SYNTHETIC_BACKEND
    @MainActor private func setup(_ directory: URL) async throws -> (MicroInputCore.MicroInput, Store) {
        let core = MicroInputCore.MicroInput.syntheticConfigWriter()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core, documentURL: directory.appendingPathComponent("workbench.json"), actionExecutor: RecordingHostActionExecutor())
        await store.loadWorkbench(); try await core.setSyntheticDiscovery(state: .twoDevices)
        await store.refresh(); await store.connect("synthetic-interface-0"); await store.readIdentity(); await store.readDeviceConfig(); await store.createDraftFromSnapshot()
        return (core,store)
    }
    @MainActor func testBothZonesAllEffectsInvalidValuesUndoAndReopen() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let (core,store)=try await setup(dir)
        let original=try XCTUnwrap(store.lightingModel())
        let before=await core.syntheticRequests().count
        for zone in ["backlight","underglow"] {
            for effect in ["off","solid","snake","rainbow","breath","gradient"] {
                await store.editLighting(zone:zone,field:"effect",value:effect,allLayers:false)
                XCTAssertEqual(store.lightingModel()?.zones[zone]?["effect"]?.value,effect,store.status)
                XCTAssertEqual(store.lightingModel()?.zones[zone]?["brightness"]?.editable,effect != "off")
                XCTAssertEqual(store.lightingModel()?.zones[zone]?["speed"]?.editable,["snake","rainbow","breath"].contains(effect))
            }
        }
        let revision=store.draft.revision
        await store.editLighting(zone:"backlight",field:"color",value:"#INVALID",allLayers:false)
        XCTAssertEqual(store.draft.revision,revision)
        await store.editLighting(zone:"backlight",field:"color",value:"#ABCDEF",allLayers:false)
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value,"#ABCDEF")
        await store.undoDraft(); XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value,original.zones["backlight"]?["color"]?.value)
        let after=await core.syntheticRequests().count;XCTAssertEqual(before,after)
        await store.shutdown()
        let reopened=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:dir.appendingPathComponent("workbench.json"))
        await reopened.loadWorkbench();XCTAssertEqual(reopened.lightingModel()?.zones["underglow"]?["effect"]?.value,"gradient")
        await reopened.shutdown()
    }
    @MainActor func testAllLayerEditIsOneUndoAndOtherZoneIsUnchanged() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let (_,store)=try await setup(dir)
        await store.duplicateLayer(name:"Second")
        let count=store.profile?.layers.count ?? 0
        XCTAssertEqual(count,2)
        let revision=store.draft.revision
        await store.editLighting(zone:"backlight",field:"brightness",value:"32",allLayers:true)
        XCTAssertEqual(store.draft.revision,revision+1)
        for index in 0..<count {store.selectLayer(index);XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["brightness"]?.value,"32");XCTAssertEqual(store.lightingModel()?.zones["underglow"]?["brightness"]?.value,"60")}
        await store.undoDraft()
        for index in 0..<count {store.selectLayer(index);XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["brightness"]?.value,"75")}
        await store.shutdown()
    }
    @MainActor func testPreviewDoesNotEditDocumentAndBlocksApplyUntilRestored() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let (core,store)=try await setup(dir)
        let revision=store.draft.revision
        await store.beginLightingPreview();XCTAssertNotNil(store.lightingPreviewToken,store.lightingPreviewResult)
        XCTAssertThrowsError(try core.beginDeviceDrain())
        await store.editLighting(zone:"backlight",field:"color",value:"#ABCDEF",allLayers:false)
        try await Task.sleep(nanoseconds:200_000_000)
        XCTAssertEqual(store.draft.revision,revision)
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value,"#ABCDEF")
        await store.editLighting(zone:"backlight",field:"effect",value:"rainbow",allLayers:false)
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.editable,false)
        await store.endLightingPreview();XCTAssertEqual(store.lightingPreviewPhase,"Restored")
        XCTAssertEqual(store.draft.revision,revision)
        let drain=try core.beginDeviceDrain();core.abandonDeviceDrain(token:drain)
        await store.shutdown()
    }
    @MainActor func testPreviewCanBeAdoptedAsOneUndoableDraftChange() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let (_, store) = try await setup(dir)
        let revision = store.draft.revision
        let original = store.lightingModel()?.zones["backlight"]?["color"]?.value
        await store.beginLightingPreview()
        await store.editLighting(zone: "backlight", field: "color", value: "#ABCDEF", allLayers: false)
        await store.saveLightingPreview()
        XCTAssertNil(store.lightingPreviewToken)
        XCTAssertEqual(store.draft.revision, revision + 1)
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value, "#ABCDEF")
        await store.previewApply()
        XCTAssertNotNil(store.applyPreview, store.status)
        await store.undoDraft()
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value, original)
        await store.shutdown()
    }
    @MainActor func testDisconnectRetiresOldPreviewGateWithoutReplay() async throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: dir) }
        let (core, store) = try await setup(dir)
        await store.beginLightingPreview()
        let token = try XCTUnwrap(store.lightingPreviewToken)
        await core.disconnect()
        await store.endLightingPreview()
        XCTAssertNil(store.lightingPreviewToken)
        XCTAssertEqual(store.lightingPreviewPhase, "Unknown")
        try await core.connect(id: "synthetic-interface-0")
        _ = try await core.readIdentity()
        let drain = try core.beginDeviceDrain()
        core.abandonDeviceDrain(token: drain)
        XCTAssertThrowsError(try core.updateLightingPreview(token: token, zone: "backlight", field: "color", value: "#ABCDEF"))
        await store.beginLightingPreview()
        XCTAssertNotNil(store.lightingPreviewToken, store.lightingPreviewResult)
        await store.endLightingPreview()
        await store.shutdown()
    }
    @MainActor func testRGBApplyAndRestoreThroughExistingWorkflow() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let (core,store)=try await setup(dir)
        await store.editLighting(zone:"backlight",field:"color",value:"#123ABC",allLayers:false)
        await store.previewApply();await store.prepareApplyBackup()
        let token=try XCTUnwrap(store.preparedApplyBackup?.token,store.status)
        await store.performDeviceOperation(token:token);XCTAssertEqual(store.operationResult?.phase,"Verified",store.status)
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value,"#123ABC")
        await store.prepareDeviceRestore(try XCTUnwrap(store.operationResult?.operation_id))
        await store.performDeviceOperation(token:try XCTUnwrap(store.restorePreview?.token),restore:true)
        XCTAssertEqual(store.operationResult?.phase,"Verified",store.status)
        // Restoring the device retains the local desired draft; verify the device snapshot separately.
        XCTAssertEqual(store.lightingModel()?.zones["backlight"]?["color"]?.value,"#123ABC")
        let snapshot = try JSONDecoder().decode(Snapshot.self, from: Data(try await core.readConfig().utf8))
        guard case .object(let zones) = snapshot.profiles[0].layers[0].lights,
              case .object(let backlight) = zones["backlight"] else { return XCTFail("missing lights") }
        XCTAssertEqual(backlight["color"]?.text,"1193046")
        await core.shutdown()
    }
    #endif
}
