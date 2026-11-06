import SwiftUI
import MicroInputCore
struct PreviewStatus: Decodable { let phase: String }
struct LightingField: Decodable { var value: String; let editable: Bool }
struct LightingModel: Decodable { var zones: [String: [String: LightingField]]; let raw: String; let can_add: Bool }
extension Store {
    func lightingModel() -> LightingModel? {
        guard let id = layer?.id.text else { return nil }
        return try? JSONDecoder().decode(LightingModel.self, from: Data(backend.layerLighting(layerId: id).utf8))
    }
    func editLighting(zone: String, field: String, value: String, allLayers: Bool) async {
        guard !isProtectedLayerSelected else { status = "受保护宿主层不支持添加或修改静态 RGB 灯光"; return }
        guard canEditDraft, !busy, !deviceOperationRunning, let pid = profile?.id.text, let lid = layer?.id.text else { return }
        busy = true
        defer { busy = false }
        do {
            if let token = lightingPreviewToken {
                guard nativeOwnership.allowsExecution else { await endLightingPreview(); return }
                try backend.updateLightingPreview(token: token, zone: zone, field: field, value: value); lightingPreviewResult = "即时预览中 · 未改配置文件" }
            else { try applyDocument(try await backend.editLighting(profileId: pid, layerId: lid, zone: zone, field: field, value: value, allLayers: allLayers)) }
        } catch { status = readableError(error) }
    }
    func beginLightingPreview() async {
        guard !isProtectedLayerSelected else { lightingPreviewResult = "受保护宿主层不支持静态灯光预览"; return }
        guard nativeOwnership.allowsExecution, lightingPreviewToken == nil, !startingLightingPreview, !deviceOperationRunning, let lid = layer?.id.text else { return }
        startingLightingPreview = true
        lightingCleanupConfirmed = false
        lightingPreviewGeneration += 1
        let generation = lightingPreviewGeneration
        defer { startingLightingPreview = false }
        do {
            let token = try await backend.beginLightingPreview(layerId: lid)
            lightingPreviewToken = token
            guard generation == lightingPreviewGeneration, nativeOwnership.allowsExecution else { await endLightingPreview(); return }
            lightingPreviewPhase = "Active"
            lightingPreviewResult = "在当前连接设备显示所选层灯光 · 30 秒后停止发送 · 未保存草稿"
        } catch { if generation == lightingPreviewGeneration { lightingPreviewResult = readableError(error) } }
    }
    func endLightingPreview() async {
        lightingPreviewGeneration += 1
        guard let token = lightingPreviewToken, !endingLightingPreview else { return }
        endingLightingPreview = true
        defer { endingLightingPreview = false }
        do {
            let raw = try await backend.endLightingPreview(token: token)
            let state = try JSONDecoder().decode(PreviewStatus.self, from: Data(raw.utf8))
            guard lightingPreviewToken == token else { return }
            lightingPreviewPhase = state.phase
            lightingCleanupConfirmed = ["Stopped", "Restored"].contains(state.phase)
            switch state.phase {
            case "Restored": lightingPreviewResult = "合成灯光已恢复 · 实体灯效尚未验收"
            case "Stopped": lightingPreviewResult = "已停止预览发送 · 不自动恢复设备灯光，配置文件未改变"
            default: lightingPreviewResult = "预览结果未确认 · 已停止发送，不自动重放"
            }
            lightingPreviewToken = nil
        } catch {
            lightingPreviewResult = readableError(error)
            if lightingPreviewToken == token,
               let state = try? JSONDecoder().decode(PreviewStatus.self, from: Data(backend.lightingPreviewStatus().utf8)), state.phase == "Idle" {
                lightingPreviewToken = nil
                lightingPreviewPhase = "Unknown"
                lightingPreviewResult = "旧连接预览已退役；设备灯光状态未确认，不自动重放"
            }
        }
    }
    func addLayerLighting() {
        guard !isProtectedLayerSelected else { status = "受保护宿主层不支持添加或修改静态 RGB 灯光"; return }
        guard let lid = layer?.id.text else { return }
        do { try applyDocument(backend.addLayerLighting(layerId: lid)) }
        catch { status = readableError(error) }
    }
    func saveLightingPreview() async {
        guard let token = lightingPreviewToken else { return }
        do {
            try applyDocument(backend.saveLightingPreview(token: token))
            await endLightingPreview()
            if lightingPreviewPhase == "Stopped" || lightingPreviewPhase == "Restored" {
                lightingPreviewResult = "预览灯光已保存到草稿；可审阅配置应用"
            } else {
                lightingPreviewResult = "灯光已保存到草稿；预览停止结果未确认：\(lightingPreviewResult)"
            }
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
    private func sliderBinding(for field: String) -> Binding<Double> {
        Binding(
            get: { Double(model?.zones[zone]?[field]?.value ?? "") ?? 0 },
            set: { newValue in
                let formatted = String(Int(newValue.rounded()))
                model?.zones[zone]?[field]?.value = formatted
                if store.lightingPreviewToken != nil {
                    save(field, formatted)
                }
            }
        )
    }

    private func sliderValueLabel(for field: String, fields: [String: LightingField]) -> String {
        guard let raw = fields[field]?.value else { return "未知" }
        return field == "magic" ? raw : "\(raw)%"
    }

    @ViewBuilder
    private func sliderRow(for field: String, fields: [String: LightingField]) -> some View {
        let label = field == "brightness" ? "亮度" : field == "speed" ? "速度" : "参数值"
        let isEditable = fields[field]?.editable == true
        HStack {
            Text(label)
                .font(UITheme.Typography.body)
                .frame(width: 64, alignment: .leading)
            Slider(
                value: sliderBinding(for: field),
                in: 0...100,
                step: 1,
                onEditingChanged: { editing in
                    if !editing && store.lightingPreviewToken == nil {
                        save(field, model?.zones[zone]?[field]?.value ?? "")
                    }
                }
            )
            .accessibilityLabel(field)
            Text(sliderValueLabel(for: field, fields: fields))
                .font(UITheme.Typography.mono)
                .frame(width: 48, alignment: .trailing)
        }
        .disabled(!isEditable || store.busy)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("键盘背光与氛围灯").font(UITheme.Typography.title)
                Spacer()
                Button("完成") { Task { await store.endLightingPreview(); store.showingLighting = false } }
            }
            Picker("灯区", selection: $zone) {
                Text("按键背光").tag("backlight")
                Text("底部氛围灯").tag("underglow")
            }
            .pickerStyle(.segmented)
            .onChange(of: zone) { _, _ in reload() }

            if model?.can_add == true {
                Text("当前图层尚未启用灯光配置。")
                    .font(UITheme.Typography.body)
                    .foregroundStyle(UITheme.textSecondary)
                Button("启用图层灯光") { store.addLayerLighting(); reload() }
                    .disabled(store.lightingPreviewToken != nil || store.busy)
            } else if let fields = model?.zones[zone] {
                Picker("灯效模式", selection: Binding(get: { model?.zones[zone]?["effect"]?.value ?? "unknown" }, set: { model?.zones[zone]?["effect"]?.value = $0; save("effect", $0) })) {
                    ForEach(effects, id: \.self) { Text($0.capitalized).tag($0) }
                    if !effects.contains(fields["effect"]?.value ?? "") { Text("保留原值").tag(fields["effect"]?.value ?? "unknown") }
                }
                .font(UITheme.Typography.body)
                .disabled(fields["effect"]?.editable != true || store.busy)

                HStack(spacing: 10) {
                    Text("色值（#RRGGBB）").font(UITheme.Typography.body)
                    TextField("#RRGGBB", text: $color)
                        .textFieldStyle(.roundedBorder)
                        .font(UITheme.Typography.mono)
                        .frame(width: 120)
                    Button(store.lightingPreviewToken == nil ? "应用颜色" : "预览颜色") { save("color", color) }
                        .font(UITheme.Typography.body)
                }
                .disabled(fields["color"]?.editable != true || store.busy)

                ForEach(["brightness", "speed", "magic"], id: \.self) { field in
                    sliderRow(for: field, fields: fields)
                }
                Toggle("同时应用到当前方案的所有图层", isOn: $allLayers)
                    .font(UITheme.Typography.body)
                    .disabled(store.lightingPreviewToken != nil)
                if allLayers {
                    Text("将同步更新：\(store.profile?.layers.map(\.name).joined(separator: "、") ?? "")")
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }
            } else {
                Text("当前图层保留原始硬件灯光数据。")
                    .font(UITheme.Typography.body)
                    .foregroundStyle(UITheme.textSecondary)
            }
            Divider()
            HStack {
                Button("同步到键盘…") { Task { await store.endLightingPreview(); store.showingLighting = false; await store.previewApply() } }
                    .disabled(store.lightingPreviewToken != nil || store.deviceOperationRunning)
                Button("在键盘上实时预览（30 秒）") { Task { allLayers = false; await store.beginLightingPreview(); reload() } }
                    .disabled(!store.operationCapabilities.preview || store.lightingPreviewToken != nil || store.startingLightingPreview || model?.can_add == true)
                Button("保留预览结果") { Task { await store.saveLightingPreview(); reload() } }
                    .disabled(store.lightingPreviewToken == nil || store.busy)
                Button("停止预览") { Task { await store.endLightingPreview(); reload() } }
                    .disabled(store.lightingPreviewToken == nil)
            }
            .font(UITheme.Typography.body)
            if !store.lightingPreviewResult.isEmpty {
                Text(store.lightingPreviewResult)
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(UITheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(24)
        .frame(width: 620)
        .background(UITheme.surface)
        .tint(UITheme.accent)
        .onAppear { store.refreshOperationCapabilities(); reload() }
        .task(id: store.lightingPreviewToken) { await store.observeLightingPreview() }
    }
}
