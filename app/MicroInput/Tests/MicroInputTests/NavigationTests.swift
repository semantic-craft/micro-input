import XCTest
import AppKit
@testable import MicroInput
import MicroInputCore
private final class SpyBackend: Backend {
    let core = MicroInputCore.MicroInput()
    var reads = 0
    var connects = 0
    var fail = false
    func loadDemo() async throws -> String { try await core.loadDemo() }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws { connects += 1 }
    func readConfig() async throws -> String { reads += 1; throw NSError(domain: "协议未验证", code: 1) }
    func disconnect() async {}
    func inputMonitoringStatus() async -> Bool { false }
}
final class NavigationTests: XCTestCase {
    @MainActor func testRustBridgeAndNavigationNeverRequestsDevice() async throws {
        let backend = SpyBackend(); let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        await store.loadDemo()
        XCTAssertEqual(store.snapshot?.source, "Demo")
        XCTAssertEqual(store.snapshot?.profiles.count, 2)
        XCTAssertEqual(store.layer?.keys.count, 13)
        XCTAssertEqual(store.layer?.encoders.count, 3)
        store.selectProfile(1)
        XCTAssertNotNil(UUID(uuidString: store.profile?.id.text ?? ""))
        XCTAssertTrue(store.snapshot?.raw.text.contains("42") == true)
        XCTAssertEqual(store.profile?.index, 1)
        store.selectProfile(0); store.selectLayer(1)
        XCTAssertEqual(store.layer?.name, "辅助层")
        XCTAssertEqual(backend.reads, 0); XCTAssertEqual(backend.connects, 0)
        XCTAssertTrue(store.layer?.keys[1].bindingLabel.contains("Unknown") == true)
    }
    @MainActor func testFailedDeviceReadRetainsDemoProvenance() async {
        let backend = SpyBackend(); let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        await store.loadDemo(); await store.connect("synthetic"); await store.readDeviceConfig()
        XCTAssertEqual(store.snapshot?.source, "Demo")
        XCTAssertTrue(store.stale)
        XCTAssertTrue(store.status.contains("读取失败"))
        XCTAssertEqual(backend.reads, 1)
        store.received([])
        XCTAssertNil(store.connectedID)
        XCTAssertEqual(store.status, "设备已断开")
    }
}
private final class DelayedBackend: Backend {
    // Backend async methods run off MainActor; publish the continuation atomically.
    private let lock = NSLock()
    private var pendingRelease: CheckedContinuation<Void, Never>?
    var release: CheckedContinuation<Void, Never>? {
        get { lock.withLock { pendingRelease } }
        set { lock.withLock { pendingRelease = newValue } }
    }
    var reads = 0
    func loadDemo() async throws -> String { "{}" }
    func listDevices() async throws -> [DeviceInfo] { [] }
    func connect(id: String) async throws { await withCheckedContinuation { release = $0 } }
    func readConfig() async throws -> String { reads += 1; return "{}" }
    func disconnect() async {}
    func inputMonitoringStatus() async -> Bool { false }
}
extension NavigationTests {
    @MainActor func testSuspendInvalidatesDelayedConnect() async {
        let backend = DelayedBackend(); let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        let task = Task { await store.connect("delayed") }
        while backend.release == nil { await Task.yield() }
        await store.suspend()
        backend.release?.resume(); await task.value
        XCTAssertNil(store.connectedID); XCTAssertEqual(backend.reads, 0)
        XCTAssertEqual(store.status, "会话已断开")
    }
    func testUnknownLargeIntegersAreExact() throws {
        for number in ["9007199254740993", "18446744073709551615", "-9007199254740993"] {
            let value = try JSONDecoder().decode(JSONValue.self, from: Data(number.utf8))
            XCTAssertEqual(value.text, number)
        }
    }
}
extension NavigationTests {
    @MainActor func testRemovalInvalidatesDelayedConnect() async {
        let backend = DelayedBackend(); let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: backend)
        let task = Task { await store.connect("delayed") }
        while backend.release == nil { await Task.yield() }
        store.received([])
        backend.release?.resume(); await task.value
        XCTAssertNil(store.connectedID); XCTAssertEqual(backend.reads, 0)
        XCTAssertEqual(store.status, "设备已断开")
    }
}
extension NavigationTests {
    func testProtocolFailureHasReadableStatus() {
        XCTAssertEqual(readableError(BridgeError.Failed(message: "协议未验证：UNKNOWN-04 fs schema")), "协议未验证，设备读取待确认")
    }
}

