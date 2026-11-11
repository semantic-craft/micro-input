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
                Rectangle().fill(UITheme.hairlineBorder).frame(width: 1)
                VStack(spacing: 0) {
                    WorkbenchHeaderBarView(store: store)
                    Rectangle().fill(UITheme.hairlineBorder).frame(height: 1)
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
                                Text("应用方案")
                                    .font(UITheme.title)
                                    .foregroundStyle(UITheme.textPrimary)
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
                            .frame(maxWidth: UITheme.Layout.contentMaxWidth, alignment: .leading)
                            .frame(maxWidth: .infinity, alignment: .top)
                        }
                    } else {
                        WorkbenchSettingsPage(
                            store: store,
                            destination: destination,
                            onActions: { showFullActionLibrarySheet = true },
                            onMacros: { showMacroLibrarySheet = true }
                        )
                    }
                }
            }
            Rectangle().fill(UITheme.hairlineBorder).frame(height: 1)
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
                store.dispatch("sidebar", ["Profile": ["Create": ["name": name.isEmpty ? "自定义方案" : name]]])
            }
        }
        .alert("重命名当前方案", isPresented: $showRenameProfileDialog) {
            TextField("新方案名称", text: $renameProfileDraftName)
            Button("取消", role: .cancel) {}
            Button("保存") {
                let name = renameProfileDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                if let pid = store.sidebar.selectedProfileId, !name.isEmpty {
                    store.dispatch("sidebar", ["Profile": ["Rename": ["profile_id": pid, "name": name]]])
                }
            }
        }
        .alert("新建按键层", isPresented: $showNewLayerDialog) {
            TextField("层名称", text: $newLayerDraftName)
            Button("取消", role: .cancel) {}
            Button("创建") {
                let name = newLayerDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                store.dispatch("inspector", ["CreateCustomLayer": ["name": name.isEmpty ? "自定义层" : name]])
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

struct WorkbenchHeaderBarView: View {
    let store: CoreStore

    var body: some View {
        let _ = ViewRenderTelemetry.record("HeaderBar")
        let h = store.header
        let isReady = (h.readyKind == "Ready")

        HStack(spacing: 10) {
            HStack(spacing: 7) {
                Circle()
                    .fill(isReady ? UITheme.statusSuccess : UITheme.textMuted)
                    .frame(width: 7, height: 7)
                Text(h.readyText)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(isReady ? UITheme.textPrimary : UITheme.textSecondary)
                    .lineLimit(1)
            }
            .padding(.horizontal, 11)
            .padding(.vertical, 6)
            .background(Capsule().fill(UITheme.subtleSurface))
            .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))

            if h.showConnectButton {
                Button("连接键盘") {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .surfaceButton(.primary)
                .fixedSize()
            }

            Spacer(minLength: 8)

            Toggle(
                "按键控制",
                isOn: Binding(
                    get: { h.hostExecutionEnabled },
                    set: { store.dispatch("header", ["SetExecutionEnabled": $0]) }
                )
            )
            .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
            .fixedSize()

            HStack(spacing: 6) {
                Button {
                    store.dispatch("header", "Undo")
                } label: {
                    Image(systemName: "arrow.uturn.backward")
                }
                .surfaceButton(.icon)
                .disabled(!h.canUndo)
                .keyboardShortcut("z", modifiers: [.command])
                .help("撤销上一步修改 (⌘Z)")

                Button {
                    store.dispatch("header", "Redo")
                } label: {
                    Image(systemName: "arrow.uturn.forward")
                }
                .surfaceButton(.icon)
                .disabled(!h.canRedo)
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .help("重做上一步修改 (⇧⌘Z)")

                Button {
                    store.dispatch("overlay", "ToggleCheatSheet")
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "square.grid.2x2")
                            .font(.system(size: 12, weight: .medium))
                        Text(h.cheatSheetVisible ? "隐藏速查" : "按键速查")
                    }
                }
                .surfaceButton(.secondary)
                .fixedSize()
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
        VStack(alignment: .leading, spacing: 24) {
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader("软件方案") {
                    HStack(spacing: 8) {
                        Menu {
                            Button("新建配置…") { onPromptNewProfile() }
                            if let pid = s.selectedProfileId {
                                Button("重命名配置…") { onPromptRenameProfile() }
                                Button("复制配置") {
                                    store.dispatch("sidebar", [
                                        "Profile": ["Duplicate": ["profile_id": pid, "name": "\(s.selectedDisplayName) 副本"]]
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
                        } label: {
                            CapsuleMenuLabel("方案管理")
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .focusEffectDisabled()
                        .fixedSize()

                        Menu {
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
                        } label: {
                            CapsuleMenuLabel("导入与备份")
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .focusEffectDisabled()
                        .fixedSize()
                    }
                }

                SettingsCard {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(s.sidebarRows) { row in
                            SidebarProfileRowButton(store: store, row: row, onBrowse: onBrowseProfile)
                                .equatable()
                        }
                    }
                    .padding(8)
                }
            }

            if !s.boardLayers.layers.isEmpty {
                BoardLayersPanel(store: store, board: s.boardLayers)
            }

            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader("方案切换选项")
                SettingsCard {
                    SettingsRow("切换软件时自动切换方案", subtitle: "根据前台活跃窗口自动加载对应软件按键映射", showsDivider: true) {
                        Toggle(
                            "",
                            isOn: Binding(
                                get: { s.autoFollowEnabled },
                                set: { store.dispatch("sidebar", ["SetAutoFollow": $0]) }
                            )
                        )
                        .labelsHidden()
                        .toggleStyle(NeutralSwitchToggleStyle())
                    }
                    SettingsRow("显示其他自定义方案", subtitle: "在方案列表中一并展示原生保护方案与自定义草稿") {
                        Toggle(
                            "",
                            isOn: Binding(
                                get: { s.showOtherProfiles },
                                set: { store.dispatch("sidebar", ["SetShowOtherProfiles": $0]) }
                            )
                        )
                        .labelsHidden()
                        .toggleStyle(NeutralSwitchToggleStyle())
                    }
                }
            }
        }
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
                    HStack(spacing: 5) {
                        Circle()
                            .fill(UITheme.statusSuccess)
                            .frame(width: 6, height: 6)
                        Text("使用中")
                            .font(UITheme.micro)
                            .foregroundStyle(UITheme.textPrimary)
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 3)
                    .background(Capsule().fill(UITheme.subtleSurface))
                    .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(row.isSelected ? UITheme.subtleSurface : Color.clear)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(row.isSelected ? UITheme.strongBorder : Color.clear, lineWidth: 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
    }
}

struct BoardLayersPanel: View {
    let store: CoreStore
    let board: BoardLayersModel
    private static let unassignedValue = "__unassigned__"

    private func assign(_ layer: Int, _ profileId: String?) {
        store.dispatch("sidebar", ["SetBoardLayerProfile": ["layer": layer, "profile_id": profileId as Any? ?? NSNull()]])
    }

    private var profileOptions: [SelectOption<String>] {
        [.init(value: Self.unassignedValue, title: "未分配")]
            + board.profileChoices.map {
                .init(value: $0.profileId, title: DesktopAgent.displayName($0.displayName))
            }
    }

    var body: some View {
        let options = profileOptions
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("本地六层方案") {
                Button("默认") {
                    store.dispatch("sidebar", "ResetBoardLayerPlan")
                }
                .surfaceButton(.ghost)
                .help("Codex 原生、Antigravity、DeepSeek Harness，4 / 5 / 6 层留空")
            }

            SettingsCard {
                ForEach(Array(board.layers.enumerated()), id: \.element.id) { idx, row in
                    VStack(alignment: .leading, spacing: 0) {
                        HStack(spacing: 12) {
                            Button {
                                store.dispatch("sidebar", ["PreviewBoardLayer": row.layer])
                            } label: {
                                VStack(alignment: .leading, spacing: 3) {
                                    HStack(spacing: 6) {
                                        Text("第 \(row.layer) 层")
                                            .font(.system(size: 14, weight: row.isPreviewSelected ? .semibold : .medium))
                                            .foregroundStyle(UITheme.textPrimary)
                                        HStack(spacing: 4) {
                                            ForEach(Array(row.leds.enumerated()), id: \.offset) { led in
                                                Circle()
                                                    .fill(led.element ? UITheme.textPrimary : UITheme.strongBorder)
                                                    .frame(width: 4.5, height: 4.5)
                                            }
                                        }
                                    }
                                    if let issue = row.assignmentIssue {
                                        Text(issue)
                                            .font(.system(size: 12.5))
                                            .foregroundStyle(UITheme.textSecondary)
                                            .fixedSize(horizontal: false, vertical: true)
                                    } else if row.owner == "MicroInput" {
                                        Text("由 Micro Input 控制")
                                            .font(.system(size: 12.5))
                                            .foregroundStyle(UITheme.textSecondary)
                                    }
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .focusEffectDisabled()
                            .accessibilityIdentifier("board-preview-\(row.layer)")

                            SelectDropdown(
                                options: options,
                                selection: row.profileId ?? Self.unassignedValue,
                                overrideLabel: DesktopAgent.displayName(row.displayName),
                                minTriggerWidth: 148
                            ) { chosen in
                                assign(row.layer, chosen == Self.unassignedValue ? nil : chosen)
                            }
                            .accessibilityIdentifier("board-assignment-\(row.layer)")
                        }
                        .padding(.horizontal, 20)
                        .padding(.vertical, 12)
                        .background(row.isPreviewSelected ? UITheme.subtleSurface.opacity(0.65) : Color.clear)

                        Rectangle()
                            .fill(UITheme.rowDivider)
                            .frame(height: 1)
                            .padding(.horizontal, 20)
                    }
                }

                SettingsRow("USB 启动方案", subtitle: board.observedStatus, showsDivider: true) {
                    HStack(spacing: 8) {
                        Button("预览") {
                            store.dispatch("sidebar", "PreviewUsbStartup")
                        }
                        .surfaceButton(.ghost)

                        SelectDropdown(
                            options: options,
                            selection: board.usbStartupProfileId ?? Self.unassignedValue,
                            overrideLabel: DesktopAgent.displayName(board.usbStartupDisplayName),
                            minTriggerWidth: 148
                        ) { chosen in
                            let payload: Any = chosen == Self.unassignedValue ? NSNull() : chosen
                            store.dispatch("sidebar", ["SetUsbStartupProfile": payload])
                        }
                    }
                }

                SettingsRow(
                    "同步六层到小键盘",
                    subtitle: board.applyBlockReason.isEmpty ? "托管层需要 Karabiner 保持运行。" : board.applyBlockReason
                ) {
                    HStack(spacing: 8) {
                        if board.restoreAvailable {
                            Button("恢复原键盘配置") {
                                store.dispatch("sidebar", "RestoreBoardLayers")
                            }
                            .surfaceButton(.ghost)
                            .accessibilityIdentifier("board-restore")
                        }
                        Button("应用到键盘") {
                            store.dispatch("sidebar", "ProvisionBoardLayers")
                        }
                        .surfaceButton(.primary)
                        .disabled(!board.canApply)
                        .accessibilityIdentifier("board-apply")
                    }
                }
            }
        }
    }
}

enum WorkbenchDestination: String, CaseIterable, Identifiable {
    case keyboard = "Creator Micro"
    case lighting = "灯光"
    case device = "设备与连接"
    case profiles = "应用方案"
    case library = "动作库"
    case radial = "轮盘菜单"
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
            Text("Micro Input")
                .font(UITheme.bodySemibold)
                .foregroundStyle(UITheme.textPrimary)
                .padding(.horizontal, 12)
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
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(UITheme.textSecondary)
                .padding(.horizontal, 12)
                .padding(.bottom, 4)
            ForEach(destinations) { item in
                let isSelected = (selection == item)
                Button {
                    selection = item
                } label: {
                    HStack(spacing: 10) {
                        Image(systemName: item.symbol)
                            .font(.system(size: 13.5, weight: isSelected ? .medium : .regular))
                            .foregroundStyle(isSelected ? UITheme.textPrimary : UITheme.textSecondary)
                            .frame(width: 18)
                        Text(item.rawValue)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                    }
                    .font(isSelected ? UITheme.bodyMedium : UITheme.body)
                    .foregroundStyle(UITheme.textPrimary)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .fill(isSelected ? UITheme.surface : Color.clear)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 8, style: .continuous)
                            .stroke(isSelected ? UITheme.hairlineBorder : Color.clear, lineWidth: 1)
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .focusEffectDisabled()
                .accessibilityIdentifier("navigation-\(item.symbol)")
                .accessibilityAddTraits(isSelected ? .isSelected : [])
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
            VStack(alignment: .leading, spacing: 28) {
                Text(destination.rawValue)
                    .font(UITheme.title)
                    .foregroundStyle(UITheme.textPrimary)

                switch destination {
                case .lighting:
                    SimpleLightingControlCard(store: store)
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("按键层灯光")
                        SettingsCard {
                            SettingsRow(
                                "按键层双区 RGB 灯效",
                                subtitle: store.canvas.canAddLighting
                                    ? "独立配置所选按键层的键帽背光与底盘光效参数。"
                                    : (store.sidebar.selectedProfileId == nil
                                        ? "先选择已分配的层或应用方案。"
                                        : "当前层由原生应用控制灯光，请在键盘页面选择可编辑的层。")
                            ) {
                                Button("编辑层灯光") {
                                    store.dispatch("editor", "OpenLightingEditor")
                                }
                                .surfaceButton(.secondary)
                                .disabled(!store.canvas.canAddLighting)
                            }
                        }
                    }

                case .device:
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("连接")
                        SettingsCard {
                            SettingsRow("当前设备状态", subtitle: store.header.deviceSummary, showsDivider: true) {
                                Button("查找键盘") {
                                    store.dispatch("header", "ConnectKeyboard")
                                }
                                .surfaceButton(.primary)
                            }
                            SettingsRow("物理层观测", subtitle: store.sidebar.boardLayers.observedStatus)
                        }
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("六层方案")
                        SettingsCard {
                            SettingsRow(
                                "硬件六层同步",
                                subtitle: store.sidebar.boardLayers.applyBlockReason.isEmpty
                                    ? "在应用方案中分配六层，应用前自动备份原配置并读回核对。"
                                    : store.sidebar.boardLayers.applyBlockReason
                            ) {
                                HStack(spacing: 8) {
                                    if store.sidebar.boardLayers.restoreAvailable {
                                        Button("恢复原键盘配置") {
                                            store.dispatch("sidebar", "RestoreBoardLayers")
                                        }
                                        .surfaceButton(.ghost)
                                    }
                                    Button("应用到键盘") {
                                        store.dispatch("sidebar", "ProvisionBoardLayers")
                                    }
                                    .surfaceButton(.secondary)
                                    .disabled(!store.sidebar.boardLayers.canApply)
                                }
                            }
                        }
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("配置工具")
                        SettingsCard {
                            SettingsRow("配置应用预检与备份", subtitle: "只读核验待写入变更并生成本地回滚备份", showsDivider: true) {
                                Button("预览并备份配置") { store.dispatch("device", "OpenApplyPreparation") }
                                    .surfaceButton(.secondary)
                            }
                            SettingsRow("设备重连三方比较", subtitle: "对比基线、本地草稿与设备端配置的差异", showsDivider: true) {
                                Button("预览重连比较") { store.dispatch("device", "OpenReconnectComparison") }
                                    .surfaceButton(.secondary)
                            }
                            SettingsRow("运行诊断归档", subtitle: "导出脱敏连接与状态诊断快照") {
                                Button("导出诊断") { store.dispatch("device", "ExportDiagnostics") }
                                    .surfaceButton(.secondary)
                            }
                        }
                    }

                case .library:
                    let libraryDisabled = store.sidebar.selectedProfileId == nil || store.inspector.editableInspector == nil
                    let lockHint = store.sidebar.selectedProfileId == nil
                        ? "先选择已分配的层或应用方案。"
                        : "当前层受原生应用保护，选择可编辑层后可绑定动作。"
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("动作与快捷键")
                        SettingsCard {
                            SettingsRow(
                                "全部可用动作库",
                                subtitle: libraryDisabled
                                    ? lockHint
                                    : "当前编辑：\(store.sidebar.selectedDisplayName)。先在键盘页面选择按键，再绑定动作。"
                            ) {
                                Button("浏览动作") { onActions() }
                                    .surfaceButton(.primary)
                                    .disabled(libraryDisabled)
                            }
                        }
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("宏与多击键")
                        SettingsCard {
                            SettingsRow("键盘宏与四分支动作", subtitle: "组合按键、等待和文本，或为单击/双击/长按分配不同动作。") {
                                Button("管理宏与多击键") { onMacros() }
                                    .surfaceButton(.secondary)
                                    .disabled(libraryDisabled)
                            }
                        }
                    }

                case .radial:
                    let slots = store.statusMenu.radialSlots
                    let directions = ["右", "右下", "下", "左下", "左", "左上", "上", "右上"]
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("八向轮盘")
                        SettingsCard {
                            SettingsRow(
                                "启用轮盘菜单",
                                subtitle: store.statusMenu.radialConfirmMode == "spring_back"
                                    ? "移动摇杆选择，回到中心确认。"
                                    : "移动摇杆选择，按下旋钮确认；回到中心取消。",
                                showsDivider: !slots.isEmpty
                            ) {
                                Toggle(
                                    "",
                                    isOn: Binding(
                                        get: { store.statusMenu.radialMenuEnabled },
                                        set: { store.dispatch("status_menu", ["SetRadialMenuEnabled": $0]) }
                                    )
                                )
                                .labelsHidden()
                                .toggleStyle(NeutralSwitchToggleStyle())
                            }
                            ForEach(Array(slots.enumerated()), id: \.element.id) { idx, slot in
                                let dirName = directions[max(0, min(7, slot.id))]
                                SettingsRow(dirName, showsDivider: idx + 1 < slots.count) {
                                    Text(slot.label.isEmpty ? "未分配" : slot.label)
                                        .font(UITheme.captionMedium)
                                        .foregroundStyle(slot.label.isEmpty ? UITheme.textMuted : UITheme.textPrimary)
                                }
                            }
                        }
                    }

                case .general:
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("按键控制")
                        SettingsCard {
                            SettingsRow("允许键盘控制应用", subtitle: "开启后由 Micro Input 接管并路由物理按键动作", showsDivider: true) {
                                Toggle(
                                    "",
                                    isOn: Binding(
                                        get: { store.header.hostExecutionEnabled },
                                        set: { store.dispatch("header", ["SetExecutionEnabled": $0]) }
                                    )
                                )
                                .labelsHidden()
                                .toggleStyle(NeutralSwitchToggleStyle())
                            }
                            SettingsRow("跟随前台应用切换方案", subtitle: "切换活跃窗口时自动加载对应软件方案", showsDivider: true) {
                                Toggle(
                                    "",
                                    isOn: Binding(
                                        get: { store.sidebar.autoFollowEnabled },
                                        set: { store.dispatch("sidebar", ["SetAutoFollow": $0]) }
                                    )
                                )
                                .labelsHidden()
                                .toggleStyle(NeutralSwitchToggleStyle())
                            }
                            SettingsRow("按键速查浮层", subtitle: "查看当前方案 4×4 物理坐标按键绑定") {
                                Button("查看按键速查") {
                                    store.dispatch("overlay", "ToggleCheatSheet")
                                }
                                .surfaceButton(.secondary)
                            }
                        }
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader("本地草稿")
                        SettingsCard {
                            SettingsRow("本地配置修订", subtitle: "文档修订 #\(store.header.documentRevision)") {
                                HStack(spacing: 8) {
                                    Button("撤销") { store.dispatch("header", "Undo") }
                                        .surfaceButton(.secondary)
                                        .disabled(!store.header.canUndo)
                                    Button("重做") { store.dispatch("header", "Redo") }
                                        .surfaceButton(.secondary)
                                        .disabled(!store.header.canRedo)
                                }
                            }
                        }
                    }

                case .keyboard, .profiles:
                    EmptyView()
                }
            }
            .padding(UITheme.Layout.pagePadding)
            .frame(maxWidth: UITheme.Layout.contentMaxWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .top)
        }
    }
}

