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
}
extension MicroInput: Backend {}
@MainActor final class Store: ObservableObject {
    private let backend: Backend
    @Published var draft = DraftState()
    @Published var catalog: [CatalogAction] = []
    @Published var slot = 0
    private var draftCore: MicroInput? { backend as? MicroInput }
    func loadWorkbench() async {
        await loadDemo()
        guard let core = draftCore else { return }
        do {
            catalog = try JSONDecoder().decode([CatalogAction].self, from: Data(try await core.actionCatalog().utf8))
            draft = try JSONDecoder().decode(DraftState.self, from: Data(try await core.draftState().utf8))
        } catch { status = readableError(error) }
    }
    var slotKey: String { "\(profileIndex):\(layerIndex):\(slot)" }
    func assign(_ action: String) async {
        guard !busy, let core = draftCore else { return }; busy = true; defer { busy = false }
        do { draft = try JSONDecoder().decode(DraftState.self, from: Data(try await core.assignAction(profile: UInt32(profileIndex), layer: UInt32(layerIndex), slot: UInt32(slot), action: action).utf8)) }
        catch { status = readableError(error) }
    }
    func undoDraft(redo: Bool = false) async {
        guard !busy, let core = draftCore else { return }; busy = true; defer { busy = false }
        do { draft = try JSONDecoder().decode(DraftState.self, from: Data(try await core.undoDraft(redo: redo).utf8)) }
        catch { status = readableError(error) }
    }
    private var generation = 0
    private var pendingID: String?
    @Published var snapshot: Snapshot?
    @Published var devices: [DeviceInfo] = []
    @Published var profileIndex = 0
    @Published var layerIndex = 0
    @Published var selectedBinding: JSONValue?
    @Published var allowed = false
    @Published var status = "未连接设备"
    @Published var connectedID: String?
    @Published var stale = false
    @Published var busy = false
    init(backend: Backend) { self.backend = backend }
    var profile: Profile? { guard let ps = snapshot?.profiles, ps.indices.contains(profileIndex) else { return nil }; return ps[profileIndex] }
    var layer: Layer? { guard let ls = profile?.layers, ls.indices.contains(layerIndex) else { return nil }; return ls[layerIndex] }
    func selectProfile(_ index: Int) { profileIndex = index; layerIndex = 0; slot = 0; selectedBinding = nil }
    func selectLayer(_ index: Int) { layerIndex = index; slot = 0; selectedBinding = nil }
    func refresh() async {
        allowed = await backend.inputMonitoringStatus()
        do { received(try await backend.listDevices()) } catch { status = readableError(error) }
    }
    func received(_ values: [DeviceInfo]) {
        devices = values
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
    nonisolated func failed(message: String) { Task { @MainActor [weak self] in self?.store?.status = message } }
}

// R-A02: keep bridge type names and protocol internals out of normal status text.
func readableError(_ error: Error) -> String {
    if case BridgeError.Failed(let message) = error {
        if message.contains("协议未验证") { return "协议未验证，设备读取待确认" }
        return message
    }
    return error.localizedDescription
}
