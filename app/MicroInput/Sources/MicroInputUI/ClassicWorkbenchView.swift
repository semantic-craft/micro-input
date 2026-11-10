import AppKit
import SwiftUI

public struct ClassicWorkbenchView: View {
    let store: CoreStore
    @State private var destination: WorkbenchDestination = .keyboard
    @State private var showFullActionLibrarySheet = false
    @State private var showMacroLibrarySheet = false
    @State private var showNewProfileDialog = false
    @State private var newProfileDraftName = ""
    @State private var showRenameProfileDialog = false
    @State private var renameProfileDraftName = ""
    @State private var showNewLayerDialog = false
    @State private var newLayerDraftName = ""
    @State private var showRenameLayerDialog = false
    @State private var renameLayerDraftName = ""

    public init(store: CoreStore) {
        self.store = store
    }

    public var body: some View {
        let _ = ViewRenderTelemetry.record("ClassicWorkbenchView")
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                WorkbenchNavigationView(selection: $destination)
                    .frame(width: UITheme.Layout.sidebarWidth)
                Rectangle().fill(UITheme.border).frame(width: 1)
                VStack(spacing: 0) {
                    WorkbenchHeaderBarView(store: store)
                    Divider()
                    if destination == .keyboard {
                        CreatorMicroStageView(
                            store: store,
                            onOpenFullActionLibrary: { showFullActionLibrarySheet = true },
                            onOpenMacroLibrary: { showMacroLibrarySheet = true },
                            onPromptNewLayer: {
                                newLayerDraftName = ""
                                showNewLayerDialog = true
                            },
                            onPromptRenameLayer: {
                                renameLayerDraftName = store.sidebar.inspectorLayerName
                                showRenameLayerDialog = true
                            }
                        )
                    } else if destination == .profiles {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 24) {
                                Text("应用方案").font(UITheme.title)
                                WorkbenchSidebarView(
                                    store: store,
                                    onBrowseProfile: { destination = .keyboard },
                                    onPromptNewProfile: { newProfileDraftName = ""; showNewProfileDialog = true },
                                    onPromptRenameProfile: {
                                        renameProfileDraftName = store.sidebar.selectedRawName
                                        showRenameProfileDialog = true
                                    }
                                )
                            }
                            .padding(UITheme.Layout.pagePadding)
                            .frame(maxWidth: UITheme.Layout.contentMaxWidth)
                            .frame(maxWidth: .infinity, alignment: .top)
                        }
                    } else {
                        WorkbenchSettingsPage(
                            store: store, destination: destination,
                            onActions: { showFullActionLibrarySheet = true },
                            onMacros: { showMacroLibrarySheet = true }
                        )
                    }
                }
            }
            Rectangle().fill(UITheme.border).frame(height: 1)
            WorkbenchFooterBarView(store: store)
        }
        .background(UITheme.pageBackground)
        .font(UITheme.body)
        .tint(UITheme.accent)
        .sheet(isPresented: $showFullActionLibrarySheet) {
            let rawName = store.sidebar.selectedRawName
            ClassicActionLibrarySheet(
                store: store,
                rawProfileName: rawName,
                agentKind: AgentProfileKind.from(profileName: rawName)
            )
        }
        .sheet(isPresented: $showMacroLibrarySheet) {
            ClassicMacroAndMultiLibrarySheet(store: store)
        }
        .alert("新建软件方案", isPresented: $showNewProfileDialog) {
            TextField("方案名称", text: $newProfileDraftName)
            Button("取消", role: .cancel) {}
            Button("创建") {
                let name = newProfileDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                store.dispatch(
                    "sidebar",
                    ["Profile": ["Create": ["name": name.isEmpty ? "自定义方案" : name]]]
                )
            }
        }
        .alert("重命名当前方案", isPresented: $showRenameProfileDialog) {
            TextField("新方案名称", text: $renameProfileDraftName)
            Button("取消", role: .cancel) {}
            Button("保存") {
                let name = renameProfileDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                if let pid = store.sidebar.selectedProfileId, !name.isEmpty {
                    store.dispatch(
                        "sidebar",
                        ["Profile": ["Rename": ["profile_id": pid, "name": name]]]
                    )
                }
            }
        }
        .alert("新建按键层", isPresented: $showNewLayerDialog) {
            TextField("层名称", text: $newLayerDraftName)
            Button("取消", role: .cancel) {}
            Button("创建") {
                let name = newLayerDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                store.dispatch(
                    "inspector",
                    ["CreateCustomLayer": ["name": name.isEmpty ? "自定义层" : name]]
                )
            }
        }
        .alert("重命名当前按键层", isPresented: $showRenameLayerDialog) {
            TextField("新层名称", text: $renameLayerDraftName)
            Button("取消", role: .cancel) {}
            Button("保存") {
                let name = renameLayerDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                if !name.isEmpty {
                    store.dispatch("inspector", ["RenameSelectedLayer": ["name": name]])
                }
            }
        }
    }
}