extension NavigationTests {
    @MainActor func testLegacySavedActionMigratesThroughRealBridgeWithOriginalBytesBackup() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true)
        let url=dir.appendingPathComponent("workbench.json"), core=MicroInputCore.MicroInput()
        let config=try JSONSerialization.jsonObject(with:Data(try await core.loadDemo().utf8)) as! [String:Any]
        let first=(config["profiles"] as! [[String:Any]])[0], layer=(first["layers"] as! [[String:Any]])[0]
        let keyData=try JSONSerialization.data(withJSONObject:[first["id"]!,layer["id"]!,"logical-1"],options:[.fragmentsAllowed,.sortedKeys,.withoutEscapingSlashes])
        let key=String(decoding:keyData,as:UTF8.self)
        let action:[String:Any] = ["id":"review","name":"Saved text","category":"文本","detail":"local","payload":["type":"text","text":"保留参数\n第二行"]]
        let bytes=try JSONSerialization.data(withJSONObject:["schema_version":1,"config":config,"edits":[key:action],"revision":7],options:[.sortedKeys])
        try bytes.write(to:url)
        let store=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:core,documentURL:url);await store.loadWorkbench()
        XCTAssertEqual(try Data(contentsOf:url),bytes);XCTAssertEqual(store.draft.loaded_format,1)
        store.slot=1;XCTAssertEqual(store.draft.edits[store.slotKey]?.payload["text"]?.text,"保留参数\n第二行")
        await store.renameProfile(name:"Migrated")
        XCTAssertEqual(store.draft.revision,8);XCTAssertEqual(store.draft.loaded_format,2)
        let backup=try FileManager.default.contentsOfDirectory(at:dir,includingPropertiesForKeys:nil).first {$0.pathExtension=="backup"}
        XCTAssertNotNil(backup);XCTAssertEqual(try Data(contentsOf:backup!),bytes)
        let reopened=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:url);await reopened.loadWorkbench();reopened.slot=1
        XCTAssertEqual(reopened.profile?.name,"Migrated")
        XCTAssertEqual(reopened.draft.edits[reopened.slotKey]?.payload["text"]?.text,"保留参数\n第二行")
    }
    @MainActor func testProfileGraphEditingThroughRealBridgeSurvivesRestart() async throws {
        let directory=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:directory)}
        let url=directory.appendingPathComponent("workbench.json")
        let store=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:url)
        await store.loadWorkbench();let oldID=store.profile?.id.text
        await store.createProfile(name:"Local profile")
        let created=store.profile?.id.text
        XCTAssertNotEqual(created,oldID);XCTAssertEqual(store.profile?.name,"Local profile")
        await store.createLayer(name:"Work layer");let layer=store.layer?.id.text
        await store.assign("save");await store.renameProfile(name:"Renamed")
        await store.moveProfile(by:-1)
        XCTAssertEqual(store.profile?.id.text,created);XCTAssertEqual(store.layer?.id.text,layer)
        await store.duplicateProfile(name:"Independent copy");let copy=store.profile?.id.text
        XCTAssertNotEqual(copy,created);await store.undoDraft();XCTAssertEqual(store.snapshot?.profiles.count,3)
        await store.undoDraft(redo:true);XCTAssertEqual(store.snapshot?.profiles.last?.id.text,copy)
        let reopened=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:url);await reopened.loadWorkbench()
        XCTAssertEqual(reopened.snapshot?.profiles.count,4)
        XCTAssertTrue(reopened.snapshot?.profiles.contains {$0.id.text==created && $0.name=="Renamed"} == true)
        XCTAssertFalse(reopened.draft.can_undo)
    }
    @MainActor func testWorkbenchDraftThroughRealBridge() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("workbench.json")
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await store.loadWorkbench()
        let before = store.snapshot?.raw.text
        store.slot = 1
        await store.assign("save")
        XCTAssertEqual(store.draft.edits[store.slotKey]?.id, "save")
        store.selectProfile(1)
        XCTAssertEqual(store.draft.edits.count, 1)
        await store.undoDraft()
        XCTAssertTrue(store.draft.edits.isEmpty)
        await store.undoDraft(redo: true)
        XCTAssertEqual(store.draft.edits.count, 1)
        XCTAssertEqual(store.snapshot?.raw.text, before)
        XCTAssertNil(store.connectedID)
        let reopened = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: MicroInputCore.MicroInput(), documentURL: url)
        await reopened.loadWorkbench()
        reopened.slot = 1
        XCTAssertEqual(reopened.draft.edits[reopened.slotKey]?.id, "save")
        XCTAssertEqual(reopened.snapshot?.raw.text, before)
    }
}

