// R-A02/R-N01: navigation has no transport dependency or device side effect.
import Foundation
import MicroInputCore
protocol Backend {
    func loadDemo() async throws -> String
    func listDevices() async throws -> [DeviceInfo]
    func connect(id: String) async throws
    func readIdentity() async throws -> IdentityInfo
    func readConfig() async throws -> String
    func disconnect() async
    func waitForDisconnect() async -> String
    func shutdown() async
    func inputMonitoringStatus() async -> Bool
    func subscribeDeviceEvents(cb: DeviceEvents) async
    func subscribeInput() async throws -> InputSubscription
    func openDocument(path: String) async throws -> String
    func actionCatalog() async throws -> String
    func actionLibrary(query: String, category: String, favoritesOnly: Bool) async throws -> String
    func setActionFavorite(actionId: String, favorite: Bool) async throws
    func assignAction(profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String
    func setControlSettings(profileId: String, layerId: String, settingsJson: String) async throws -> String
    func saveMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String
    func bindMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32) async throws -> String
    func deleteMacroAction(macroId: String) async throws -> String
    func undoDraft(redo: Bool) async throws -> String
    func documentView() async throws -> String
    func createProfile(name: String) async throws -> String
    func renameProfile(profileId: String, name: String) async throws -> String
    func duplicateProfile(profileId: String, name: String) async throws -> String
    func deleteProfile(profileId: String) async throws -> String
    func moveProfile(profileId: String, toIndex: UInt32) async throws -> String
    func createLayer(profileId: String, name: String) async throws -> String
    func renameLayer(layerId: String, name: String) async throws -> String
    func duplicateLayer(profileId: String, layerId: String, name: String) async throws -> String
    func deleteLayer(profileId: String, layerId: String) async throws -> String
    func moveLayer(profileId: String, layerId: String, toIndex: UInt32) async throws -> String
    func createDraftFromSnapshot() async throws -> String
}
extension Backend {
    func waitForDisconnect() async -> String { "" }
    func shutdown() async { await disconnect() }
    func subscribeInput() async throws -> InputSubscription { throw BridgeError.Failed(message: "输入监听不可用") }
    func readIdentity() async throws -> IdentityInfo { throw BridgeError.Failed(message: "身份读取不可用") }
    func openDocument(path: String) async throws -> String { throw BridgeError.Failed(message: "本地文档不可用") }
    func actionCatalog() async throws -> String { throw BridgeError.Failed(message: "动作目录不可用") }
    func actionLibrary(query: String, category: String, favoritesOnly: Bool) async throws -> String { throw BridgeError.Failed(message: "动作库不可用") }
    func setActionFavorite(actionId: String, favorite: Bool) async throws { throw BridgeError.Failed(message: "收藏不可用") }
    func assignAction(profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String { throw BridgeError.Failed(message: "草稿编辑不可用") }
    func setControlSettings(profileId: String, layerId: String, settingsJson: String) async throws -> String { throw BridgeError.Failed(message: "控件设置不可用") }
    func saveMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32, actionJson: String) async throws -> String { throw BridgeError.Failed(message: "本地宏不可用") }
    func bindMacroAction(macroId: String?, profileId: String, layerId: String, slot: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地宏不可用") }
    func deleteMacroAction(macroId: String) async throws -> String { throw BridgeError.Failed(message: "本地宏不可用") }
    func undoDraft(redo: Bool) async throws -> String { throw BridgeError.Failed(message: "草稿撤销不可用") }

    func documentView() async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func createProfile(name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func renameProfile(profileId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func duplicateProfile(profileId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func deleteProfile(profileId: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func moveProfile(profileId: String, toIndex: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func createLayer(profileId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func renameLayer(layerId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func duplicateLayer(profileId: String, layerId: String, name: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func deleteLayer(profileId: String, layerId: String) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func moveLayer(profileId: String, layerId: String, toIndex: UInt32) async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func createDraftFromSnapshot() async throws -> String { throw BridgeError.Failed(message: "本地文档功能不可用") }
    func subscribeDeviceEvents(cb: DeviceEvents) async {}
}
extension MicroInput: Backend {}
@MainActor final class Store: ObservableObject {
    func subscribeInput() async throws -> InputSubscription { try await backend.subscribeInput() }
    private let backend: Backend
    @Published var draft = DraftState()
    @Published var macros: [MacroSummary] = []
    @Published var catalog: [CatalogAction] = []
    @Published var library = ActionLibraryState()
    @Published var actionCapabilities: [String:ActionCapability] = [:]
    private var libraryGeneration = 0
    private var libraryQuery = "", libraryCategory = "全部"
    private var libraryFavoritesOnly = false
    func searchActions(query: String, category: String, favoritesOnly: Bool) async {
        libraryGeneration += 1; let generation = libraryGeneration
        libraryQuery = query; libraryCategory = category; libraryFavoritesOnly = favoritesOnly
        do {
            let result = try await backend.actionLibrary(query: query, category: category, favoritesOnly: favoritesOnly)
            guard generation == libraryGeneration else { return }
            library = try JSONDecoder().decode(ActionLibraryState.self, from: Data(result.utf8))
        } catch { if generation == libraryGeneration { status = readableError(error) } }
    }
    func setFavorite(_ id: String, favorite: Bool) async {
        do {
            try await backend.setActionFavorite(actionId: id, favorite: favorite)
            await searchActions(query: libraryQuery, category: libraryCategory, favoritesOnly: libraryFavoritesOnly)
            status = "收藏已保存为本机偏好 · 草稿修订不变"
        } catch { status = readableError(error) }
    }
    @Published var controlSettings: [String:LocalControlSettings] = [:]
    @Published var controlSettingsError: String?
    var currentControlSettings: LocalControlSettings? { layer.flatMap { controlSettings[$0.id.text] } }
    @Published var slot = 0
    @Published var documentReady = false
    @Published private(set) var browsingDeviceSnapshot = false
    private var localSnapshot: Snapshot?
    private var localSelection: (profile: String?, layer: String?, slot: Int) = (nil, nil, 0)
    var canEditDraft: Bool { documentReady && !browsingDeviceSnapshot }
    func showLocalDraft() {
        guard !busy, let localSnapshot else { return }
        snapshot = localSnapshot; browsingDeviceSnapshot = false; stale = false
        restoreSelection(profile: localSelection.profile, layer: localSelection.layer, fallbackProfile: 0, fallbackLayer: 0); slot = localSelection.slot
        status = "本地草稿 · 修改自动保存 · 不写设备"
    }
    private var documentOverride: URL?
    var documentURL: URL {
        if let documentOverride { return documentOverride }
        if let directory = ProcessInfo.processInfo.environment["MI_DOCUMENT_DIR"] {
            return URL(fileURLWithPath: directory).appendingPathComponent("workbench.json")
        }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("MicroInput/workbench.json")
    }
    func loadWorkbench() async {
        do {
            try applyDocument(try await backend.openDocument(path: documentURL.path))
            catalog = try JSONDecoder().decode([CatalogAction].self, from: Data(try await backend.actionCatalog().utf8))
            await searchActions(query: "", category: "全部", favoritesOnly: false)
        } catch { status = readableError(error); documentReady = false }
    }
    private func restoreSelection(profile id: String?, layer lid: String?, fallbackProfile: Int, fallbackLayer: Int) {
        profileIndex = snapshot?.profiles.firstIndex(where: {$0.id.text == id}) ?? min(fallbackProfile, max(0, (snapshot?.profiles.count ?? 0)-1))
        layerIndex = profile?.layers.firstIndex(where: {$0.id.text == lid}) ?? min(fallbackLayer, max(0, (profile?.layers.count ?? 0)-1))
        slot = min(slot, max(0, (layer.map {$0.keys.count+$0.encoders.count+$0.joystick.sectors.count} ?? 0)-1))
    }
    private func applyDocument(_ json: String, fresh: Bool = false) throws {
        let result = try JSONDecoder().decode(DocumentView.self, from: Data(json.utf8))
        let oldProfile = fresh ? nil : profile?.id.text, oldLayer = fresh ? nil : layer?.id.text
        let pi = fresh ? 0 : profileIndex, li = fresh ? 0 : layerIndex
        macros = result.macro_library ?? []
        draft = result.draft; snapshot = result.config; localSnapshot = result.config
        actionCapabilities = result.action_capabilities ?? [:]
        controlSettings = result.control_settings ?? [:]; controlSettingsError = nil
        browsingDeviceSnapshot = false; stale = false; documentReady = !draft.read_only
        restoreSelection(profile: result.selection_hint?.profile_id ?? oldProfile, layer: result.selection_hint?.layer_id ?? oldLayer, fallbackProfile: pi, fallbackLayer: li)
        localSelection = (profile?.id.text, layer?.id.text, slot)
        status = draft.read_only ? "文档含未知数据 · 只读 · 原文件保留" : "本地草稿 · 修改自动保存 · 不写设备"
        if draft.loaded_format == 1 { status += " · 旧格式将在首次修改时转换并备份" }
    }
    private func editDocument(_ action: () async throws -> String) async {
        guard canEditDraft, !busy else { return }; busy = true; defer { busy = false }
        do { try applyDocument(try await action()) } catch { status = readableError(error) }
    }
    func createProfile(name: String) async { await editDocument { try await backend.createProfile(name: name) } }
    func renameProfile(name: String) async { guard let id=profile?.id.text else {return}; await editDocument { try await backend.renameProfile(profileId:id,name:name) } }
    func duplicateProfile(name: String) async { guard let id=profile?.id.text else {return}; await editDocument {try await backend.duplicateProfile(profileId:id,name:name)} }
    func deleteProfile() async {guard let id=profile?.id.text else {return}; await editDocument {try await backend.deleteProfile(profileId:id)} }
    func moveProfile(by offset: Int) async {guard let id=profile?.id.text, let count=snapshot?.profiles.count, (0..<count).contains(profileIndex+offset) else {return}; await editDocument {try await backend.moveProfile(profileId:id,toIndex:UInt32(profileIndex+offset))} }
    func createLayer(name: String) async {guard let id=profile?.id.text else {return}; await editDocument {try await backend.createLayer(profileId:id,name:name)} }
    func renameLayer(name: String) async {guard let id=layer?.id.text else {return}; await editDocument {try await backend.renameLayer(layerId:id,name:name)} }
    func duplicateLayer(name: String) async {guard let pid=profile?.id.text, let id=layer?.id.text else {return}; await editDocument {try await backend.duplicateLayer(profileId:pid,layerId:id,name:name)} }
    func deleteLayer() async {guard let pid=profile?.id.text, let id=layer?.id.text else {return}; await editDocument {try await backend.deleteLayer(profileId:pid,layerId:id)} }
    func moveLayer(by offset: Int) async {guard let pid=profile?.id.text, let id=layer?.id.text, let count=profile?.layers.count, (0..<count).contains(layerIndex+offset) else {return}; await editDocument {try await backend.moveLayer(profileId:pid,layerId:id,toIndex:UInt32(layerIndex+offset))} }
    func createDraftFromSnapshot() async {
        guard browsingDeviceSnapshot, !stale, !busy else {return};busy=true;defer {busy=false}
        do {try applyDocument(try await backend.createDraftFromSnapshot(), fresh:true);status="已创建独立本地草稿 · 原工作台保留 · 重开继续此草稿"} catch {status=readableError(error)}
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
    @discardableResult func saveAction(_ action: CatalogAction) async -> Bool {
        if action.payload["type"]?.text == "local_macro" { return await saveMacro(action) }
        guard canEditDraft, !busy, let profile, let layer else { return false }
        busy = true; defer { busy = false }
        do {
            let encoder = JSONEncoder()
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            let a = String(decoding: try encoder.encode(action), as: UTF8.self)
            try applyDocument(try await backend.assignAction(profileId: p, layerId: l, slot: UInt32(slot), actionJson: a))
            status = "草稿已保存到本机 · 不执行动作 · 不写设备"
            return true
        } catch { status = readableError(error); return false }
    }
    @discardableResult func saveControlSettings(mode: String, sectors: String) async -> Bool {
        guard canEditDraft, !busy, let profile, let layer else { return false }
        guard let count = UInt32(sectors), count <= 32 else {
            controlSettingsError = "活动扇区数：请输入 0–32 的整数（本机编辑器限制，非设备容量）"
            return false
        }
        busy = true; defer { busy = false }
        do {
            let data = try JSONEncoder().encode(LocalControlSettings(joystick_mode: mode, active_sectors: count))
            try applyDocument(try await backend.setControlSettings(profileId: profile.id.text, layerId: layer.id.text, settingsJson: String(decoding: data, as: UTF8.self)))
            status = "控件意图已保存到本机 · 旧槽位及绑定保留 · 不写设备"
            return true
        } catch { controlSettingsError = readableError(error); return false }
    }
    var selectedMacro: MacroSummary? {
        macros.first { $0.references.contains { $0.profile_id == profile?.id.text && $0.layer_id == layer?.id.text && $0.logical_control == "logical-\(slot)" } }
    }
    @discardableResult func saveMacro(_ action: CatalogAction, id: String? = nil) async -> Bool {
        guard canEditDraft, !busy, let profile, let layer else { return false }
        busy = true; defer { busy = false }
        do {
            let encoder = JSONEncoder()
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            let a = String(decoding: try encoder.encode(action), as: UTF8.self)
            try applyDocument(try await backend.saveMacroAction(macroId: id, profileId: p, layerId: l, slot: UInt32(slot), actionJson: a))
            return true
        } catch { status = readableError(error); return false }
    }
    func bindMacro(_ id: String?) async {
        guard let profile, let layer else { return }
        let encoder = JSONEncoder()
        do {
            let p = String(decoding: try encoder.encode(profile.id), as: UTF8.self)
            let l = String(decoding: try encoder.encode(layer.id), as: UTF8.self)
            await editDocument { try await backend.bindMacroAction(macroId: id, profileId: p, layerId: l, slot: UInt32(slot)) }
        } catch { status = readableError(error) }
    }
    func deleteMacro(_ id: String) async { await editDocument { try await backend.deleteMacroAction(macroId: id) } }
    func undoDraft(redo: Bool = false) async {
        guard canEditDraft, !busy else { return }; busy = true; defer { busy = false }
        do { try applyDocument(try await backend.undoDraft(redo: redo)) }
        catch { status = readableError(error) }
    }
    private var generation = 0
    private var pendingID: String?
    private var connectionWatcher: Task<Void, Never>?
    var canDisconnect: Bool { pendingID != nil || connectedID != nil }
    @Published var identity: IdentityInfo?
    @Published var identityStatus = "身份尚未读取"
    @Published var snapshot: Snapshot?
    @Published var devices: [DeviceInfo] = []
    @Published var selectedDeviceID: String?
    @Published var discoveryStatus = "USB 发现尚未开始"
    private var events: Events?
    var selectedDevice: DeviceInfo? { devices.first { $0.id == selectedDeviceID } }
    var connectionSummary: String {
        if connectedID != nil, connectedID == selectedDeviceID {
            return identity.map { "USB 会话已连接 · 固件 \($0.version ?? "未知")" } ?? "USB 会话已连接 · 身份尚未读取"
        }
        return discoveryStatus
    }
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
        if let id = pendingID ?? connectedID, !values.contains(where: { $0.id == id }) { retireConnection("设备已断开") }
    }
    func loadDemo() async {
        guard !busy else { return }; busy = true; defer { busy = false }
        generation += 1; let token = generation
        await backend.disconnect(); guard token == generation else { return }; connectedID = nil; identity = nil; identityStatus = "身份尚未读取"
        do { let json = try await backend.loadDemo(); guard token == generation else { return }; try replace(json); localSnapshot = snapshot; browsingDeviceSnapshot = false; status = "本地草稿 · 演示基线 · 未与设备同步" } catch { if token == generation { status = readableError(error) } }
    }
    func connect(_ id: String) async {
        guard !busy else { return }
        generation += 1; let token = generation
        busy = true; defer { if token == generation { busy = false } }
        connectionWatcher?.cancel()
        stale = snapshot != nil
        connectedID = nil; identity = nil; identityStatus = "身份尚未读取"; pendingID = id
        // Preserve provenance: a failed device read cannot turn demo data into a device snapshot.
        do {
            try await backend.connect(id: id)
            guard token == generation else { return }
            pendingID = nil; connectedID = id; status = "设备已打开 · 身份尚未读取 · 配置尚未读取"
            connectionWatcher = Task { [weak self, backend] in
                let reason = await backend.waitForDisconnect()
                guard !Task.isCancelled, !reason.isEmpty, let self, token == self.generation else { return }
                self.retireConnection(reason)
            }
        } catch { if token == generation { pendingID = nil; stale = snapshot != nil; status = "连接失败 · \(readableError(error))" } }
    }
    func readIdentity() async {
        guard !busy, connectedID != nil else { return }
        let token = generation
        busy = true; defer { if token == generation { busy = false } }
        identity = nil; identityStatus = "正在读取身份"
        do {
            let result = try await backend.readIdentity()
            guard token == generation else { return }
            identity = result
            identityStatus = "身份已读回 · \(result.sourceMethod)"
            status = "设备身份已读回 · 固件 \(result.version ?? "未知") · 配置尚未读取"
        } catch { if token == generation { identityStatus = "身份读取失败 · \(readableError(error))" } }
    }
    func readDeviceConfig() async {
        guard !busy, connectedID != nil else { return }
        let token = generation
        busy = true; defer { if token == generation { busy = false } }
        stale = snapshot != nil
        status = "正在读取设备配置 · 旧数据尚未更新"
        do {
            let json = try await backend.readConfig()
            guard token == generation else { return }
            let result = try JSONDecoder().decode(Snapshot.self, from: Data(json.utf8))
            if !browsingDeviceSnapshot {
                localSnapshot = snapshot
                localSelection = (profile?.id.text, layer?.id.text, slot)
            }
            snapshot = result; browsingDeviceSnapshot = true; stale = false; selectProfile(0)
            status = result.source == "DeviceSnapshot" ? "设备快照 · 只读 · 已核对文件大小及摘要" : "合成设备夹具 · 只读 · 不是实际设备快照"
        } catch { if token == generation { stale = snapshot != nil; status = "配置读取失败 · \(readableError(error))" } }
    }

    private func retireConnection(_ reason: String) {
        generation += 1; pendingID = nil; busy = false
        connectionWatcher?.cancel(); connectionWatcher = nil
        connectedID = nil; identity = nil; identityStatus = "身份尚未读取"
        stale = snapshot != nil; status = reason
    }
    func suspend() async { retireConnection("会话已断开"); await backend.disconnect() }
    func shutdown() async { retireConnection("应用正在退出"); await backend.shutdown() }
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
    if case BridgeError.Disconnected = error { return "设备已断开；请重新连接" }
    if case BridgeError.Timeout = error { return "请求超时；可以手动重试" }
    if case BridgeError.Unavailable = error { return "设备 I/O 失败，原因未确认；请检查连接后重试" }
    if case BridgeError.Failed(let message) = error {
        if message.contains("协议未验证") { return "协议未验证，设备读取待确认" }
        return message
    }
    return error.localizedDescription
}