private struct WorkbenchHeaderBarView: View {
    let store: CoreStore

    var body: some View {
        let _ = ViewRenderTelemetry.record("HeaderBar")
        let h = store.header
        let isReady = (h.readyKind == "Ready")
        let pillFg = isReady ? UITheme.accent : UITheme.caution
        let pillBg = isReady ? UITheme.accentSoft : UITheme.cautionSoft

        HStack(spacing: 12) {
            HStack(spacing: 8) {
                Circle()
                    .fill(pillFg)
                    .frame(width: 7, height: 7)
                Text(h.readyText)
                    .font(UITheme.bodySemibold)
                    .foregroundStyle(pillFg)
                    .lineLimit(1)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(Capsule().fill(pillBg))

            if h.showConnectButton {
                Button("连接键盘") {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .buttonStyle(.borderedProminent)
                .tint(UITheme.accent)
                .controlSize(.small)
            }

            Spacer()

            Toggle(
                "按键控制",
                isOn: Binding(
                    get: { h.hostExecutionEnabled },
                    set: { store.dispatch("header", ["SetExecutionEnabled": $0]) }
                )
            )
            .toggleStyle(.switch)
            .controlSize(.small)
            .font(UITheme.bodyMedium)
            .foregroundStyle(UITheme.textPrimary)

            HStack(spacing: 6) {
                Button {
                    store.dispatch("header", "Undo")
                } label: {
                    Image(systemName: "arrow.uturn.backward")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(!h.canUndo)
                .keyboardShortcut("z", modifiers: [.command])
                .help("撤销上一步修改 (⌘Z)")

                Button {
                    store.dispatch("header", "Redo")
                } label: {
                    Image(systemName: "arrow.uturn.forward")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(!h.canRedo)
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .help("重做上一步修改 (⇧⌘Z)")

                Button(h.cheatSheetVisible ? "隐藏速查" : "按键速查") {
                    store.dispatch("overlay", "ToggleCheatSheet")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(UITheme.surface)
    }
}

private struct WorkbenchSidebarView: View {
    let store: CoreStore
    var onBrowseProfile: () -> Void
    var onPromptNewProfile: () -> Void
    var onPromptRenameProfile: () -> Void

    var body: some View {
        let _ = ViewRenderTelemetry.record("Sidebar")
        let s = store.sidebar
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("软件方案")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.top, 14)
            .padding(.bottom, 10)

            Group {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(s.sidebarRows) { row in
                        SidebarProfileRowButton(store: store, row: row, onBrowse: onBrowseProfile)
                            .equatable()
                    }
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 10)
            }

            if !s.boardLayers.layers.isEmpty {
                Rectangle().fill(UITheme.border).frame(height: 1)
                BoardLayersPanel(store: store, board: s.boardLayers)
            }

            Rectangle().fill(UITheme.border).frame(height: 1)

            VStack(alignment: .leading, spacing: 8) {
                Toggle(
                    "切换软件时自动切换方案",
                    isOn: Binding(
                        get: { s.autoFollowEnabled },
                        set: { store.dispatch("sidebar", ["SetAutoFollow": $0]) }
                    )
                )
                .toggleStyle(.checkbox)
                .controlSize(.small)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)

                Toggle(
                    "显示其他自定义方案",
                    isOn: Binding(
                        get: { s.showOtherProfiles },
                        set: { store.dispatch("sidebar", ["SetShowOtherProfiles": $0]) }
                    )
                )
                .toggleStyle(.checkbox)
                .controlSize(.small)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)

                HStack(spacing: 8) {
                    Menu("方案管理") {
                        Button("新建配置…") {
                            onPromptNewProfile()
                        }
                        if let pid = s.selectedProfileId {
                            Button("重命名配置…") {
                                onPromptRenameProfile()
                            }
                            Button("复制配置") {
                                store.dispatch("sidebar", [
                                    "Profile": [
                                        "Duplicate": [
                                            "profile_id": pid,
                                            "name": "\(s.selectedDisplayName) 副本"
                                        ]
                                    ]
                                ])
                            }
                            Divider()
                            Button("上移配置") {
                                store.dispatch("sidebar", ["Profile": ["MoveUp": ["profile_id": pid]]])
                            }
                            .disabled(!s.canMoveUp)
                            Button("下移配置") {
                                store.dispatch("sidebar", ["Profile": ["MoveDown": ["profile_id": pid]]])
                            }
                            .disabled(!s.canMoveDown)
                            Divider()
                            Button("删除配置…", role: .destructive) {
                                store.dispatch("sidebar", ["Profile": ["Delete": ["profile_id": pid]]])
                            }
                            .disabled(!s.canDelete)
                        }
                        Divider()
                        Button("补齐内置软件方案") {
                            store.dispatch("sidebar", ["Profile": "InstallCodingAgentPresets"])
                        }
                    }
                    .controlSize(.small)

                    Menu("导入与备份") {
                        Button("导入预设文件…") {
                            let panel = NSOpenPanel()
                            panel.canChooseFiles = true
                            panel.canChooseDirectories = false
                            if panel.runModal() == .OK,
                               let url = panel.url,
                               let text = try? String(contentsOf: url, encoding: .utf8)
                            {
                                store.dispatch("sidebar", ["Backup": ["ImportPreset": ["text": text]]])
                            }
                        }
                        Divider()
                        Button("导出整个本地文档…") {
                            store.dispatch("sidebar", [
                                "Backup": ["ExportPreset": ["profile_id": NSNull(), "layer_id": NSNull()]]
                            ])
                            if let text = store.sidebar.lastExportedPreset {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(text, forType: .string)
                            }
                        }
                        if let pid = s.selectedProfileId {
                            Button("导出当前配置…") {
                                store.dispatch("sidebar", [
                                    "Backup": ["ExportPreset": ["profile_id": pid, "layer_id": NSNull()]]
                                ])
                                if let text = store.sidebar.lastExportedPreset {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(text, forType: .string)
                                }
                            }
                            if let lid = s.selectedLayerId {
                                Button("导出当前层…") {
                                    store.dispatch("sidebar", [
                                        "Backup": ["ExportPreset": ["profile_id": pid, "layer_id": lid]]
                                    ])
                                    if let text = store.sidebar.lastExportedPreset {
                                        NSPasteboard.general.clearContents()
                                        NSPasteboard.general.setString(text, forType: .string)
                                    }
                                }
                            }
                        }
                    }
                    .controlSize(.small)
                }
            }
            .padding(12)
            .background(UITheme.surface)
        }
        .background(UITheme.surface)
    }
}

private struct WorkbenchFooterBarView: View {
    let store: CoreStore

    var body: some View {
        let _ = ViewRenderTelemetry.record("FooterBar")
        let f = store.footer
        HStack {
            Text(f.statusMessage)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(UITheme.surface)
    }
}

private struct SidebarProfileRowButton: View, Equatable {
    let store: CoreStore
    let row: SidebarRowModel
    var onBrowse: () -> Void

    static func == (lhs: SidebarProfileRowButton, rhs: SidebarProfileRowButton) -> Bool {
        lhs.row == rhs.row
    }

    var body: some View {
        Button {
            store.dispatch("sidebar", ["BrowseProfile": row.profileId])
            onBrowse()
        } label: {
            HStack(spacing: 10) {
                AgentBadge(name: row.rawName, size: 22)
                Text(row.displayName)
                    .font(row.isSelected ? UITheme.bodySemibold : UITheme.body)
                    .foregroundStyle(UITheme.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 4)
                if row.isActiveHost {
                    Text("使用中")
                        .font(UITheme.micro)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(UITheme.accentSoft))
                        .foregroundStyle(UITheme.accent)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                    .fill(row.isSelected ? UITheme.surface : Color.clear)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                    .stroke(row.isSelected ? UITheme.strongBorder : Color.clear, lineWidth: 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

struct BoardLayersPanel: View {
    let store: CoreStore
    let board: BoardLayersModel

    private func assign(_ layer: Int, _ profileId: String?) {
        store.dispatch("sidebar", ["SetBoardLayerProfile": ["layer": layer, "profile_id": profileId as Any? ?? NSNull()]])
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text("本地六层方案").font(UITheme.captionMedium)
                Spacer()
                Button("默认") { store.dispatch("sidebar", "ResetBoardLayerPlan") }
                    .buttonStyle(.borderless)
                    .font(UITheme.micro)
                    .help("Codex 原生、Antigravity、DeepSeek Harness、Cindy 托管，5 / 6 层留空")
            }
            ForEach(board.layers) { row in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Button {
                            store.dispatch("sidebar", ["PreviewBoardLayer": row.layer])
                        } label: {
                            HStack(spacing: 4) {
                                Text("第 \(row.layer) 层").font(UITheme.caption)
                                ForEach(Array(row.leds.enumerated()), id: \.offset) { led in
                                    Circle().fill(led.element ? UITheme.accent : UITheme.strongBorder).frame(width: 4, height: 4)
                                }
                                Spacer(minLength: 0)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("board-preview-\(row.layer)")
                        Menu {
                            Button("未分配") { assign(row.layer, nil) }
                            ForEach(board.profileChoices) { choice in
                                Button(choice.displayName) { assign(row.layer, choice.profileId) }
                            }
                        } label: {
                            Text(DesktopAgent.displayName(row.displayName)).font(UITheme.body).fixedSize(horizontal: false, vertical: true)
                        }
                        .menuStyle(.borderlessButton)
                        .accessibilityIdentifier("board-assignment-\(row.layer)")
                    }
                    if let issue = row.assignmentIssue {
                        Text(issue).font(UITheme.micro).foregroundStyle(UITheme.textMuted).fixedSize(horizontal: false, vertical: true)
                    } else if row.owner == "MicroInput" {
                        Text("由 Micro Input 控制").font(UITheme.micro).foregroundStyle(UITheme.textMuted)
                    }
                }
                .padding(6)
                .background(row.isPreviewSelected ? UITheme.surface : Color.clear)
            }
            Divider()
            HStack {
                Button("USB 启动方案") { store.dispatch("sidebar", "PreviewUsbStartup") }
                    .buttonStyle(.borderless)
                    .font(UITheme.micro)
                    .foregroundStyle(board.usbPreviewSelected ? UITheme.accent : UITheme.textSecondary)
                Menu {
                    Button("未分配") { store.dispatch("sidebar", ["SetUsbStartupProfile": NSNull()]) }
                    ForEach(board.profileChoices) { choice in
                        Button(choice.displayName) { store.dispatch("sidebar", ["SetUsbStartupProfile": choice.profileId]) }
                    }
                } label: { Text(board.usbStartupDisplayName).font(UITheme.micro).lineLimit(1) }
                .menuStyle(.borderlessButton)
            }
            Text(board.observedStatus).font(UITheme.micro).foregroundStyle(UITheme.textSecondary)
            Toggle(
                "Cindy 原生键盘集成已关闭",
                isOn: Binding(
                    get: { board.cindyNativePausedConfirmed },
                    set: { store.dispatch("sidebar", ["ConfirmCindyNativePaused": ["confirmed": $0]]) }
                )
            )
            .toggleStyle(.checkbox)
            .controlSize(.small)
            .font(UITheme.micro)
            .accessibilityIdentifier("board-cindy-native-paused")
            HStack {
                Button("应用到键盘") { store.dispatch("sidebar", "ProvisionBoardLayers") }
                    .disabled(!board.canApply)
                    .accessibilityIdentifier("board-apply")
                if board.restoreAvailable {
                    Button("恢复原键盘配置") { store.dispatch("sidebar", "RestoreBoardLayers") }
                        .accessibilityIdentifier("board-restore")
                }
            }
            .font(UITheme.micro)
            if !board.applyBlockReason.isEmpty {
                Text(board.applyBlockReason).font(UITheme.micro).foregroundStyle(UITheme.textMuted).fixedSize(horizontal: false, vertical: true)
            }
            if board.runtimeReady {
                Text(board.firstEventVerified ? "分层控制已就绪" : "配置已应用，等待首次按键")
                    .font(UITheme.micro).foregroundStyle(UITheme.textSecondary)
            } else if board.configurationObserved && board.keyboardGrabbed {
                Text("按键接管已就绪").font(UITheme.micro).foregroundStyle(UITheme.textSecondary)
            }
            Text("托管层需要 Karabiner 保持运行")
                .font(UITheme.micro).foregroundStyle(UITheme.textMuted)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(UITheme.surface)
    }
}

enum WorkbenchDestination: String, CaseIterable, Identifiable {
    case keyboard = "Creator Micro"
    case lighting = "灯光"
    case device = "设备与连接"
    case profiles = "应用方案"
    case library = "动作库"
    case radial = "径向菜单"
    case general = "通用"

    var id: String { rawValue }
    var symbol: String {
        switch self {
        case .keyboard: return "keyboard"
        case .lighting: return "lightbulb"
        case .device: return "cable.connector"
        case .profiles: return "square.stack"
        case .library: return "list.bullet.rectangle"
        case .radial: return "arrow.up.left.and.arrow.down.right"
        case .general: return "gearshape"
        }
    }
}

private struct WorkbenchNavigationView: View {
    @Binding var selection: WorkbenchDestination

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Micro Input").font(UITheme.bodySemibold).padding(.horizontal, 12)
            navigationGroup("小键盘", [.keyboard, .lighting, .device])
            navigationGroup("自定义", [.profiles, .library, .radial])
            navigationGroup("Micro Input", [.general])
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 24)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(UITheme.subtleSurface)
    }

    private func navigationGroup(_ title: String, _ destinations: [WorkbenchDestination]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                .padding(.horizontal, 12).padding(.bottom, 4)
            ForEach(destinations) { item in
                Button { selection = item } label: {
                    HStack(spacing: 10) {
                        Image(systemName: item.symbol).frame(width: 18)
                        Text(item.rawValue).fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                    }
                    .font(selection == item ? UITheme.bodySemibold : UITheme.body)
                    .foregroundStyle(UITheme.textPrimary)
                    .padding(.horizontal, 12).padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 8).fill(selection == item ? UITheme.mutedSurface : .clear))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("navigation-\(item.symbol)")
                .accessibilityAddTraits(selection == item ? .isSelected : [])
            }
        }
    }
}

private struct WorkbenchSettingsPage: View {
    let store: CoreStore
    let destination: WorkbenchDestination
    var onActions: () -> Void
    var onMacros: () -> Void

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 32) {
                Text(destination.rawValue).font(UITheme.title)
                switch destination {
                case .lighting:
                    SimpleLightingControlCard(store: store)
                    Divider()
                    Text("按键层灯光").font(UITheme.heading)
                    Text("编辑所选层的背光与底光。").foregroundStyle(UITheme.textSecondary)
                    Button("编辑层灯光") { store.dispatch("editor", "OpenLightingEditor") }
                        .disabled(!store.canvas.canAddLighting)
                    if !store.canvas.canAddLighting {
                        Text(store.sidebar.selectedProfileId == nil ? "先选择已分配的层或应用方案。" : "当前层由原生应用控制灯光。请在键盘页面选择可编辑的层。")
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                    }
                case .device:
                    settingsSection("连接") {
                        Text(store.header.deviceSummary).fixedSize(horizontal: false, vertical: true)
                        Button("查找键盘") { store.dispatch("header", "ConnectKeyboard") }
                        Text(store.sidebar.boardLayers.observedStatus)
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                    }
                    settingsSection("六层方案") {
                        Text("在应用方案中分配六层。应用前备份原配置，完成后读回核对。")
                        Text(store.sidebar.boardLayers.applyBlockReason)
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                        HStack {
                            Button("应用到键盘") { store.dispatch("sidebar", "ProvisionBoardLayers") }
                                .disabled(!store.sidebar.boardLayers.canApply)
                            if store.sidebar.boardLayers.restoreAvailable {
                                Button("恢复原键盘配置") { store.dispatch("sidebar", "RestoreBoardLayers") }
                            }
                        }
                    }
                    settingsSection("配置工具") {
                        Text("以下入口保留现有配置预览流程，结果以操作面板为准。")
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                        Button("预览并备份配置") { store.dispatch("device", "OpenApplyPreparation") }
                        Button("预览重连比较") { store.dispatch("device", "OpenReconnectComparison") }
                        Button("导出诊断") { store.dispatch("device", "ExportDiagnostics") }
                    }
                case .library:
                    settingsSection("动作与快捷键") {
                        Text("当前编辑：\(store.sidebar.selectedDisplayName)")
                        Text("先在键盘页面选择按键，再选择要绑定的动作。")
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                        Button("浏览动作") { onActions() }
                            .disabled(store.sidebar.selectedProfileId == nil || store.inspector.editableInspector == nil)
                    }
                    settingsSection("宏与多击键") {
                        Text("组合按键、等待和文本，或为不同点击方式分配动作。")
                        Button("管理宏与多击键") { onMacros() }
                            .disabled(store.sidebar.selectedProfileId == nil || store.inspector.editableInspector == nil)
                    }
                    if store.sidebar.selectedProfileId == nil || store.inspector.editableInspector == nil {
                        Text(store.sidebar.selectedProfileId == nil ? "先选择已分配的层或应用方案。" : "当前层受原生应用保护。选择可编辑层后可绑定动作。")
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                    }
                case .radial:
                    settingsSection("摇杆菜单") {
                        Toggle("启用径向菜单", isOn: Binding(
                            get: { store.statusMenu.radialMenuEnabled },
                            set: { store.dispatch("status_menu", ["SetRadialMenuEnabled": $0]) }
                        ))
                        Text(store.statusMenu.radialConfirmMode == "spring_back" ? "移动摇杆选择，回到中心确认。" : "移动摇杆选择，按下旋钮确认；回到中心取消。")
                            .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                        ForEach(store.statusMenu.radialSlots) { slot in
                            HStack {
                                Text(["右", "右下", "下", "左下", "左", "左上", "上", "右上"][max(0, min(7, slot.id))]).frame(width: 64, alignment: .leading)
                                Text(slot.label.isEmpty ? "未分配" : slot.label)
                                Spacer()
                            }
                            .padding(.vertical, 8)
                            Divider()
                        }
                    }
                case .general:
                    settingsSection("按键控制") {
                        Toggle("允许键盘控制应用", isOn: Binding(
                            get: { store.header.hostExecutionEnabled },
                            set: { store.dispatch("header", ["SetExecutionEnabled": $0]) }
                        ))
                        Toggle("跟随前台应用切换方案", isOn: Binding(
                            get: { store.sidebar.autoFollowEnabled },
                            set: { store.dispatch("sidebar", ["SetAutoFollow": $0]) }
                        ))
                        Button("查看按键速查") { store.dispatch("overlay", "ToggleCheatSheet") }
                    }
                    settingsSection("本地草稿") {
                        Text("文档修订 \(store.header.documentRevision)")
                        HStack {
                            Button("撤销") { store.dispatch("header", "Undo") }.disabled(!store.header.canUndo)
                            Button("重做") { store.dispatch("header", "Redo") }.disabled(!store.header.canRedo)
                        }
                    }
                case .keyboard, .profiles:
                    EmptyView()
                }
            }
            .font(UITheme.body)
            .foregroundStyle(UITheme.textPrimary)
            .toggleStyle(.switch)
            .padding(UITheme.Layout.pagePadding)
            .frame(maxWidth: UITheme.Layout.contentMaxWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .top)
        }
    }

