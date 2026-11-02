// R-A02/R-N01: navigation has no transport dependency or device side effect.
import Foundation
import MicroInputCore
protocol Backend {
    func loadDemo() async throws -> String
    func listDevices() async throws -> [DeviceInfo]
    func connect(id: String) async throws
    func readConfig() async throws -> String
    func disconnect() async
    func inputMonitoringStatus() async -> Bool
    func subscribeDeviceEvents(cb: DeviceEvents) async
}
extension Backend {
    func subscribeDeviceEvents(cb: DeviceEvents) async {}
}
extension MicroInput: Backend {}
@MainActor final class Store: ObservableObject {
    private let backend: Backend
    @Published var draft = DraftState()
    @Published var catalog: [CatalogAction] = []
    @Published var slot = 0
    private var draftCore: MicroInput? { backend as? MicroInput }
    @Published var documentReady = false
    private var documentOverride: URL?
    var documentURL: URL {
        if let documentOverride { return documentOverride }
        if let directory = ProcessInfo.processInfo.environment["MI_DOCUMENT_DIR"] {
            return URL(fileURLWithPath: directory).appendingPathComponent("workbench.json")
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("MicroInput/workbench.json")
    }
    func loadWorkbench() async {
        guard let core = draftCore else { await loadDemo(); return }
        do {
            draft = try JSONDecoder().decode(DraftState.self, from: Data(try await core.openDocument(path: documentURL.path).utf8))
            await loadDemo()
            catalog = try JSONDecoder().decode([CatalogAction].self, from: Data(try await core.actionCatalog().utf8))
            documentReady = !draft.read_only
            status = draft.read_only ? "文档含未知数据 · 只读 · 原文件保留" : "本地草稿 · 修改自动保存 · 不写设备"
        } catch { status = readableError(error); documentReady = false }
    }
    var slotKey: String { key(slot) }
    func key(_ slot: Int) -> String {
        guard let profile, let layer else { return "" }
        return bindingKey(profile: profile.id, layer: layer.id, slot: slot)
    }
    func assign(_ id: String) async {
        guard let action = catalog.first(where: {$0.id == id}) else { return }
        await saveAction(action)
    }
    func saveAction(_ action: CatalogAction) async {
        guard documentReady, !busy, let core = draftCore, let profile, let layer else { return }
        busy = true; defer { busy = false }
        do {
            let encoder = JSONEncoder()
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            let a = String(decoding: try encoder.encode(action), as: UTF8.self)
            draft = try JSONDecoder().decode(DraftState.self, from: Data(try await core.assignAction(profileId: p, layerId: l, slot: UInt32(slot), actionJson: a).utf8))
            status = "草稿已保存到本机 · 不执行动作 · 不写设备"
        } catch { status = readableError(error) }
    }
    func undoDraft(redo: Bool = false) async {
        guard documentReady, !busy, let core = draftCore else { return }; busy = true; defer { busy = false }
        do { draft = try JSONDecoder().decode(DraftState.self, from: Data(try await core.undoDraft(redo: redo).utf8)) }
        catch { status = readableError(error) }
    }
    private var generation = 0
    private var pendingID: String?
    @Published var snapshot: Snapshot?
    @Published var devices: [DeviceInfo] = []
    @Published var selectedDeviceID: String?
    @Published var discoveryStatus = "USB 发现尚未开始"
    private var events: Events?
    var selectedDevice: DeviceInfo? { devices.first { $0.id == selectedDeviceID } }
    func startDiscovery() async {
        guard events == nil else { return }
        let receiver = Events(self)
        events = receiver
        await backend.subscribeDeviceEvents(cb: receiver)
        await refresh()
    }
    func selectDevice(_ id: String) {
        guard let device = devices.first(where: { $0.id == id }) else { return }
        selectedDeviceID = id
        discoveryStatus = device.verifiedModel ? "已连接 USB · 固件未知 · 尚未打开设备" : "未知型号 · 尚未验证 · 固件未知"
    }
    func discoveryFailed(_ message: String) {
        devices = []; selectedDeviceID = nil
        discoveryStatus = "发现失败 · \(message)"
    }
    @Published var profileIndex = 0
    @Published var layerIndex = 0
    @Published var selectedBinding: JSONValue?
    @Published var allowed = false
    @Published var status = "未连接设备"
    @Published var connectedID: String?
    @Published var stale = false
    @Published var busy = false
    init(backend: Backend, documentURL: URL? = nil) { self.backend = backend; self.documentOverride = documentURL }
    var profile: Profile? { guard let ps = snapshot?.profiles, ps.indices.contains(profileIndex) else { return nil }; return ps[profileIndex] }
    var layer: Layer? { guard let ls = profile?.layers, ls.indices.contains(layerIndex) else { return nil }; return ls[layerIndex] }
    func selectProfile(_ index: Int) { profileIndex = index; layerIndex = 0; slot = 0; selectedBinding = nil }
    func selectLayer(_ index: Int) { layerIndex = index; slot = 0; selectedBinding = nil }
    func refresh() async {
        // R-D02: descriptors only, no permission preflight/request or device open.
        do { received(try await backend.listDevices()) } catch { discoveryFailed(readableError(error)) }
    }
    func received(_ values: [DeviceInfo]) {
        let removed = selectedDeviceID.map { id in !values.contains { $0.id == id } } ?? false
        devices = values
        if removed {
            selectedDeviceID = nil
            discoveryStatus = "设备已断开 · 可继续使用离线工作台"
        } else if selectedDeviceID == nil {
            discoveryStatus = values.isEmpty ? "无 USB 候选设备" : "发现 \(values.count) 个 USB 候选 · 请选择设备"
        }
        if let id = pendingID ?? connectedID, !values.contains(where: { $0.id == id }) { generation += 1; pendingID = nil; connectedID = nil; stale = snapshot != nil; status = "设备已断开" }
    }
    func loadDemo() async {
        guard !busy else { return }; busy = true; defer { busy = false }
        generation += 1; let token = generation
        await backend.disconnect(); guard token == generation else { return }; connectedID = nil
        do { let json = try await backend.loadDemo(); guard token == generation else { return }; try replace(json); status = "演示数据 · 未与设备同步" } catch { if token == generation { status = readableError(error) } }
    }
    func connect(_ id: String) async {
        guard !busy else { return }; busy = true; defer { busy = false }
        generation += 1; let token = generation
        connectedID = nil; pendingID = id
        // Preserve provenance: a failed device read cannot turn demo data into a device snapshot.
        do {
            try await backend.connect(id: id)
            guard token == generation else { return }
            pendingID = nil; connectedID = id; status = "已连接 · 协议未验证"
            let json = try await backend.readConfig()
            guard token == generation else { return }; try replace(json)
        } catch { if token == generation { pendingID = nil; stale = snapshot != nil; status = "读取失败 · \(readableError(error))" } }
    }
    func suspend() async { generation += 1; pendingID = nil; await backend.disconnect(); connectedID = nil; stale = snapshot != nil; status = "会话已断开" }
    private func replace(_ json: String) throws { let result = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8)); snapshot = result; stale = false; selectProfile(0) }
}
@MainActor final class Events: DeviceEvents {
    private weak var store: Store?
    init(_ store: Store) { self.store = store }
    nonisolated func changed(devices: [DeviceInfo]) { Task { @MainActor [weak self] in self?.store?.received(devices) } }
    nonisolated func failed(message: String) { Task { @MainActor [weak self] in self?.store?.discoveryFailed(message) } }
}

// R-A02: keep bridge type names and protocol internals out of normal status text.
func readableError(_ error: Error) -> String {
    if case BridgeError.Failed(let message) = error {
        if message.contains("协议未验证") { return "协议未验证，设备读取待确认" }
        return message
    }
    return error.localizedDescription
}