// R-H11: closing the editor must not terminate the offline menu-bar host.
extension NavigationTests {
    @MainActor func testClosingLastWindowKeepsHostAlive() {
        XCTAssertFalse(AppLifetime().applicationShouldTerminateAfterLastWindowClosed(NSApplication.shared))
    }
}

#if MI_SYNTHETIC_BACKEND
// R-D01/R-D13: OS events cross real Rust → UniFFI → Store, with no physical device.
extension NavigationTests {
    @MainActor func testDiscoveryKeepsCandidatesSeparateAndRemovalClearsSelection() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        await store.startDiscovery()
        XCTAssertEqual(store.discoveryStatus, "无 USB 候选设备")
        try await core.setSyntheticDiscovery(state: .twoDevices)
        for _ in 0..<100 where store.devices.count != 2 { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertEqual(store.devices.count, 2)
        XCTAssertNil(store.selectedDeviceID)
        XCTAssertFalse(store.devices[1].verifiedModel)
        store.selectDevice(store.devices[0].id)
        XCTAssertEqual(store.selectedDeviceID, "synthetic-interface-0")
        XCTAssertTrue(store.discoveryStatus.contains("固件未知"))
        try await core.setSyntheticDiscovery(state: .noDevices)
        for _ in 0..<100 where store.selectedDeviceID != nil { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertNil(store.selectedDeviceID)
        XCTAssertEqual(store.discoveryStatus, "设备已断开 · 可继续使用离线工作台")
        try await core.setSyntheticDiscovery(state: .failure)
        for _ in 0..<100 where !store.discoveryStatus.contains("失败") { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertTrue(store.discoveryStatus.contains("发现失败"))
    }
}
#endif

#if MI_SYNTHETIC_BACKEND
extension NavigationTests {
    @MainActor func testExplicitSnapshotDraftPreservesPreviousDocumentAndRestoresSelection() async throws {
        let dir=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:dir)}
        let url=dir.appendingPathComponent("workbench.json")
        let core=MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state:.twoDevices);try await core.setSyntheticConfig(scenario:1)
        let store=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:core,documentURL:url);await store.loadWorkbench();await store.assign("save")
        let original=try Data(contentsOf:url)
        await store.refresh();store.selectDevice("synthetic-interface-0");await store.connect("synthetic-interface-0")
        await store.readIdentity();await store.readDeviceConfig()
        XCTAssertFalse(store.canEditDraft)
        let requestCount=await core.syntheticRequests().count
        await store.createDraftFromSnapshot()
        XCTAssertTrue(store.canEditDraft);XCTAssertFalse(store.browsingDeviceSnapshot)
        XCTAssertEqual(store.snapshot?.source,"LocalDraft")
        let chosen=store.draft.document_id
        store.slot=1 // Unknown original remains visible while a separate local intent is assigned.
        await store.assign("save");await store.renameProfile(name:"Device working copy")
        XCTAssertEqual(store.draft.edits[store.slotKey]?.id,"save")
        XCTAssertEqual(try Data(contentsOf:url),original)
        let finalRequests=await core.syntheticRequests().count;XCTAssertEqual(finalRequests,requestCount)
        await store.suspend()
        let reopened=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:MicroInputCore.MicroInput(),documentURL:url);await reopened.loadWorkbench()
        XCTAssertEqual(reopened.draft.document_id,chosen)
        XCTAssertEqual(reopened.profile?.name,"Device working copy")
        XCTAssertEqual(reopened.draft.edits.count,1);XCTAssertFalse(reopened.draft.can_undo)
        XCTAssertTrue(reopened.snapshot?.raw.text.contains("18446744073709551615") == true)
    }
    @MainActor func testIdentityThroughRealBridgeIsSeparateFromConnectionAndConfig() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        await store.loadDemo(); await store.refresh()
        store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0")
        XCTAssertNotNil(store.connectedID)
        XCTAssertNil(store.identity)
        var requests = await core.syntheticRequests()
        XCTAssertEqual(requests, [])
        await store.readIdentity()
        XCTAssertEqual(store.identity?.version, "synthetic-0.6.3")
        XCTAssertEqual(store.identity?.profileIndex, 2)
        XCTAssertTrue(store.connectionSummary.contains("synthetic-0.6.3"))
        XCTAssertTrue(store.status.contains("身份已读回"))
        XCTAssertEqual(store.snapshot?.source, "Demo")
        requests = await core.syntheticRequests()
        XCTAssertEqual(requests, ["device.status"])
        store.selectProfile(1); store.selectLayer(0)
        await store.readDeviceConfig()
        requests = await core.syntheticRequests()
        XCTAssertEqual(requests, ["device.status", "fs.list"])
        XCTAssertEqual(store.snapshot?.source, "Demo")
        XCTAssertNotNil(store.connectedID)
        await store.suspend()
        XCTAssertNil(store.identity)
    }
    @MainActor func testIdentityFallbackAndFailureThroughRealBridge() async throws {
        let core = MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state: .twoDevices)
        try await core.setSyntheticIdentity(scenario: 1)
        let store = Store(nativeOwnership: NativeOwnershipController(isolated: true), backend: core)
        await store.connect("synthetic-interface-0"); await store.readIdentity()
        XCTAssertEqual(store.identity?.sourceMethod, "sys.version")
        XCTAssertNil(store.identity?.profileIndex)
        XCTAssertNil(store.identity?.layerIndex)
        await store.suspend()
        try await core.setSyntheticIdentity(scenario: 2)
        await store.connect("synthetic-interface-0"); await store.readIdentity()
        XCTAssertNil(store.identity)
        XCTAssertNotNil(store.connectedID)
        XCTAssertTrue(store.identityStatus.contains("失败"))
        await store.suspend()
    }
}
#endif

