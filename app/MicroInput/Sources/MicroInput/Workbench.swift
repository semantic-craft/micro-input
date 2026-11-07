// R-P03/R-E01/R-E02/R-K01: selected workbench; all edits flow through UniFFI.
import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct Workbench: View {
    @ObservedObject var store: Store
    @State private var choosing = false
    @State private var reviewing = false
    @State private var choosingMacro = false
    @State private var choosingMulti = false
    @State private var showingDiagnosticExport = false
    @State private var showingSettings = false
    @State private var showDiagnostics = false
    @State private var showOtherProfiles = false
    @State private var naming = false
    @State private var nameOperation = ""
    @State private var objectName = ""
    @State private var deleting = false
    @State private var deleteLayer = false
    @State private var inspectorSearch = ""
    @State private var inspectorCategory = "全部"
    @State private var inspectorFavoritesOnly = false
    @AppStorage("appAppearanceMode") private var appearance = "system"
    @ObservedObject private var visualPrefs = KeycapVisualPreferences.shared

    var changeCount: Int { store.draft.edits.count + store.draft.structure_changes.count }

    func promptName(_ operation: String) {
        nameOperation = operation
        objectName = operation.contains("配置") ? (store.profile?.name ?? "新配置") : (store.layer?.name ?? "新层")
        if operation.hasPrefix("新建") { objectName = operation.contains("配置") ? "新配置" : "新层" }
        if operation.hasPrefix("复制") { objectName += " 副本" }
        naming = true
    }

    var bindings: [JSONValue] {
        guard let l = store.layer else { return [] }
        return l.keys + l.encoders + l.joystick.sectors
    }

    func title(_ slot: Int) -> String {
        switch slot {
        case 0: return "左上键（按键 01）"
        case 1: return "右上键（按键 02）"
        case 2..<10: return String(format: "按键 %02d", slot + 1)
        case 10: return store.isBottomRowSplit ? "底行左键（开关 11）" : "底部宽键（2U，开关 11+12）"
        case 11: return store.isBottomRowSplit ? "底行中键（开关 12）" : "底部宽键右开关（与开关 11 同键）"
        case 12: return "底行右键（开关 13）"
        case 13: return "旋钮左旋"
        case 14: return "旋钮右旋"
        case 15: return "旋钮按下"
        default:
            let sectorIndex = slot - 15
            let directions = [1: "上", 2: "左", 3: "下", 4: "右"]
            if let dir = directions[sectorIndex] {
                return "四向摇杆（\(dir)）"
            }
            return "摇杆扇区 \(sectorIndex)"
        }
    }

    func shortPositionLabel(_ slot: Int) -> String {
        switch slot {
        case 0: return "左上键"
        case 1: return "右上键"
        case 2..<10: return String(format: "按键 %02d", slot + 1)
        case 10: return store.isBottomRowSplit ? "按键 11" : "底部宽键"
        case 11: return store.isBottomRowSplit ? "按键 12" : "宽键右轴"
        case 12: return "按键 13"
        case 13: return "旋钮左旋"
        case 14: return "旋钮右旋"
        case 15: return "旋钮按下"
        default:
            let sectorIndex = slot - 15
            let directions = [1: "摇杆上", 2: "摇杆左", 3: "摇杆下", 4: "摇杆右"]
            return directions[sectorIndex] ?? "扇区 \(sectorIndex)"
        }
    }

    func action(_ slot: Int) -> CatalogAction? {
        store.browsingDeviceSnapshot ? nil : store.draft.edits[store.key(slot)]
    }

    func label(_ slot: Int) -> String {
        let raw: String
        if let a = action(slot) {
            raw = a.name
        } else if bindings.indices.contains(slot) {
            raw = bindings[slot].bindingLabel
        } else {
            raw = "未分配"
        }
        return raw.replacingOccurrences(of: " · ", with: "，")
    }

    private var isHeaderStatusReady: Bool {
        !store.editingDevice
            && !store.browsingDeviceSnapshot
            && store.connectedID != nil
            && store.hostExecutionEnabled
            && store.accessibilityTrusted
            && store.nativeOwnership.allowsExecution
    }

    private var headerStatusText: String {
        if store.editingDevice {
            return "键盘硬件配置编辑模式"
        }
        if store.browsingDeviceSnapshot {
            return "正在查看键盘快照（只读）"
        }
        if store.connectedID == nil {
            return "键盘未连接"
        }
        if !store.hostExecutionEnabled {
            return "按键控制已暂停"
        }
        if !store.accessibilityTrusted {
            return "需要开启辅助功能权限"
        }
        if !store.nativeOwnership.allowsExecution {
            return store.nativeOwnership.summary.replacingOccurrences(of: " · ", with: "，")
        }
        if store.autoSwitchState.isEnabled {
            return "Creator Micro 2 已就绪，\(store.routingMode)"
        }
        return "Creator Micro 2 已就绪"
    }

    var body: some View {
        VStack(spacing: 0) {
            topStatusBar
            Rectangle().fill(UITheme.border).frame(height: 1)

            HStack(spacing: 0) {
                leftSidebar
                Rectangle().fill(UITheme.border).frame(width: 1)
                if !store.editingDevice && !store.browsingDeviceSnapshot {
                    agentFocusedStage
                } else {
                    centerStage
                    if !store.isProtectedLayerSelected {
                        Rectangle().fill(UITheme.border).frame(width: 1)
                        rightInspector
                    }
                }
            }

            if showDiagnostics {
                Rectangle().fill(UITheme.border).frame(height: 1)
                ScrollView {
                    VStack(alignment: .leading, spacing: 8) {
                        discoveryPanel
                        InputMonitorPanel(store: store)
                    }
                }
                .frame(maxHeight: 190)
                .background(UITheme.subtleSurface)
            }

            Rectangle().fill(UITheme.border).frame(height: 1)
            bottomFooterBar
        }
        .frame(minWidth: 1020, minHeight: 620)
        .background(UITheme.surface)
        .foregroundStyle(UITheme.textPrimary)
        .background {
            Button("前一个控件") { store.slot = max(0, store.slot - 1) }.keyboardShortcut("[", modifiers: [.command]).hidden()
            Button("后一个控件") { store.slot = min(max(0, bindings.count - 1), store.slot + 1) }.keyboardShortcut("]", modifiers: [.command]).hidden()
            Button("分配动作") { if store.canEditDraft && store.layer != nil && !store.isProtectedLayerSelected { choosing = true } }.keyboardShortcut("e", modifiers: [.command]).hidden()
            Button("查看修改") { reviewing = true }.keyboardShortcut("d", modifiers: [.command]).hidden()
            Button("撤销草稿") { Task { await store.undoDraft() } }.keyboardShortcut("z", modifiers: [.command]).hidden()
            Button("重做草稿") { Task { await store.undoDraft(redo: true) } }.keyboardShortcut("z", modifiers: [.command, .shift]).hidden()
        }
        .onChange(of: store.isProtectedLayerSelected) { _, isProtected in
            if isProtected {
                choosing = false
                choosingMacro = false
                choosingMulti = false
            }
        }
        .sheet(isPresented: $choosing) { actionSheet }
        .sheet(isPresented: $showingSettings) {
            MicroInputSettingsView(
                store: store,
                appearance: $appearance,
                showDiagnostics: $showDiagnostics,
                onOpenDiagnosticExport: {
                    showingSettings = false
                    showingDiagnosticExport = true
                },
                onOpenDiffReview: {
                    showingSettings = false
                    reviewing = true
                },
                close: { showingSettings = false }
            )
        }
        .sheet(isPresented: $choosingMulti) { MultiLibraryView(store: store) }
        .sheet(isPresented: $choosingMacro) { MacroLibraryView(store: store) }
        .sheet(isPresented: $reviewing) { diffSheet }
        .sheet(isPresented: $naming) { nameSheet }
        .sheet(isPresented: $showingDiagnosticExport) {
            DiagnosticsExportView(store: store, close: { showingDiagnosticExport = false })
        }
        .alert(deleteLayer ? "删除此图层？" : "删除此配置方案？", isPresented: $deleting) {
            Button("取消", role: .cancel) {}
            Button("删除") {
                Task {
                    if deleteLayer { await store.deleteLayer() } else { await store.deleteProfile() }
                }
            }
        } message: {
            Text("删除后可通过 ⌘Z 撤销，不会修改键盘硬件内原有数据。")
        }
        .sheet(isPresented: $store.showingLighting, onDismiss: { Task { await store.endLightingPreview() } }) { LightingEditor(store: store) }
        .sheet(isPresented: $store.showingDeviceOperations) { DeviceOperationsView(store: store) }
        .sheet(item: $store.applyPreview, onDismiss: { Task { await store.cancelApplyPreview() } }) { preview in ApplyPreparationView(store: store, preview: preview) }
        .sheet(item: $store.comparison) { comparison in ReconnectComparisonView(store: store, initialComparison: comparison) }
        .tint(UITheme.accent)
        .accentColor(UITheme.accent)
        .task {
            if store.snapshot == nil { await store.loadWorkbench() }
            if ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == "diagnostic-export" {
                showingDiagnosticExport = true
            }
            if (ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] ?? "").isEmpty {
                await store.prepareDesktopProfiles()
                await store.ensureDefaultAgentLinks()
            }
            await store.searchActions(query: inspectorSearch, category: inspectorCategory, favoritesOnly: inspectorFavoritesOnly)
        }
        .task(id: "\(inspectorSearch)\u{0}\(inspectorCategory)\u{0}\(inspectorFavoritesOnly)") {
            await store.searchActions(query: inspectorSearch, category: inspectorCategory, favoritesOnly: inspectorFavoritesOnly)
        }
    }

    private var topStatusBar: some View {
        let ready = isHeaderStatusReady
        return HStack(spacing: 14) {
            HStack(spacing: 8) {
                Circle()
                    .fill(ready ? UITheme.accent : UITheme.caution)
                    .frame(width: 7, height: 7)
                Text(headerStatusText)
                    .font(UITheme.Typography.captionMedium)
                    .foregroundStyle(ready ? UITheme.accent : UITheme.caution)
                    .lineLimit(1)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(
                (ready ? UITheme.accentSoft : UITheme.cautionSoft),
                in: RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
            )

            if store.connectedID == nil {
                Button("连接键盘") {
                    Task { await store.autoSetupHostExecutionIfNeeded() }
                }
                .controlSize(.small)
                .disabled(store.busy)
            }

            if store.editingDevice {
                Button("返回日常软件方案") {
                    Task { await store.returnToHostDocument() }
                }
                .controlSize(.small)
                .disabled(store.busy || store.deviceOperationRunning)
            }

            Spacer()

            Toggle("按键控制", isOn: Binding(get: { store.hostExecutionEnabled }, set: { value in Task { await store.setHostExecutionEnabled(value) } }))
                .toggleStyle(.switch)
                .controlSize(.small)
                .font(UITheme.Typography.body)
                .fixedSize()

            HStack(spacing: 4) {
                Button { Task { await store.undoDraft() } } label: {
                    Image(systemName: "arrow.uturn.backward")
                }
                .help("撤销（⌘Z）")
                .accessibilityLabel("撤销")
                .disabled(!store.canEditDraft || !store.draft.can_undo || store.busy)

                Button { Task { await store.undoDraft(redo: true) } } label: {
                    Image(systemName: "arrow.uturn.forward")
                }
                .help("重做（⇧⌘Z）")
                .accessibilityLabel("重做")
                .disabled(!store.canEditDraft || !store.draft.can_redo || store.busy)
            }

            Button("设置…") {
                showingSettings = true
            }
            .font(UITheme.Typography.body)
            .controlSize(.small)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 10)
        .background(UITheme.subtleSurface)
    }

    // MARK: - Left Sidebar (Scheme A)
    private var leftSidebar: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(store.editingDevice ? "键盘硬件配置" : store.browsingDeviceSnapshot ? "键盘只读快照" : "常用软件方案")
                .font(UITheme.Typography.captionMedium)
                .foregroundStyle(UITheme.textSecondary)
                .padding(.horizontal, 6)

            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    if let snapshot = store.snapshot {
                        ForEach(visibleProfileIndices, id: \.self) { i in
                            let isSelected = i == store.profileIndex
                            let isActiveHost = !store.browsingDeviceSnapshot && !store.editingDevice && snapshot.profiles[i].id.text == store.hostActiveProfileId
                            Button {
                                store.selectProfile(i)
                                if !store.browsingDeviceSnapshot, !store.editingDevice, DesktopAgent.named(snapshot.profiles[i].name) != nil {
                                    Task { await store.setHostActiveProfile(snapshot.profiles[i].id.text) }
                                }
                            } label: {
                                HStack(spacing: 10) {
                                    AgentBadge(name: snapshot.profiles[i].name, size: 22)
                                    Text(DesktopAgent.displayName(snapshot.profiles[i].name))
                                        .font(isSelected ? UITheme.Typography.bodySemibold : UITheme.Typography.body)
                                        .foregroundStyle(UITheme.textPrimary)
                                        .lineLimit(2)
                                        .fixedSize(horizontal: false, vertical: true)
                                    Spacer(minLength: 4)
                                    if isActiveHost {
                                        Text("使用中")
                                            .font(UITheme.Typography.captionMedium)
                                            .foregroundStyle(UITheme.accent)
                                            .padding(.horizontal, 6)
                                            .padding(.vertical, 2)
                                            .background(UITheme.accentSoft, in: RoundedRectangle(cornerRadius: 4))
                                            .fixedSize()
                                    }
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 8)
                                .background(
                                    isSelected ? UITheme.surface : Color.clear,
                                    in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                                )
                                .overlay(
                                    RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                                        .strokeBorder(isSelected ? UITheme.accentBorder : Color.clear, lineWidth: 1)
                                )
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(DesktopAgent.displayName(snapshot.profiles[i].name))
                        }
                    }
                }
            }

            Rectangle().fill(UITheme.border).frame(height: 1)

            VStack(alignment: .leading, spacing: 8) {
                Toggle(AutoSwitchState.toggleTitle, isOn: Binding(
                    get: { store.autoSwitchState.isEnabled },
                    set: { value in
                        Task {
                            await store.setAutoFollow(value)
                        }
                    }
                ))
                .toggleStyle(.checkbox)
                .font(UITheme.Typography.caption)

                Toggle("显示其他自定义方案", isOn: $showOtherProfiles)
                    .toggleStyle(.checkbox)
                    .font(UITheme.Typography.caption)

                HStack(spacing: 8) {
                    Menu("方案管理") {
                        Button("新建配置") { promptName("新建配置") }
                        Button("重命名配置") { promptName("重命名配置") }.disabled(store.profile == nil)
                        Button("复制配置") { promptName("复制配置") }.disabled(store.profile == nil)
                        Divider()
                        Button("上移配置") { Task { await store.moveProfile(by: -1) } }.disabled(store.profileIndex == 0 || store.profile == nil)
                        Button("下移配置") { Task { await store.moveProfile(by: 1) } }.disabled(store.profileIndex + 1 >= (store.snapshot?.profiles.count ?? 0))
                        Button("删除配置…", role: .destructive) { deleteLayer = false; deleting = true }.disabled(store.profile == nil)
                        Divider()
                        Button("补齐内置软件方案") { Task { await store.installCodingAgentPresets() } }.disabled(store.editingDevice)
                    }
                    .font(UITheme.Typography.captionMedium)
                    .fixedSize()
                    .disabled(!store.canEditDraft || store.busy || store.editingDevice)

                    Menu("导入与备份") {
                        PresetMenu(store: store).disabled(store.editingDevice)
                        DocumentRecoveryMenu(store: store)
                        AgentProposalButton(store: store).disabled(store.editingDevice)
                    }
                    .font(UITheme.Typography.captionMedium)
                    .fixedSize()
                    .disabled(!store.canEditDraft || store.busy)
                }
            }
        }
        .padding(12)
        .frame(width: 244)
        .background(UITheme.subtleSurface)
    }

    private var agentFocusedStage: some View {
        let agentKind = AgentProfileKind.from(profileName: store.profile?.name)

        return Group {
            if store.layer == nil {
                VStack(spacing: 12) {
                    Text(store.profile == nil ? "暂无配置方案" : "当前方案暂无图层")
                        .font(UITheme.Typography.heading)
                    Button(store.profile == nil ? "新建配置" : "新建层") {
                        promptName(store.profile == nil ? "新建配置" : "新建层")
                    }
                    .disabled(!store.canEditDraft || store.busy)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(UITheme.pageBackground)
            } else {
                CreatorMicroStageView(
                    store: store,
                    agentKind: agentKind,
                    actionForSlot: { action($0) },
                    labelForSlot: { label($0) },
                    titleForSlot: { title($0) },
                    onOpenFullActionLibrary: {
                        choosing = true
                    },
                    onOpenMacroLibrary: {
                        choosingMacro = true
                    }
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Center Stage (1:1 Physical Keyboard Canvas)
    private var centerStage: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                // Header
                HStack(alignment: .top, spacing: 16) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(DesktopAgent.displayName(store.profile?.name ?? (store.snapshot == nil ? "正在载入" : "暂无方案")))
                            .font(UITheme.Typography.title)
                        if let agent = DesktopAgent.named(store.profile?.name ?? ""), !store.browsingDeviceSnapshot, !store.editingDevice {
                            Text(agent.installed ? "应用已安装，支持 \(agent.capabilities)" : "未在应用程序目录找到此软件")
                                .font(UITheme.Typography.caption)
                                .foregroundStyle(UITheme.textSecondary)
                        } else if store.browsingDeviceSnapshot {
                            Text("当前为只读快照，点选按键可查看原始绑定")
                                .font(UITheme.Typography.caption)
                                .foregroundStyle(UITheme.textSecondary)
                        }
                    }

                    Spacer()

                    if let profile = store.profile {
                        HStack(spacing: 8) {
                            if profile.layers.count > 1 {
                                Picker("图层", selection: Binding(get: { store.layerIndex }, set: { store.selectLayer($0) })) {
                                    ForEach(profile.layers.indices, id: \.self) { i in
                                        Text(profile.layers[i].name).tag(i)
                                    }
                                }
                                .pickerStyle(.segmented)
                                .labelsHidden()
                                .frame(maxWidth: 240)
                            }

                            Menu("图层") {
                                Button("新建层") { promptName("新建层") }
                                Button("添加托管层") { Task { await store.createProtectedOaiLayer() } }
                                Button("重命名层") { promptName("重命名层") }.disabled(store.layer == nil)
                                Button("复制层") { promptName("复制层") }.disabled(store.layer == nil || store.isProtectedLayerSelected)
                                Divider()
                                Button("上移层") { Task { await store.moveLayer(by: -1) } }.disabled(store.layerIndex == 0 || store.layer == nil)
                                Button("下移层") { Task { await store.moveLayer(by: 1) } }.disabled(store.layerIndex + 1 >= (store.profile?.layers.count ?? 0))
                                Button("删除层…", role: .destructive) { deleteLayer = true; deleting = true }.disabled(store.layer == nil)
                            }
                            .font(UITheme.Typography.body)
                            .fixedSize()
                            .disabled(!store.canEditDraft || store.busy || store.editingDevice)
                        }
                    }
                }

                // Agent action bar (only when needed or when switching active target)
                if let agent = DesktopAgent.named(store.profile?.name ?? ""), !store.browsingDeviceSnapshot, !store.editingDevice {
                    HStack(spacing: 10) {
                        if !store.accessibilityTrusted {
                            Text("辅助功能未就绪，按键与按住说话暂不可用")
                                .font(UITheme.Typography.captionMedium)
                                .foregroundStyle(UITheme.caution)
                            Button("开启权限…") { store.requestAccessibilityAuthorization() }
                                .font(UITheme.Typography.captionMedium)
                            Button("重置签名登记…") { store.resetAndRequestAccessibilityAuthorization() }
                                .font(UITheme.Typography.caption)
                                .help("若系统设置中已打开开关仍提示未授权，点击重置旧签名登记并重新授权")
                        } else {
                            Text(store.hostActiveProfileId == store.profile?.id.text ? "当前正控制此软件" : "点击右侧按钮将此方案设为当前生效")
                                .font(UITheme.Typography.caption)
                                .foregroundStyle(UITheme.textSecondary)
                        }
                        Spacer()
                        if !agent.isGeneralFallback {
                            Button("打开 \(DesktopAgent.displayName(agent.name))") {
                                Task { await store.openDesktopAgent(agent) }
                            }
                            .font(UITheme.Typography.captionMedium)
                            .disabled(!agent.installed)
                        }

                        if store.hostActiveProfileId != store.profile?.id.text {
                            Button("启用此方案") {
                                Task { await store.setHostActiveProfile(store.profile?.id.text) }
                            }
                            .font(UITheme.Typography.captionMedium)
                            .disabled(store.busy)
                        }
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                    .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.itemInner).strokeBorder(UITheme.border, lineWidth: 1))
                }

                if store.layer == nil {
                    VStack(spacing: 12) {
                        Text(store.profile == nil ? "暂无配置方案" : "当前方案暂无图层")
                            .font(UITheme.Typography.heading)
                        Button(store.profile == nil ? "新建配置" : "新建层") {
                            promptName(store.profile == nil ? "新建配置" : "新建层")
                        }
                        .disabled(!store.canEditDraft || store.busy)
                    }
                    .frame(maxWidth: .infinity, minHeight: 280)
                } else {
                    // 1:1 Physical Keyboard Chassis (Outer radius 20pt, padding 12pt -> Inner keycap radius 8pt)
                    VStack(spacing: 8) {
                        // Top hardware row: Joystick + Key 01 + Key 02 + Encoder
                        HStack(spacing: 8) {
                            joystickHardwareCluster
                            physicalKeycap(slot: 0)
                            physicalKeycap(slot: 1)
                            encoderHardwareCluster
                        }

                        // Row 2: Keys 03..06 (slots 2..5)
                        HStack(spacing: 8) {
                            ForEach(2..<6, id: \.self) { slot in
                                physicalKeycap(slot: slot)
                            }
                        }

                        // Row 3: Keys 07..10 (slots 6..9)
                        HStack(spacing: 8) {
                            ForEach(6..<10, id: \.self) { slot in
                                physicalKeycap(slot: slot)
                            }
                        }

                        // Bottom Row: Split (slot 10 + slot 11) or Merged 2U Wide Key + Bottom-Right Key (slot 12)
                        HStack(spacing: 8) {
                            if store.isBottomRowSplit {
                                physicalKeycap(slot: 10)
                                physicalKeycap(slot: 11)
                            } else {
                                widePhysicalKeycap
                            }
                            physicalKeycap(slot: 12)
                        }
                    }
                    .padding(UITheme.Radius.chassisPadding)
                    .frame(maxWidth: 500)
                    .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter))
                    .overlay(
                        RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter)
                            .strokeBorder(UITheme.strongBorder, lineWidth: 1)
                    )
                    .overlay {
                        if store.isProtectedLayerSelected {
                            ZStack {
                                RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter)
                                    .fill(UITheme.surface.opacity(0.92))
                                VStack(spacing: 8) {
                                    Image(systemName: "lock.fill")
                                        .font(UITheme.Typography.title)
                                        .foregroundStyle(UITheme.textSecondary)
                                    Text("当前为客户端原生托管层\n按键与任务灯光由对应软件直接驱动")
                                        .font(UITheme.Typography.bodyMedium)
                                        .multilineTextAlignment(.center)
                                    Text("请在对应客户端内调整此层设置")
                                        .font(UITheme.Typography.caption)
                                        .foregroundStyle(UITheme.textSecondary)
                                }
                                .padding(20)
                            }
                        }
                    }
                    .disabled(store.isProtectedLayerSelected)
                    .frame(maxWidth: .infinity)

                    // Expanded Encoder & Joystick sub-slots selector bar
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 6) {
                            Text("旋钮动作：")
                                .font(UITheme.Typography.captionMedium)
                                .foregroundStyle(UITheme.textSecondary)
                            ForEach(13..<16, id: \.self) { slot in
                                subSlotPill(slot: slot)
                            }
                            Spacer()
                            if !store.isProtectedLayerSelected,
                               (bindings.count < 20 || store.currentControlSettings?.joystick_mode != "logical_sectors" || (store.currentControlSettings?.active_sectors ?? 0) < 4),
                               !store.browsingDeviceSnapshot {
                                Button("启用四向摇杆扇区") {
                                    Task { _ = await store.saveControlSettings(mode: "logical_sectors", sectors: "4") }
                                }
                                .font(UITheme.Typography.captionMedium)
                                .disabled(!store.canEditDraft || store.busy || store.editingDevice)
                            }
                        }

                        if bindings.count > 16 {
                            HStack(spacing: 6) {
                                Text("摇杆方向：")
                                    .font(UITheme.Typography.captionMedium)
                                    .foregroundStyle(UITheme.textSecondary)
                                ScrollView(.horizontal, showsIndicators: false) {
                                    HStack(spacing: 6) {
                                        ForEach(16..<bindings.count, id: \.self) { slot in
                                            subSlotPill(slot: slot)
                                        }
                                    }
                                }
                            }
                        }
                    }
                    .frame(maxWidth: 500)
                    .frame(maxWidth: .infinity)
                }
            }
            .padding(22)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(UITheme.surface)
    }

    private var joystickHardwareCluster: some View {
        let isJoystickSelected = !store.isProtectedLayerSelected && store.slot >= 16
        let targetSlot = max(16, store.slot >= 16 ? store.slot : 16)
        return Button {
            if bindings.count > 16 {
                store.slot = targetSlot
            } else if !store.browsingDeviceSnapshot, store.canEditDraft, !store.editingDevice {
                Task {
                    _ = await store.saveControlSettings(mode: "logical_sectors", sectors: "4")
                    store.slot = 16
                }
            }
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                Text("四向摇杆")
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(isJoystickSelected ? UITheme.accent : UITheme.textMuted)
                Text(bindings.count > 16 ? label(min(targetSlot, bindings.count - 1)) : "点击启用四向")
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(2)
            }
            .padding(8)
            .frame(maxWidth: .infinity, minHeight: 62, alignment: .topLeading)
            .background(
                isJoystickSelected ? UITheme.accentSoft : UITheme.surface,
                in: RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                    .strokeBorder(
                        isJoystickSelected ? UITheme.accent : UITheme.strongBorder,
                        style: StrokeStyle(lineWidth: isJoystickSelected ? 1.5 : 1, dash: [4, 3])
                    )
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("四向摇杆")
    }

    private var encoderHardwareCluster: some View {
        let isEncoderSelected = !store.isProtectedLayerSelected && (13..<16).contains(store.slot)
        let activeEncoderSlot = (13..<16).contains(store.slot) ? store.slot : 15
        return Button {
            store.slot = activeEncoderSlot
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                Text(shortPositionLabel(activeEncoderSlot))
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(isEncoderSelected ? UITheme.accent : UITheme.textMuted)
                Text(label(activeEncoderSlot))
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(2)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, minHeight: 62, alignment: .topLeading)
            .background(
                isEncoderSelected ? UITheme.accentSoft : UITheme.surface,
                in: RoundedRectangle(cornerRadius: 24)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 24)
                    .strokeBorder(isEncoderSelected ? UITheme.accent : UITheme.strongBorder, lineWidth: isEncoderSelected ? 1.5 : 1)
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(title(activeEncoderSlot))，\(label(activeEncoderSlot))")
    }

    private func physicalKeycap(slot: Int) -> some View {
        let isSelected = !store.isProtectedLayerSelected && store.slot == slot
        return Button {
            store.slot = slot
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                Text(shortPositionLabel(slot))
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(isSelected ? UITheme.accent : UITheme.textMuted)
                Text(label(slot))
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(8)
            .frame(maxWidth: .infinity, minHeight: 62, alignment: .topLeading)
            .background(
                isSelected ? UITheme.accentSoft : UITheme.surface,
                in: RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                    .strokeBorder(isSelected ? UITheme.accent : UITheme.border, lineWidth: isSelected ? 1.5 : 1)
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(title(slot))，\(label(slot))")
    }

    private var widePhysicalKeycap: some View {
        let isSelected = !store.isProtectedLayerSelected && (store.slot == 10 || store.slot == 11)
        let displaySlot = store.slot == 11 ? 11 : 10
        return Button {
            store.slot = 10
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text(displaySlot == 11 ? "底部宽键（副轴 12）" : "底部宽键（2U）")
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(isSelected ? UITheme.accent : UITheme.textMuted)
                    Spacer()
                    if isSelected {
                        Text("已选中")
                            .font(UITheme.Typography.captionMedium)
                            .foregroundStyle(UITheme.accent)
                    }
                }
                Text(label(displaySlot))
                    .font(UITheme.Typography.bodySemibold)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(2)
            }
            .padding(8)
            .frame(maxWidth: .infinity, minHeight: 62, alignment: .topLeading)
            .background(
                isSelected ? UITheme.accentSoft : UITheme.surface,
                in: RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                    .strokeBorder(isSelected ? UITheme.accent : UITheme.border, lineWidth: isSelected ? 1.5 : 1)
            )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(title(displaySlot))，\(label(displaySlot))")
    }

    private func subSlotPill(slot: Int) -> some View {
        let isSelected = !store.isProtectedLayerSelected && store.slot == slot
        return Button {
            store.slot = slot
        } label: {
            Text("\(shortPositionLabel(slot))：\(label(slot))")
                .font(UITheme.Typography.captionMedium)
                .foregroundStyle(isSelected ? UITheme.accent : UITheme.textPrimary)
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(
                    isSelected ? UITheme.accentSoft : UITheme.subtleSurface,
                    in: RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                        .strokeBorder(isSelected ? UITheme.accent : UITheme.border, lineWidth: 1)
                )
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(title(slot))，\(label(slot))")
    }

    // MARK: - Right Inspector (Inline Action Assignment)
    private var rightInspector: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                // Selected key header
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(title(store.slot))
                            .font(UITheme.Typography.heading)
                        Spacer()
                        if DesktopAgent.named(store.profile?.name ?? "") != nil, !store.browsingDeviceSnapshot {
                            Button("试运行一次") {
                                Task { await store.tryDesktopAction() }
                            }
                            .font(UITheme.Typography.captionMedium)
                            .disabled(!store.canTryDesktopAction)
                        }
                    }
                    Text("当前功能：\(label(store.slot))")
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.accent)
                }

                if store.slot == 10 || store.slot == 11 {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("底行左中键硬件布局（全局）")
                            .font(UITheme.Typography.captionMedium)
                            .foregroundStyle(UITheme.textPrimary)
                        Text("跟随实体键盘键帽：拆成两颗 1U 按键（开关 11 + 开关 12），或合并为一颗 2U 宽键。")
                            .font(UITheme.Typography.caption)
                            .foregroundStyle(UITheme.textSecondary)
                        HStack(spacing: 6) {
                            Button {
                                Task { await store.setBottomRowSplit(true) }
                            } label: {
                                Text("拆成两个按键 (1U + 1U)")
                                    .font(UITheme.Typography.captionMedium)
                                    .foregroundStyle(store.isBottomRowSplit ? UITheme.accent : UITheme.textPrimary)
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 6)
                                    .background(
                                        store.isBottomRowSplit ? UITheme.accentSoft : UITheme.surface,
                                        in: RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                                    )
                                    .overlay(
                                        RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                                            .strokeBorder(store.isBottomRowSplit ? UITheme.accent : UITheme.border, lineWidth: 1)
                                    )
                            }
                            .buttonStyle(.plain)
                            .disabled(store.busy)

                            Button {
                                Task { await store.setBottomRowSplit(false) }
                            } label: {
                                Text("合并为 2U 宽键")
                                    .font(UITheme.Typography.captionMedium)
                                    .foregroundStyle(!store.isBottomRowSplit ? UITheme.accent : UITheme.textPrimary)
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 6)
                                    .background(
                                        !store.isBottomRowSplit ? UITheme.accentSoft : UITheme.surface,
                                        in: RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                                    )
                                    .overlay(
                                        RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                                            .strokeBorder(!store.isBottomRowSplit ? UITheme.accent : UITheme.border, lineWidth: 1)
                                    )
                            }
                            .buttonStyle(.plain)
                            .disabled(store.busy)
                        }
                        if !store.isBottomRowSplit {
                            HStack {
                                Text("双轴合并触发（覆盖开关 11 与 12）")
                                    .font(UITheme.Typography.caption)
                                    .foregroundStyle(UITheme.textSecondary)
                                Spacer()
                                Button(store.slot == 11 ? "切回主槽 11" : "查看副轴 12") {
                                    store.slot = store.slot == 11 ? 10 : 11
                                }
                                .font(UITheme.Typography.captionMedium)
                                .buttonStyle(.plain)
                                .foregroundStyle(UITheme.accent)
                            }
                        }
                    }
                    .padding(10)
                    .background(UITheme.surface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                    .overlay(
                        RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                            .strokeBorder(UITheme.border, lineWidth: 1)
                    )
                }

                // Inline Action Picker (Direct 1-Click Assignment)
                if !store.browsingDeviceSnapshot && !(store.editingDevice && store.slot >= 13) {
                    VStack(alignment: .leading, spacing: 8) {
                        TextField("搜索功能、快捷键或应用…", text: $inspectorSearch)
                            .textFieldStyle(.roundedBorder)
                            .font(UITheme.Typography.body)

                        HStack(spacing: 8) {
                            Picker("分类", selection: $inspectorCategory) {
                                Text("全部").tag("全部")
                                ForEach(store.library.categories, id: \.self) { Text($0).tag($0) }
                            }
                            .labelsHidden()
                            .font(UITheme.Typography.caption)

                            Toggle("收藏", isOn: $inspectorFavoritesOnly)
                                .toggleStyle(.checkbox)
                                .font(UITheme.Typography.caption)

                            Spacer()

                            Button("完整动作库…") { choosing = true }
                                .font(UITheme.Typography.captionMedium)
                                .buttonStyle(.plain)
                                .foregroundStyle(UITheme.accent)
                        }

                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: 6) {
                                ForEach(store.library.entries.filter { !store.editingDevice || $0.action.payload["type"]?.text == "device_key" }) { entry in
                                    let isCurrent = action(store.slot)?.id == entry.action.id && action(store.slot)?.name == entry.action.name
                                    Button {
                                        Task { _ = await store.saveAction(entry.action) }
                                    } label: {
                                        HStack(alignment: .center, spacing: 8) {
                                            VStack(alignment: .leading, spacing: 2) {
                                                Text(entry.action.name.replacingOccurrences(of: " · ", with: "："))
                                                    .font(UITheme.Typography.bodyMedium)
                                                    .foregroundStyle(UITheme.textPrimary)
                                                    .lineLimit(2)
                                                Text(entry.capability.execution_available ? "目标：\(entry.capability.target)" : entry.capability.unavailable_reason)
                                                    .font(UITheme.Typography.caption)
                                                    .foregroundStyle(UITheme.textSecondary)
                                                    .lineLimit(1)
                                            }
                                            Spacer(minLength: 4)
                                            if isCurrent {
                                                Text("已分配")
                                                    .font(UITheme.Typography.captionMedium)
                                                    .foregroundStyle(UITheme.accent)
                                                    .padding(.horizontal, 6)
                                                    .padding(.vertical, 2)
                                                    .background(UITheme.accentSoft, in: RoundedRectangle(cornerRadius: 4))
                                            }
                                        }
                                        .padding(.horizontal, 10)
                                        .padding(.vertical, 8)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .background(
                                            isCurrent ? UITheme.accentSoft : UITheme.surface,
                                            in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                                        )
                                        .overlay(
                                            RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                                                .strokeBorder(isCurrent ? UITheme.accent : UITheme.border, lineWidth: 1)
                                        )
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(store.busy || !store.canEditDraft)
                                }
                            }
                        }
                        .frame(height: 230)
                    }
                }

                // Parameter editor for currently bound action
                if let a = action(store.slot) {
                    Rectangle().fill(UITheme.border).frame(height: 1)
                    ActionParameters(action: a, store: store, macroID: store.selectedMacro?.id, multiID: store.selectedMulti?.id)
                        .id(store.slotKey + String(store.draft.revision))
                }

                Rectangle().fill(UITheme.border).frame(height: 1)

                // Macros, Multi, and Joystick sector settings
                HStack(spacing: 8) {
                    Button("普通宏（\(store.macros.count) 个）") { choosingMacro = true }
                        .font(UITheme.Typography.captionMedium)
                        .disabled(!store.canEditDraft || store.editingDevice)
                    Button("多分支动作（\(store.multis.count) 个）") { choosingMulti = true }
                        .font(UITheme.Typography.captionMedium)
                        .disabled(!store.canEditDraft || store.editingDevice)
                }

                if let multi = store.selectedMulti {
                    HStack {
                        Text("共享多分支动作（\(multi.references.count) 处使用）")
                            .font(UITheme.Typography.caption)
                            .foregroundStyle(UITheme.textSecondary)
                        Spacer()
                        Button("解除绑定") { Task { await store.bindMulti(nil) } }
                            .font(UITheme.Typography.caption)
                            .disabled(store.busy || !store.canEditDraft)
                    }
                }
                if let macro = store.selectedMacro {
                    HStack {
                        Text("共享宏（\(macro.references.count) 处使用）")
                            .font(UITheme.Typography.caption)
                            .foregroundStyle(UITheme.textSecondary)
                        Spacer()
                        Button("解除绑定") { Task { await store.bindMacro(nil) } }
                            .font(UITheme.Typography.caption)
                            .disabled(store.busy || !store.canEditDraft)
                    }
                }

                if !store.browsingDeviceSnapshot && !store.editingDevice && store.slot >= 13 {
                    DisclosureGroup("旋钮与摇杆扇区数量设置") {
                        ControlSettingsEditor(store: store)
                            .id((store.layer?.id.text ?? "") + String(store.draft.revision))
                            .padding(.top, 6)
                    }
                    .font(UITheme.Typography.captionMedium)
                }
            }
            .padding(16)
        }
        .frame(width: 296)
        .background(UITheme.subtleSurface)
    }

    // MARK: - Bottom Footer Bar
    private var bottomFooterBar: some View {
        let cleanedStatus = store.status
            .replacingOccurrences(of: " · ", with: "，")
            .replacingOccurrences(of: "·", with: "，")
            .replacingOccurrences(of: " (", with: "（")
            .replacingOccurrences(of: ")", with: "）")
        return HStack {
            Text(cleanedStatus)
                .font(UITheme.Typography.caption)
                .foregroundStyle(UITheme.textSecondary)
                .lineLimit(1)
            Spacer()
            Text("长按说话上限 120 秒，响应超时 8 秒")
                .font(UITheme.Typography.caption)
                .foregroundStyle(UITheme.textMuted)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 8)
        .background(UITheme.subtleSurface)
    }

    var visibleProfileIndices: [Int] {
        guard let profiles = store.snapshot?.profiles else { return [] }
        let order = DesktopAgent.all.map(\.name)
        return profiles.indices.filter { store.editingDevice || store.browsingDeviceSnapshot || showOtherProfiles || DesktopAgent.named(profiles[$0].name) != nil }
            .sorted { (order.firstIndex(of: profiles[$0].name) ?? (order.count + $0)) < (order.firstIndex(of: profiles[$1].name) ?? (order.count + $1)) }
    }

    var discoveryPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Label(store.connectionSummary, systemImage: "cable.connector")
                    .font(UITheme.Typography.captionMedium)
                Spacer()
                if !store.devices.isEmpty {
                    Picker("USB 候选", selection: Binding(get: { store.selectedDeviceID ?? "" }, set: { store.selectDevice($0) })) {
                        Text("请选择设备").tag("")
                        ForEach(Array(store.devices.enumerated()), id: \.element.id) { index, device in
                            Text("\(device.product)（\(String(format: "%04X", device.pid))，接口 \(index + 1)\(device.verifiedModel ? "" : "，未验证型号")）").tag(device.id)
                        }
                    }.frame(maxWidth: 410)
                }
                Button("自动连接并启用") { Task { await store.autoSetupHostExecutionIfNeeded() } }.disabled(store.busy)
                Button("刷新 USB") { Task { await store.startDiscovery(); await store.refresh() } }
                Button("返回日常方案") { store.showLocalDraft() }.disabled(!store.browsingDeviceSnapshot || store.busy)
            }
            HStack {
                Button("连接设备") { if let id = store.selectedDeviceID { Task { await store.connect(id) } } }
                    .disabled(store.busy || store.selectedDevice?.verifiedModel != true)
                Button("查询固件版本") { Task { await store.readIdentity() } }
                    .disabled(store.busy || store.connectedID == nil || store.connectedID != store.selectedDeviceID)
                Button("读取硬件快照") { Task { await store.readDeviceConfig() } }
                    .disabled(store.busy || store.connectedID == nil || store.connectedID != store.selectedDeviceID || !["0.6.3", "synthetic-0.6.3"].contains(store.identity?.version ?? ""))
                Button("断开连接") { Task { await store.suspend() } }.disabled(!store.canDisconnect)
                Button("导出诊断") { showingDiagnosticExport = true }
                if store.browsingDeviceSnapshot {
                    Button("从快照创建草稿") { Task { await store.createDraftFromSnapshot() } }
                        .disabled(store.busy || store.stale || store.connectedID == nil)
                }
                Button("对比硬件与草稿") { Task { await store.compareSnapshot() } }.disabled(store.busy || store.connectedID == nil || store.stale)
                Text(store.identityStatus).font(UITheme.Typography.caption).foregroundStyle(UITheme.textSecondary)
            }
            if let files = store.snapshot?.device_files, store.browsingDeviceSnapshot {
                DisclosureGroup("硬件文件校验（\(files.count) 个文件）") {
                    ForEach(files, id: \.name) { file in
                        Text("\(file.name)（\(file.size) 字节，SHA-1 \(file.sha1)）")
                            .font(UITheme.Typography.mono)
                            .textSelection(.enabled)
                    }
                }
                .font(UITheme.Typography.caption)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 10)
    }

    var actionSheet: some View {
        ActionLibrarySheet(store: store) { choosing = false }
    }

    var nameSheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(nameOperation).font(UITheme.Typography.title)
            TextField("请输入名称", text: $objectName)
                .textFieldStyle(.roundedBorder)
                .font(UITheme.Typography.body)
            HStack {
                Button("取消") { naming = false }.keyboardShortcut(.cancelAction)
                Spacer()
                Button("保存") {
                    let name = objectName
                    let operation = nameOperation
                    naming = false
                    Task {
                        switch operation {
                        case "新建配置": await store.createProfile(name: name)
                        case "重命名配置": await store.renameProfile(name: name)
                        case "复制配置": await store.duplicateProfile(name: name)
                        case "新建层": await store.createLayer(name: name)
                        case "重命名层": await store.renameLayer(name: name)
                        case "复制层": await store.duplicateLayer(name: name)
                        default: break
                        }
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(objectName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding(24)
        .frame(width: 360)
        .background(UITheme.surface)
    }

    var diffSheet: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("已修改项（\(changeCount) 处）").font(UITheme.Typography.title)
            ScrollView {
                ForEach(Array(store.draft.structure_changes.enumerated()), id: \.offset) { _, change in
                    Text(change)
                        .font(UITheme.Typography.body)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.vertical, 4)
                }
                ForEach(store.draft.edits.keys.sorted(), id: \.self) { key in
                    if let snapshot = store.snapshot {
                        ForEach(snapshot.profiles.indices, id: \.self) { pi in
                            let p = snapshot.profiles[pi]
                            ForEach(p.layers.indices, id: \.self) { li in
                                let l = p.layers[li]
                                let values = l.keys + l.encoders + l.joystick.sectors
                                ForEach(values.indices, id: \.self) { si in
                                    if bindingKey(profile: p.id, layer: l.id, slot: si) == key, let action = store.draft.edits[key] {
                                        VStack(alignment: .leading, spacing: 4) {
                                            Text("\(DesktopAgent.displayName(p.name)) / \(l.name) / \(title(si))")
                                                .font(UITheme.Typography.bodySemibold)
                                            Text("\(values[si].bindingLabel) 改为 \(action.name)")
                                                .font(UITheme.Typography.caption)
                                            Text(store.parameterSummary(action))
                                                .font(UITheme.Typography.caption)
                                                .foregroundStyle(UITheme.textSecondary)
                                        }
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                        .padding(.vertical, 6)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            HStack {
                Spacer()
                Button("完成") { reviewing = false }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(24)
        .frame(width: 520, height: 360)
        .background(UITheme.surface)
    }
}

struct ActionLibrarySheet: View {
    @ObservedObject var store: Store
    var close: () -> Void
    @State private var search = ""
    @State private var category = "全部"
    @State private var favoritesOnly = false
    @State private var candidate: CatalogAction?
    @FocusState private var searchFocused: Bool

    func choose(_ action: CatalogAction) {
        candidate = action
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("分配动作").font(UITheme.Typography.title)
                Spacer()
                Button("关闭") { close() }.keyboardShortcut(.cancelAction)
            }
            if let candidate {
                Button("返回动作列表") { self.candidate = nil }
                    .font(UITheme.Typography.captionMedium)
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        Text(candidate.name).font(UITheme.Typography.heading)
                        if let info = store.library.entries.first(where: { $0.id == candidate.id })?.capability {
                            ActionCapabilityView(capability: info)
                        }
                        ActionParameters(action: candidate, store: store, saved: close).id(candidate.id)
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                TextField("搜索功能、应用或快捷键", text: $search)
                    .textFieldStyle(.roundedBorder)
                    .font(UITheme.Typography.body)
                    .focused($searchFocused)
                    .defaultFocus($searchFocused, true)
                    .onSubmit { if let first = store.library.entries.first { choose(first.action) } }
                HStack {
                    Picker("分类", selection: $category) {
                        Text("全部").tag("全部")
                        ForEach(store.library.categories, id: \.self) { Text($0).tag($0) }
                    }
                    Toggle("只看收藏", isOn: $favoritesOnly)
                }
                .font(UITheme.Typography.body)

                if let error = store.library.preferences_error {
                    Text(error).font(UITheme.Typography.caption).foregroundStyle(UITheme.caution)
                }
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 8) {
                        if store.library.entries.isEmpty {
                            Text("未找到匹配的动作")
                                .font(UITheme.Typography.body)
                                .foregroundStyle(UITheme.textSecondary)
                                .padding()
                        }
                        if !favoritesOnly && (category == "全部" || category == "普通宏") {
                            ForEach(store.macros.filter { search.isEmpty || $0.action.name.localizedCaseInsensitiveContains(search) }) { macro in
                                Button {
                                    Task { await store.bindMacro(macro.id); close() }
                                } label: {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(macro.action.name).font(UITheme.Typography.bodySemibold)
                                        Text("本机共享宏（\(macro.references.count) 个按键正在使用）")
                                            .font(UITheme.Typography.caption)
                                            .foregroundStyle(UITheme.textSecondary)
                                    }
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(10)
                                    .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                                }
                                .buttonStyle(.plain)
                                .disabled(store.busy || !store.canEditDraft)
                            }
                        }
                        ForEach(store.library.entries.filter { !store.editingDevice || $0.action.payload["type"]?.text == "device_key" }) { entry in
                            HStack(alignment: .center) {
                                Button { choose(entry.action) } label: {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(entry.action.name).font(UITheme.Typography.bodySemibold)
                                        Text(entry.capability.execution_available ? "目标：\(entry.capability.target)" : entry.capability.unavailable_reason)
                                            .font(UITheme.Typography.caption)
                                            .foregroundStyle(entry.capability.execution_available ? UITheme.accent : UITheme.textSecondary)
                                    }
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(10)
                                }
                                .buttonStyle(.plain)
                                .disabled(store.busy || !store.canEditDraft)

                                Button {
                                    Task { await store.setFavorite(entry.id, favorite: !entry.favorite) }
                                } label: {
                                    Image(systemName: entry.favorite ? "star.fill" : "star")
                                        .foregroundStyle(entry.favorite ? UITheme.accent : UITheme.textMuted)
                                }
                                .buttonStyle(.borderless)
                                .padding(10)
                                .accessibilityLabel("\(entry.favorite ? "取消收藏" : "收藏") \(entry.action.name)")
                                .disabled(store.library.preferences_error != nil)
                            }
                            .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                        }
                    }
                }
            }
        }
        .padding(24)
        .frame(width: 580, height: 520)
        .background(UITheme.surface)
        .onAppear { search = ""; category = "全部"; favoritesOnly = false; candidate = nil; searchFocused = true }
        .task(id: "\(search)\u{0}\(category)\u{0}\(favoritesOnly)") {
            await store.searchActions(query: search, category: category, favoritesOnly: favoritesOnly)
        }
    }
}

// R-K05: typed parameters edited locally; host-executable adapters (herdr / dsh) dispatch on macOS.
struct ActionParameters: View {
    @State var action: CatalogAction
    @State private var editingMacro = false
    @State private var editingMulti = false
    @ObservedObject var store: Store
    var macroID: String? = nil
    var multiID: String? = nil
    var saved: (() -> Void)? = nil
    var type: String { action.payload["type"]?.text ?? "unknown" }
    var fields: [(String, String)] {
        switch type {
        case "text": return [("text", "输入文本内容")]
        case "open_url": return [("url", "网址（HTTP 或 HTTPS）")]
        case "shortcut": return [("key", "按键名称"), ("target", "限定目标应用（留空为全局）")]
        case "agent_command": return [("adapter", "目标软件标识"), ("command", "操作指令")]
        case "focus_session": return [("adapter", "目标软件标识"), ("session", "会话标识")]
        case "skill_reference": return [("adapter", "目标软件标识"), ("skill", "技能名称")]
        default: return []
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if type == "local_multi", MultiEditorDraft(action: action) != nil {
                Button("编辑四分支动作（点按 / 双击 / 长按）") { editingMulti = true }
                    .font(UITheme.Typography.body)
                    .disabled(!store.canEditDraft || store.busy)
                Text(store.parameterSummary(action))
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(UITheme.textSecondary)
            }
            if type == "local_macro", MacroEditorDraft(action: action) != nil {
                Button("编辑宏步骤（按键与毫秒延时）") { editingMacro = true }
                    .font(UITheme.Typography.body)
                    .disabled(!store.canEditDraft || store.busy)
            }
            if !["local_macro", "local_multi"].contains(type) {
                Text("功能名称")
                    .font(UITheme.Typography.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                TextField("功能名称", text: $action.name)
                    .textFieldStyle(.roundedBorder)
                    .font(UITheme.Typography.body)
                ForEach(fields, id: \.0) { key, label in
                    Text(label)
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.textSecondary)
                    TextField(label, text: Binding(get: { action.payload[key]?.text ?? "" }, set: { action.payload[key] = .string($0) }), axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .font(UITheme.Typography.body)
                        .lineLimit(1...4)
                }
                if type == "device_key" {
                    Picker("硬件键值", selection: Binding(get: { action.payload["token"]?.text ?? "KC_B" }, set: { action.payload["token"] = .string($0) })) {
                        ForEach(["KC_A", "KC_B", "KC_ENT", "KC_ESC", "KC_SPC", "KC_TAB", "KC_BSPC", "KC_VOLU", "KC_VOLD", "KC_MPLY", "KC_MUTE", "KC_MNXT", "KC_MPRV", "KC_F1", "KC_F2", "KC_F3", "KC_F4", "KC_F5", "KC_F6", "KC_F7", "KC_F8", "KC_F9", "KC_F10", "KC_F11", "KC_F12", "KI_LSNEXT", "KI_LSPREV", "KI_PS1", "KC_ENTER", "KC_SPACE"], id: \.self) { Text($0).tag($0) }
                    }
                    .font(UITheme.Typography.body)
                }
                if type == "open_app" {
                    Text("目标应用：\(action.payload["bundle_id"]?.text ?? "未选择")")
                        .font(UITheme.Typography.caption)
                        .textSelection(.enabled)
                    Button("选择应用程序…") {
                        let panel = NSOpenPanel()
                        panel.allowedContentTypes = [.applicationBundle]
                        panel.canChooseDirectories = false
                        panel.allowsMultipleSelection = false
                        panel.directoryURL = URL(fileURLWithPath: "/Applications")
                        panel.begin { result in
                            if result == .OK, let url = panel.url, let bundleID = Bundle(url: url)?.bundleIdentifier {
                                action.payload["bundle_id"] = .string(bundleID)
                                action.name = "打开 \(url.deletingPathExtension().lastPathComponent)"
                            }
                        }
                    }
                    .font(UITheme.Typography.body)
                }
                if type == "shell_command" {
                    Text("使用 /bin/sh 执行，单次运行上限 10 秒")
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(UITheme.textSecondary)
                    TextField("终端命令", text: Binding(get: { action.payload["command"]?.text ?? "" }, set: { action.payload["command"] = .string($0) }), axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .font(UITheme.Typography.mono)
                        .lineLimit(1...4)
                    TextField("工作目录路径", text: Binding(get: { action.payload["working_directory"]?.text ?? "" }, set: { action.payload["working_directory"] = .string($0) }))
                        .textFieldStyle(.roundedBorder)
                        .font(UITheme.Typography.mono)
                }
                if type == "shortcut" {
                    ShortcutRecorder(
                        action: $action,
                        isDisabled: !store.canEditDraft || store.busy
                    )
                    HStack(spacing: 8) {
                        Button("选择目标应用…") {
                            let panel = NSOpenPanel()
                            panel.allowedContentTypes = [.applicationBundle]
                            panel.canChooseDirectories = false
                            panel.allowsMultipleSelection = false
                            panel.directoryURL = URL(fileURLWithPath: "/Applications")
                            panel.begin { result in
                                if result == .OK, let url = panel.url, let bundleID = Bundle(url: url)?.bundleIdentifier {
                                    action.payload["target"] = .string(bundleID)
                                }
                            }
                        }
                        .font(UITheme.Typography.caption)
                        if (action.payload["target"]?.text ?? "frontmost-editor") != "frontmost-editor" {
                            Button("恢复为当前前台应用") {
                                action.payload["target"] = .string("frontmost-editor")
                            }
                            .font(UITheme.Typography.caption)
                        }
                    }
                    HStack(spacing: 10) {
                        ForEach([("command", "⌘ Cmd"), ("shift", "⇧ Shift"), ("option", "⌥ Opt"), ("control", "⌃ Ctrl")], id: \.0) { modifier, label in
                            Toggle(label, isOn: Binding(get: { modifiers.contains(modifier) }, set: { enabled in
                                var next = modifiers.filter { $0 != modifier }
                                if enabled { next.append(modifier) }
                                action.payload["modifiers"] = .array(next.map { .string($0) })
                            }))
                            .toggleStyle(.checkbox)
                            .font(UITheme.Typography.caption)
                        }
                    }
                }
                Button("应用修改") {
                    Task { if await store.saveAction(action) { saved?() } }
                }
                .font(UITheme.Typography.bodyMedium)
                .disabled(!store.canEditDraft || store.busy)
            }
        }
        .sheet(isPresented: $editingMulti) { MultiEditor(action: action, multiID: multiID, store: store, saved: saved) }
        .sheet(isPresented: $editingMacro) { MacroEditor(action: action, macroID: macroID, store: store, saved: saved) }
    }
    var modifiers: [String] {
        if case .array(let values) = action.payload["modifiers"] { return values.map(\.text) }
        return []
    }
}

enum ShortcutCaptureOutcome: Equatable {
    case recorded(ShortcutChord)
    case cancelled
    case ignored
}

struct ShortcutChord: Equatable {
    var key: String
    var modifiers: [String]

    private static let modifierOrder = ["command", "shift", "option", "control"]

    static let keyCodeToToken: [UInt16: String] = [
        0x00: "a", 0x01: "s", 0x02: "d", 0x03: "f", 0x04: "h", 0x05: "g",
        0x06: "z", 0x07: "x", 0x08: "c", 0x09: "v", 0x0B: "b", 0x0C: "q",
        0x0D: "w", 0x0E: "e", 0x0F: "r", 0x10: "y", 0x11: "t", 0x12: "1",
        0x13: "2", 0x14: "3", 0x15: "4", 0x16: "6", 0x17: "5", 0x18: "equal",
        0x19: "9", 0x1A: "7", 0x1B: "minus", 0x1C: "8", 0x1D: "0",
        0x1E: "right_bracket", 0x1F: "o", 0x20: "u", 0x21: "left_bracket",
        0x22: "i", 0x23: "p", 0x24: "return", 0x25: "l", 0x26: "j",
        0x27: "quote", 0x28: "k", 0x29: "semicolon", 0x2A: "backslash",
        0x2B: "comma", 0x2C: "slash", 0x2D: "n", 0x2E: "m", 0x2F: "period",
        0x30: "tab", 0x31: "space", 0x32: "grave", 0x33: "delete",
        0x35: "escape", 0x3F: "fn", 0x60: "f5", 0x61: "f6", 0x62: "f7", 0x63: "f3",
        0x64: "f8", 0x65: "f9", 0x67: "f11", 0x6D: "f10", 0x6F: "f12",
        0x73: "home", 0x74: "page_up", 0x75: "forward_delete", 0x76: "f4",
        0x77: "end", 0x78: "f2", 0x79: "page_down", 0x7A: "f1",
        0x7B: "left", 0x7C: "right", 0x7D: "down", 0x7E: "up",
    ]

    init(key: String, modifiers: [String]) {
        self.key = key.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        let active = Set(modifiers.map { $0.lowercased() })
        self.modifiers = Self.modifierOrder.filter { active.contains($0) }
    }

    init?(payload: [String: JSONValue]) {
        let rawKey = (payload["key"]?.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !rawKey.isEmpty else { return nil }
        let mods: [String]
        if case .array(let values) = payload["modifiers"] {
            mods = values.map(\.text)
        } else {
            mods = []
        }
        self.init(key: rawKey, modifiers: mods)
    }

    static func from(event: NSEvent) -> ShortcutCaptureOutcome {
        if event.type == .flagsChanged {
            guard event.keyCode == 0x3F, event.modifierFlags.contains(.function) else {
                return .ignored
            }
            return from(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
        }
        return from(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
    }

    static func from(keyCode: UInt16, modifierFlags: NSEvent.ModifierFlags) -> ShortcutCaptureOutcome {
        let flags = modifierFlags.intersection(.deviceIndependentFlagsMask)
        var mods: [String] = []
        if flags.contains(.command) { mods.append("command") }
        if flags.contains(.shift) { mods.append("shift") }
        if flags.contains(.option) { mods.append("option") }
        if flags.contains(.control) { mods.append("control") }

        if keyCode == 0x35 && mods.isEmpty {
            return .cancelled
        }
        guard let token = keyCodeToToken[keyCode] else {
            return .ignored
        }
        return .recorded(ShortcutChord(key: token, modifiers: mods))
    }

    var displayLabel: String {
        let orderedSymbols: [(String, String)] = [
            ("control", "⌃"),
            ("option", "⌥"),
            ("shift", "⇧"),
            ("command", "⌘"),
        ]
        let symbols = orderedSymbols.compactMap { mod, symbol in
            modifiers.contains(mod) ? symbol : nil
        }.joined()
        return symbols + Self.displayKeyName(key)
    }

    private static func displayKeyName(_ key: String) -> String {
        switch key.lowercased() {
        case "fn", "globe", "apple_fn", "fn_globe": return "🌐 Fn"
        case "space": return "Space"
        case "return", "enter": return "↩"
        case "tab": return "⇥"
        case "delete", "backspace": return "⌫"
        case "forward_delete", "forwarddelete": return "⌦"
        case "escape", "esc": return "⎋"
        case "up": return "↑"
        case "down": return "↓"
        case "left": return "←"
        case "right": return "→"
        case "home": return "Home"
        case "end": return "End"
        case "page_up", "pageup": return "PgUp"
        case "page_down", "pagedown": return "PgDn"
        case "minus": return "-"
        case "equal": return "="
        case "left_bracket", "leftbracket": return "["
        case "right_bracket", "rightbracket": return "]"
        case "backslash": return "\\"
        case "semicolon": return ";"
        case "quote": return "'"
        case "comma": return ","
        case "period": return "."
        case "slash": return "/"
        case "grave", "backtick": return "`"
        default: return key.uppercased()
        }
    }

    func apply(to action: inout CatalogAction) {
        let previousDefaultName = ShortcutChord(payload: action.payload).map { "快捷键 \($0.displayLabel)" }
        action.payload["key"] = .string(key)
        action.payload["modifiers"] = .array(modifiers.map { .string($0) })
        let trimmedName = action.name.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedName.isEmpty || trimmedName == "快捷键" || trimmedName == previousDefaultName {
            action.name = "快捷键 \(displayLabel)"
        }
    }
}

struct ShortcutRecorder: View {
    @Binding var action: CatalogAction
    var isDisabled: Bool = false
    var onCommit: ((CatalogAction) -> Void)? = nil
    @State private var isRecording = false
    @State private var eventMonitor: Any?

    private var currentChord: ShortcutChord? {
        ShortcutChord(payload: action.payload)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Button {
                    if isRecording {
                        stopRecording()
                    } else {
                        startRecording()
                    }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: isRecording ? "record.circle.fill" : "keyboard")
                            .foregroundStyle(isRecording ? UITheme.accent : UITheme.textSecondary)
                        Text(isRecording ? "请直接按下组合键（Esc 取消）…" : (currentChord?.displayLabel ?? "未设置"))
                            .font(UITheme.Typography.bodyMedium)
                            .foregroundStyle(isRecording ? UITheme.accent : UITheme.textPrimary)
                        Spacer()
                        Text(isRecording ? "取消录制" : "录制快捷键")
                            .font(UITheme.Typography.captionMedium)
                            .foregroundStyle(UITheme.accent)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                    .background(
                        isRecording ? UITheme.accentSoft : UITheme.subtleSurface,
                        in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                            .strokeBorder(isRecording ? UITheme.accent : UITheme.border, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
                .disabled(isDisabled)
                .accessibilityLabel(isRecording ? "正在录制快捷键，按 Esc 取消" : "录制快捷键，当前为 \(currentChord?.displayLabel ?? "未设置")")

                if currentChord != nil && !isRecording {
                    Button {
                        action.payload["key"] = .string("")
                        action.payload["modifiers"] = .array([])
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(UITheme.textMuted)
                    }
                    .buttonStyle(.plain)
                    .help("清除快捷键")
                    .accessibilityLabel("清除快捷键")
                    .disabled(isDisabled)
                }
            }
        }
        .onDisappear {
            stopRecording()
        }
    }

    private func startRecording() {
        stopRecording()
        isRecording = true
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .flagsChanged]) { event in
            switch ShortcutChord.from(event: event) {
            case .cancelled:
                stopRecording()
                return nil
            case .ignored:
                return event.type == .flagsChanged ? event : nil
            case .recorded(let chord):
                var updated = action
                chord.apply(to: &updated)
                action = updated
                stopRecording()
                onCommit?(updated)
                return nil
            }
        }
    }

    private func stopRecording() {
        if let eventMonitor {
            NSEvent.removeMonitor(eventMonitor)
            self.eventMonitor = nil
        }
        isRecording = false
    }
}

struct ActionCapabilityView: View {
    let capability: ActionCapability
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Label(capability.execution_available ? "当前可执行" : capability.unavailable_reason, systemImage: capability.execution_available ? "checkmark.circle.fill" : "pause.circle")
                .font(UITheme.Typography.captionMedium)
                .foregroundStyle(capability.execution_available ? UITheme.accent : UITheme.caution)
            Text("作用对象：\(capability.target)")
                .font(UITheme.Typography.caption)
                .foregroundStyle(UITheme.textSecondary)
                .textSelection(.enabled)
        }
    }
}

struct MicroInputSettingsView: View {
    @ObservedObject var store: Store
    @Binding var appearance: String
    var showDiagnostics: Binding<Bool>? = nil
    var onOpenDiagnosticExport: (() -> Void)? = nil
    var onOpenDiffReview: (() -> Void)? = nil
    var close: (() -> Void)? = nil

    @ObservedObject private var visualPrefs = KeycapVisualPreferences.shared
    @State private var localShowingDiagnosticExport = false
    @State private var localShowingDiffReview = false

    private var changeCount: Int {
        store.draft.edits.count + store.draft.structure_changes.count
    }

    var body: some View {
        VStack(spacing: 0) {
            if let close {
                HStack {
                    Text("设置")
                        .font(UITheme.Typography.title)
                        .foregroundStyle(UITheme.textPrimary)
                    Spacer()
                    Button("完成") { close() }
                        .keyboardShortcut(.defaultAction)
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 12)
                Rectangle().fill(UITheme.border).frame(height: 1)
            }

            TabView {
                generalSettingsTab
                    .tabItem { Label("通用", systemImage: "gearshape") }

                lightingSettingsTab
                    .tabItem { Label("灯光", systemImage: "lightbulb") }

                hardwareSettingsTab
                    .tabItem { Label("硬件与诊断", systemImage: "keyboard") }
            }
            .padding(20)
        }
        .frame(width: 520, height: close != nil ? 420 : 380)
        .background(UITheme.surface)
        .sheet(isPresented: $localShowingDiagnosticExport) {
            DiagnosticsExportView(store: store, close: { localShowingDiagnosticExport = false })
        }
        .sheet(isPresented: $localShowingDiffReview) {
            VStack(alignment: .leading, spacing: 14) {
                Text("未同步硬件改动（\(changeCount) 项）")
                    .font(UITheme.Typography.title)
                ScrollView {
                    ForEach(Array(store.draft.structure_changes.enumerated()), id: \.offset) { _, change in
                        Text(change)
                            .font(UITheme.Typography.body)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 4)
                    }
                    ForEach(store.draft.edits.keys.sorted(), id: \.self) { key in
                        if let action = store.draft.edits[key] {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(action.name)
                                    .font(UITheme.Typography.bodySemibold)
                                Text(store.parameterSummary(action))
                                    .font(UITheme.Typography.caption)
                                    .foregroundStyle(UITheme.textSecondary)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.vertical, 4)
                        }
                    }
                }
                HStack {
                    Spacer()
                    Button("完成") { localShowingDiffReview = false }
                        .keyboardShortcut(.defaultAction)
                }
            }
            .padding(24)
            .frame(width: 480, height: 340)
            .background(UITheme.surface)
        }
    }

    private var generalSettingsTab: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("启用按键控制")
                    .font(UITheme.Typography.bodyMedium)
                Spacer()
                Toggle("启用按键控制", isOn: Binding(
                    get: { store.hostExecutionEnabled },
                    set: { value in Task { await store.setHostExecutionEnabled(value) } }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
            }

            HStack {
                Text("按键时自动唤起窗口")
                    .font(UITheme.Typography.bodyMedium)
                Spacer()
                Toggle("按键时自动唤起窗口", isOn: $visualPrefs.singleTapFocusWindow)
                    .labelsHidden()
                    .toggleStyle(.switch)
            }

            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("允许执行终端命令")
                        .font(UITheme.Typography.bodyMedium)
                    Text("开启后，绑定为终端命令的按键才会在本机执行")
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }
                Spacer()
                Toggle("允许执行终端命令", isOn: Binding(
                    get: { store.commandExecutionEnabled },
                    set: { value in Task { await store.setCommandExecutionEnabled(value) } }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
            }

            HStack {
                Text("窗口外观")
                    .font(UITheme.Typography.bodyMedium)
                Spacer()
                Picker("窗口外观", selection: $appearance) {
                    Text("跟随系统").tag("system")
                    Text("浅色").tag("light")
                    Text("深色").tag("dark")
                }
                .labelsHidden()
                .frame(width: 140)
            }

            Spacer()
        }
        .padding(.vertical, 8)
    }

    private var lightingSettingsTab: some View {
        VStack(alignment: .leading, spacing: 12) {
            SimpleLightingControlCard(
                store: store,
                lighting: store.fourAppLighting,
                showAdvancedButton: !(store.layer == nil || !store.canEditDraft || store.isProtectedLayerSelected),
                onOpenAdvancedLighting: {
                    close?()
                    store.showingLighting = true
                }
            )
            Spacer(minLength: 0)
        }
        .padding(.vertical, 4)
    }

    private var hardwareSettingsTab: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Button("读取并编辑键盘硬件配置…") {
                    close?()
                    Task { await store.editDeviceConfiguration() }
                }
                .disabled(store.busy || store.deviceOperationRunning)

                Button("同步按键与灯光到键盘…") {
                    close?()
                    Task { await store.previewApply() }
                }
                .disabled(!store.canEditDraft || store.busy)
            }

            HStack(spacing: 10) {
                Button("硬件写入与恢复记录…") {
                    close?()
                    store.showingDeviceOperations = true
                }

                Button("查看未同步硬件改动 (\(changeCount) 项)…") {
                    if let onOpenDiffReview {
                        onOpenDiffReview()
                    } else {
                        localShowingDiffReview = true
                    }
                }
                .disabled(store.browsingDeviceSnapshot || changeCount == 0)
            }

            HStack(spacing: 10) {
                if let showDiagnostics {
                    Button(showDiagnostics.wrappedValue ? "收起连接与诊断面板" : "展开连接与诊断面板") {
                        showDiagnostics.wrappedValue.toggle()
                        close?()
                    }
                } else {
                    Button("展开/收起连接与诊断面板") {
                        close?()
                    }
                }

                Button("导出故障诊断报告…") {
                    if let onOpenDiagnosticExport {
                        onOpenDiagnosticExport()
                    } else {
                        localShowingDiagnosticExport = true
                    }
                }
            }

            Spacer()
        }
        .padding(.vertical, 8)
    }
}
