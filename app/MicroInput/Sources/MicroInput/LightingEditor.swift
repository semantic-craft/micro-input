import SwiftUI
import MicroInputCore
struct PreviewStatus: Decodable { let phase: String }
struct LightingField: Decodable { var value: String; let editable: Bool }
struct LightingModel: Decodable { var zones: [String: [String: LightingField]]; let raw: String }
extension Store {
    func lightingModel() -> LightingModel? {
        guard let id = layer?.id.text else { return nil }
        return try? JSONDecoder().decode(LightingModel.self, from: Data(backend.layerLighting(layerId: id).utf8))
    }
    func editLighting(zone: String, field: String, value: String, allLayers: Bool) async {
        guard canEditDraft, !busy, !deviceOperationRunning, let pid = profile?.id.text, let lid = layer?.id.text else { return }
        busy = true
        defer { busy = false }
        do {
            if let token = lightingPreviewToken { try backend.updateLightingPreview(token: token, zone: zone, field: field, value: value); lightingPreviewResult = "合成预览中 · 未改配置文件" }
            else { try applyDocument(try await backend.editLighting(profileId: pid, layerId: lid, zone: zone, field: field, value: value, allLayers: allLayers)) }
        } catch { status = readableError(error) }
    }
    func beginLightingPreview() async {
        guard lightingPreviewToken == nil, !deviceOperationRunning else { return }
        do { lightingPreviewToken = try await backend.beginLightingPreview(); lightingPreviewPhase = "Active"; lightingPreviewResult = "临时预览设备活动层 · 不保存草稿 · 30 秒后自动结束" }
        catch { lightingPreviewResult = readableError(error) }
    }
    func endLightingPreview() async {
        guard let token = lightingPreviewToken, !endingLightingPreview else { return }
        endingLightingPreview = true
        defer { endingLightingPreview = false }
        do {
            let raw = try await backend.endLightingPreview(token: token)
            let state = try JSONDecoder().decode(PreviewStatus.self, from: Data(raw.utf8))
            guard lightingPreviewToken == token else { return }
            lightingPreviewPhase = state.phase
            lightingPreviewResult = state.phase == "Restored" ? "合成灯光已恢复 · 实体灯效尚未验收" : "预览恢复未确认 · 设备操作保持关闭，不自动重放"
            lightingPreviewToken = nil
        } catch { lightingPreviewResult = readableError(error) }
    }
    func observeLightingPreview() async {
        guard let token = lightingPreviewToken else { return }
        while !Task.isCancelled, lightingPreviewToken == token {
            if let state = try? JSONDecoder().decode(PreviewStatus.self, from: Data(backend.lightingPreviewStatus().utf8)), state.phase != "Active", state.phase != "Starting" {
                await endLightingPreview(); return
            }
            do { try await Task.sleep(nanoseconds: 200_000_000) } catch { return }
        }
    }
    #if MI_SYNTHETIC_BACKEND
    func runSyntheticLightingScenario() async {
        guard let core = backend as? MicroInputCore.MicroInput else { return }
        do {
            try await core.setSyntheticDiscovery(state: .twoDevices)
            await refresh(); selectDevice("synthetic-interface-0"); await connect("synthetic-interface-0")
            await readIdentity(); await readDeviceConfig(); await createDraftFromSnapshot()
            await editLighting(zone: "backlight", field: "color", value: "#3377CC", allLayers: false)
            await previewApply(); await prepareApplyBackup()
            guard let token = applyPreview?.token else { throw BridgeError.Failed(message: status) }
            await performDeviceOperation(token: token)
            guard operationResult?.phase == "Verified", let id = operationResult?.operation_id else { throw BridgeError.Failed(message: status) }
            await prepareDeviceRestore(id)
            guard let restore = restorePreview else { throw BridgeError.Failed(message: status) }
            await performDeviceOperation(token: restore.token, restore: true)
            await beginLightingPreview()
            guard lightingPreviewToken != nil else { throw BridgeError.Failed(message: lightingPreviewResult) }
            await editLighting(zone: "backlight", field: "color", value: "#CC5500", allLayers: false)
            try await Task.sleep(nanoseconds: 200_000_000)
            await endLightingPreview()
            guard lightingPreviewPhase == "Restored" else { throw BridgeError.Failed(message: lightingPreviewResult) }
            lightingEvidence(success: true)
            showingLighting = true
        } catch { status = readableError(error); lightingEvidence(success: false) }
    }
    private func lightingEvidence(success: Bool) {
        guard let path = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"] else { return }
        let url = URL(fileURLWithPath: path).deletingLastPathComponent().appendingPathComponent("lighting.json")
        let payload: [String: Any] = ["status": success ? "PASS" : "FAIL", "runtime_readback": success ? "SYNTHETIC_PASS" : "NOT_VERIFIED", "physical_key": "NOT_RUN", "lighting_visual": "NOT_RUN"]
        if let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys]) { try? data.write(to: url, options: .atomic) }
    }
    #endif
}
struct LightingEditor: View {
    @ObservedObject var store: Store
    @State private var model: LightingModel?
    @State private var zone = "backlight"
    @State private var allLayers = false
    @State private var color = ""
    private let effects = ["off", "solid", "snake", "rainbow", "breath", "gradient"]
    func reload() { model = store.lightingModel(); color = model?.zones[zone]?["color"]?.value ?? "" }
    func save(_ field: String, _ value: String) { Task { await store.editLighting(zone: zone, field: field, value: value, allLayers: allLayers); reload() } }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack { Text("RGB 灯光").font(.title2); Spacer(); Button("结束预览并关闭") { Task { await store.endLightingPreview(); store.showingLighting = false } } }
            Text("本地编辑保存到当前层。即时预览只对合成设备开放，实体灯效尚未验收。")
            Picker("灯区", selection: $zone) { Text("Backlight · 按键背光").tag("backlight"); Text("Underglow · 底部灯光").tag("underglow") }.pickerStyle(.segmented).onChange(of: zone) { _,_ in reload() }
            if let fields = model?.zones[zone] {
                Picker("灯效", selection: Binding(get: { model?.zones[zone]?["effect"]?.value ?? "unknown" }, set: { model?.zones[zone]?["effect"]?.value = $0; save("effect", $0) })) {
                    ForEach(effects, id: \.self) { Text($0.capitalized).tag($0) }
                    if !effects.contains(fields["effect"]?.value ?? "") { Text("未知原值（保留）").tag(fields["effect"]?.value ?? "unknown") }
                }.disabled(fields["effect"]?.editable != true || store.busy)
                HStack { Text("RGB"); TextField("#RRGGBB", text: $color).textFieldStyle(.roundedBorder); Button(store.lightingPreviewToken == nil ? "保存颜色" : "预览颜色") { save("color", color) } }.disabled(fields["color"]?.editable != true || store.busy)
                ForEach(["brightness", "speed", "magic"], id: \.self) { field in
                    HStack {
                        Text(field == "brightness" ? "亮度" : field == "speed" ? "速度" : "Magic").frame(width: 60, alignment: .leading)
                        Slider(value: Binding(get: { Double(model?.zones[zone]?[field]?.value ?? "") ?? 0 }, set: { model?.zones[zone]?[field]?.value = String(Int($0.rounded())) }), in: 0...100, step: 1, onEditingChanged: { editing in if !editing { save(field, model?.zones[zone]?[field]?.value ?? "") } }).accessibilityLabel(field)
                        Text(fields[field]?.value ?? "未知").frame(width: 40)
                    }.disabled(fields[field]?.editable != true || store.busy)
                }
                Toggle("应用到当前配置的所有层", isOn: $allLayers).disabled(store.lightingPreviewToken != nil)
                if allLayers { Text("受影响层：\(store.profile?.layers.map(\.name).joined(separator: "、") ?? "")").font(.caption) }
                Text("禁用参数保留原值。0–100 为编辑器刻度，Magic 含义未作额外推断。").font(.caption)
            } else { Text("灯光来源缺失或未知；保留原值，不创建默认配置。") }
            Divider()
            HStack {
                Button("审阅配置应用…") { Task { await store.endLightingPreview(); store.showingLighting = false; await store.previewApply() } }.disabled(store.lightingPreviewToken != nil || store.deviceOperationRunning)
                Button("开始即时预览（合成）") { Task { allLayers = false; await store.beginLightingPreview(); reload() } }.disabled(!store.operationCapabilities.preview || store.lightingPreviewToken != nil)
                Button("结束并恢复") { Task { await store.endLightingPreview(); reload() } }.disabled(store.lightingPreviewToken == nil)
            }
            Text(store.lightingPreviewResult).font(.caption).textSelection(.enabled)
            Text(store.status).font(.caption)
            DisclosureGroup("原始灯光值") { Text(model?.raw ?? "未知").font(.caption.monospaced()).textSelection(.enabled) }
        }.padding(24).frame(width: 680).onAppear { store.refreshOperationCapabilities(); reload() }.task(id: store.lightingPreviewToken) { await store.observeLightingPreview() }
    }
}