struct BoardLayerOverview: View {
    let store: CoreStore
    let board: BoardLayersModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            SettingsSectionHeader("键盘层")
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 104), spacing: 8)], spacing: 10) {
                ForEach(board.layers) { layer in
                    let isCurrent = layer.isPreviewSelected && layer.profileId == store.sidebar.selectedProfileId
                    Button {
                        store.dispatch("sidebar", ["PreviewBoardLayer": layer.layer])
                    } label: {
                        VStack(spacing: 8) {
                            HStack(spacing: 4) {
                                ForEach(Array(layer.leds.enumerated()), id: \.offset) { led in
                                    Circle()
                                        .fill(led.element ? UITheme.textPrimary : UITheme.strongBorder)
                                        .frame(width: 5, height: 5)
                                }
                            }
                            .accessibilityHidden(true)
                            Text("第 \(layer.layer) 层")
                                .font(UITheme.caption)
                                .foregroundStyle(UITheme.textSecondary)
                            Text(DesktopAgent.displayName(layer.displayName))
                                .font(UITheme.bodySemibold)
                                .foregroundStyle(UITheme.textPrimary)
                                .fixedSize(horizontal: false, vertical: true)
                                .multilineTextAlignment(.center)
                            if board.runtimeReady && board.observedLayer == layer.layer {
                                HStack(spacing: 4) {
                                    Circle().fill(UITheme.statusSuccess).frame(width: 5, height: 5)
                                    Text("使用中").font(UITheme.micro).foregroundStyle(UITheme.textPrimary)
                                }
                            }
                        }
                        .padding(12)
                        .frame(maxWidth: .infinity, minHeight: 104)
                        .background(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .fill(isCurrent ? UITheme.subtleSurface : UITheme.surface)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .stroke(isCurrent ? UITheme.textPrimary : UITheme.hairlineBorder, lineWidth: isCurrent ? 1.5 : 1)
                        )
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .focusEffectDisabled()
                    .accessibilityIdentifier("layer-overview-\(layer.layer)")
                    .accessibilityAddTraits(isCurrent ? .isSelected : [])
                }
            }
        }
    }
}