#if MI_SYNTHETIC_BACKEND
extension NavigationTests {
    @MainActor func testDeviceSnapshotThroughRealBridgeNeverEditsLocalDraft() async throws {
        let directory=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer {try? FileManager.default.removeItem(at:directory)}
        let url=directory.appendingPathComponent("workbench.json")
        let core=MicroInputCore.MicroInput.syntheticDiscovery()
        try await core.setSyntheticDiscovery(state:.twoDevices)
        try await core.setSyntheticConfig(scenario:1)
        let store=Store(nativeOwnership: NativeOwnershipController(isolated: true), backend:core,documentURL:url)
        await store.loadWorkbench()
        await store.assign("save")
        let localFile=try Data(contentsOf:url)
        let revision=store.draft.revision
        await store.refresh(); store.selectDevice("synthetic-interface-0")
        await store.connect("synthetic-interface-0"); await store.readIdentity(); await store.readDeviceConfig()
        XCTAssertTrue(store.browsingDeviceSnapshot)
        XCTAssertFalse(store.canEditDraft)
        XCTAssertEqual(store.snapshot?.source,"Imported")
        XCTAssertEqual(store.snapshot?.firmware,"synthetic-0.6.3")
        XCTAssertEqual(store.snapshot?.device_files?.count,2)
        XCTAssertEqual(store.layer?.displayColor,"#123456")
        XCTAssertEqual(store.snapshot?.smartRecords.prefix(3).map { $0.0 }, ["SA_2", "SA_19", "SA_9007199254740993"])
        XCTAssertTrue(store.snapshot?.raw.text.contains("18446744073709551615") == true)
        XCTAssertEqual(store.profile?.id.text,"7")
        let count=await core.syntheticRequests().count
        store.selectProfile(1);store.selectLayer(0);await store.assign("copy");await store.undoDraft()
        let afterNavigation=await core.syntheticRequests().count
        XCTAssertEqual(afterNavigation,count)
        XCTAssertEqual(store.draft.revision,revision)
        XCTAssertEqual(try Data(contentsOf:url),localFile)
        try await core.setSyntheticConfig(scenario:3)
        await store.readDeviceConfig()
        XCTAssertTrue(store.stale)
        XCTAssertEqual(store.snapshot?.profiles.count,2)
        XCTAssertTrue(store.browsingDeviceSnapshot)
        store.showLocalDraft()
        XCTAssertTrue(store.canEditDraft)
        XCTAssertFalse(store.browsingDeviceSnapshot)
        XCTAssertEqual(store.snapshot?.source,"Demo")
        XCTAssertEqual(store.draft.edits.count,1)
        await store.undoDraft()
        XCTAssertTrue(store.draft.edits.isEmpty)
        await store.suspend()
    }
}
#endif