    private func settingsSection<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(title).font(UITheme.heading)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct BoardLayerOverview: View {
    let store: CoreStore
    let board: BoardLayersModel

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("键盘层").font(UITheme.heading)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 104), spacing: 8)], spacing: 12) {
                ForEach(board.layers) { layer in
                    Button {
                        store.dispatch("sidebar", ["PreviewBoardLayer": layer.layer])
                    } label: {
                        VStack(spacing: 8) {
                            HStack(spacing: 4) {
                                ForEach(Array(layer.leds.enumerated()), id: \.offset) { led in
                                    Circle().fill(led.element ? UITheme.accent : UITheme.border)
                                        .frame(width: 5, height: 5)
                                }
                            }
                            .accessibilityHidden(true)
                            Text("第 \(layer.layer) 层").font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                            Text(DesktopAgent.displayName(layer.displayName)).font(UITheme.bodySemibold)
                                .fixedSize(horizontal: false, vertical: true)
                                .multilineTextAlignment(.center)
                            if board.runtimeReady && board.observedLayer == layer.layer {
                                Text("使用中").font(UITheme.caption).foregroundStyle(UITheme.accent)
                            }
                        }
                        .padding(12)
                        .frame(maxWidth: .infinity, minHeight: 104)
                        .background(RoundedRectangle(cornerRadius: 8).fill((layer.isPreviewSelected && layer.profileId == store.sidebar.selectedProfileId) ? UITheme.accentSoft : UITheme.surface))
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke((layer.isPreviewSelected && layer.profileId == store.sidebar.selectedProfileId) ? UITheme.accent : UITheme.border))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("layer-overview-\(layer.layer)")
                    .accessibilityAddTraits((layer.isPreviewSelected && layer.profileId == store.sidebar.selectedProfileId) ? .isSelected : [])
                }
            }
        }
    }
}
