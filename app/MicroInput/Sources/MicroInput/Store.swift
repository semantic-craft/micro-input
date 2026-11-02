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
    func selectProfile(_ index: Int) { profileIndex = index; layerIndex = 0; selectedBinding = nil }
    func selectLayer(_ index: Int) { layerIndex = index; selectedBinding = nil }
    func refresh() async {
        allowed = await backend.inputMonitoringStatus()
        do { received(try await backend.listDevices()) } catch { status = error.localizedDescription }
    }
    func received(_ values: [DeviceInfo]) {
        devices = values
        if let id = pendingID ?? connectedID, !values.contains(where: { $0.id == id }) { generation += 1; pendingID = nil; connectedID = nil; stale = snapshot != nil; status = "设备已断开" }
    }
    func loadDemo() async {
        guard !busy else { return }; busy = true; defer { busy = false }
        generation += 1; let token = generation
        await backend.disconnect(); guard token == generation else { return }; connectedID = nil
        do { let json = try await backend.loadDemo(); guard token == generation else { return }; try replace(json); status = "演示数据 · 未与设备同步" } catch { if token == generation { status = error.localizedDescription } }
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
        } catch { if token == generation { pendingID = nil; stale = snapshot != nil; status = "读取失败 · \(error.localizedDescription)" } }
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
