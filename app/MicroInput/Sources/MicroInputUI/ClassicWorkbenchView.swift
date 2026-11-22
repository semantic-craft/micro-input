import AppKit
import SwiftUI
import UniformTypeIdentifiers

public struct ClassicWorkbenchView: View {
    let store: CoreStore
    @Bindable var navigation: WorkbenchNavigationModel
    let updater: SoftwareUpdateController
    let launchAtLogin: LaunchAtLoginController
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

    @MainActor
    public init(
        store: CoreStore,
        navigation: WorkbenchNavigationModel? = nil,
        updater: SoftwareUpdateController? = nil,
        launchAtLogin: LaunchAtLoginController? = nil
    ) {
        self.store = store
        self.navigation = navigation ?? WorkbenchNavigationModel()
        self.updater = updater ?? SoftwareUpdateController()
        self.launchAtLogin = launchAtLogin ?? LaunchAtLoginController()
    }

    public var body: some View {
        let destination = navigation.destination
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                WorkbenchNavigationView(
                    selection: $navigation.destination,
                    versionLabel: updater.identity.displayVersionLabel,
                    locale: store.locale
                )
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
                            onPromptNewLayer: { newLayerDraftName = ""; showNewLayerDialog = true },
                            onPromptRenameLayer: {
                                renameLayerDraftName = store.sidebar.inspectorLayerName
                                showRenameLayerDialog = true
                            }
                        )
                    } else if destination == .profiles {
                        ScrollView {
                            VStack(alignment: .leading, spacing: 24) {
                                Text(store.tr("应用方案")).font(UITheme.title).foregroundStyle(UITheme.textPrimary)
                                WorkbenchSidebarView(
                                    store: store,
                                    onBrowseProfile: { navigation.destination = .keyboard },
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
                            updater: updater,
                            launchAtLogin: launchAtLogin,
                            destination: destination,
                            onSelectDestination: { navigation.destination = $0 }
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
            ClassicActionLibrarySheet(store: store)
        }
        .sheet(isPresented: $showMacroLibrarySheet) {
            ClassicMacroAndMultiLibrarySheet(store: store)
        }
        .alert(store.tr("新建软件方案"), isPresented: $showNewProfileDialog) {
            TextField(store.tr("方案名称"), text: $newProfileDraftName)
            Button(store.tr("取消"), role: .cancel) {}
            Button(store.tr("创建")) {
                let name = newProfileDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                store.dispatch("sidebar", ["Profile": ["Create": ["name": name.isEmpty ? "新建方案" : name]]])
            }
        }
        .alert(store.tr("重命名当前方案"), isPresented: $showRenameProfileDialog) {
            TextField(store.tr("新方案名称"), text: $renameProfileDraftName)
            Button(store.tr("取消"), role: .cancel) {}
            Button(store.tr("保存")) {
                let name = renameProfileDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                if let pid = store.sidebar.selectedProfileId, !name.isEmpty {
                    store.dispatch("sidebar", ["Profile": ["Rename": ["profile_id": pid, "name": name]]])
                }
            }
        }
        .alert(store.tr("新建按键层"), isPresented: $showNewLayerDialog) {
            TextField(store.tr("层名称"), text: $newLayerDraftName)
            Button(store.tr("取消"), role: .cancel) {}
            Button(store.tr("创建")) {
                let name = newLayerDraftName.trimmingCharacters(in: .whitespacesAndNewlines)
                store.dispatch("inspector", ["CreateCustomLayer": ["name": name.isEmpty ? "自定义层" : name]])
            }
        }
        .alert(store.tr("重命名当前按键层"), isPresented: $showRenameLayerDialog) {
            TextField(store.tr("新层名称"), text: $renameLayerDraftName)
            Button(store.tr("取消"), role: .cancel) {}
            Button(store.tr("保存")) {
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
                Button(store.tr("连接键盘")) {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .surfaceButton(.primary)
                .fixedSize()
            }

            if let powerStatus = h.powerStatus {
                HardwarePowerStatusCapsule(badge: powerStatus)
            }
            if h.candidateDevices.count > 1 {
                CandidateDevicePickerPill(candidates: h.candidateDevices, locale: store.locale) {
                    store.selectHeaderCandidateDevice($0)
                }
            }

            Spacer(minLength: 8)

            Toggle(
                store.tr("按键映射"),
                isOn: Binding(
                    get: { h.hostExecutionEnabled },
                    set: { store.dispatch("header", ["SetExecutionEnabled": $0]) }
                )
            )
            .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true, allowsKeyboardFocus: false))
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
                .help(store.tr("撤销上一步修改 (⌘Z)"))

                Button {
                    store.dispatch("header", "Redo")
                } label: {
                    Image(systemName: "arrow.uturn.forward")
                }
                .surfaceButton(.icon)
                .disabled(!h.canRedo)
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .help(store.tr("重做上一步修改 (⇧⌘Z)"))

                Button {
                    store.dispatch("overlay", "ToggleCheatSheet")
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "square.grid.2x2")
                            .font(.system(size: 12, weight: .medium))
                        Text(h.cheatSheetVisible ? store.tr("隐藏速查") : store.tr("按键速查"))
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
        let s = store.sidebar
        let locale = store.locale
        VStack(alignment: .leading, spacing: 24) {
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader(store.tr("软件方案")) {
                    HStack(spacing: 8) {
                        Menu {
                            Button(store.tr("新建配置…")) { onPromptNewProfile() }
                            if let pid = s.selectedProfileId {
                                Button(store.tr("重命名配置…")) { onPromptRenameProfile() }
                                Button(store.tr("复制配置")) {
                                    store.dispatch("sidebar", [
                                        "Profile": ["Duplicate": ["profile_id": pid, "name": ""]]
                                    ])
                                }
                                Divider()
                                Button(store.tr("上移配置")) {
                                    store.dispatch("sidebar", ["Profile": ["MoveUp": ["profile_id": pid]]])
                                }
                                .disabled(!s.canMoveUp)
                                Button(store.tr("下移配置")) {
                                    store.dispatch("sidebar", ["Profile": ["MoveDown": ["profile_id": pid]]])
                                }
                                .disabled(!s.canMoveDown)
                                Divider()
                                Button(store.tr("删除配置…"), role: .destructive) {
                                    store.dispatch("sidebar", ["Profile": ["Delete": ["profile_id": pid]]])
                                }
                                .disabled(!s.canDelete)
                            }
                            Divider()
                            Button(store.tr("补齐内置软件方案")) {
                                store.dispatch("sidebar", ["Profile": "InstallCodingAgentPresets"])
                            }
                        } label: {
                            CapsuleMenuLabel(store.tr("方案管理"))
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .focusEffectDisabled()
                        .fixedSize()

                        Menu {
                            Button(store.tr("导入预设文件…")) {
                                let panel = NSOpenPanel()
                                panel.canChooseFiles = true
                                panel.canChooseDirectories = false
                                if panel.runModal() == .OK,
                                   let url = panel.url,
                                   let data = try? Data(contentsOf: url)
                                {
                                    store.stagePresetImport(data: data)
                                }
                            }
                            Divider()
                            Button(store.tr("导出整个本地文档…")) {
                                exportPreset(profileId: nil, layerId: nil, defaultName: "micro-input-document.json")
                            }
                            if let pid = s.selectedProfileId {
                                Button(store.tr("导出当前配置…")) {
                                    exportPreset(profileId: pid, layerId: nil, defaultName: "micro-input-profile.json")
                                }
                                if let lid = s.selectedLayerId {
                                    Button(store.tr("导出当前层…")) {
                                        exportPreset(profileId: pid, layerId: lid, defaultName: "micro-input-layer.json")
                                    }
                                }
                            }
                        } label: {
                            CapsuleMenuLabel(store.tr("导入与备份"))
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
                            SidebarProfileRowButton(store: store, row: row, locale: locale, onBrowse: onBrowseProfile)
                                .equatable()
                        }
                    }
                    .padding(8)
                }
            }

            if !s.boardLayers.layers.isEmpty {
                BoardLayersPanel(store: store, board: s.boardLayers)
            }

            let showHardwareLayerSync = store.v070Hardware.supported || store.v070Hardware.autoLayerSyncEnabled
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader(store.tr("方案与硬件层选项"))
                SettingsCard {
                    SettingsRow(
                        store.tr("显示其他自定义方案"),
                        subtitle: store.tr("在方案列表中一并展示原生保护方案与自定义草稿"),
                        showsDivider: showHardwareLayerSync
                    ) {
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
                    if showHardwareLayerSync {
                        SettingsRow(
                            store.tr("同步切换键盘硬件层"),
                            subtitle: store.tr("在桌面端切换或激活方案时，同步切换已连接键盘的硬件层"),
                            showsDivider: false
                        ) {
                            Toggle(
                                "",
                                isOn: Binding(
                                    get: { store.v070Hardware.autoLayerSyncEnabled },
                                    set: { store.setV070AutoLayerSync($0) }
                                )
                            )
                            .labelsHidden()
                            .toggleStyle(NeutralSwitchToggleStyle())
                            .disabled(store.v070Hardware.busy)
                            .accessibilityIdentifier("v070-auto-layer-sync")
                        }
                    }
                }
            }
        }
    }

    private func exportPreset(profileId: String?, layerId: String?, defaultName: String) {
        store.dispatch("sidebar", [
            "Backup": [
                "ExportPreset": [
                    "profile_id": profileId as Any? ?? NSNull(),
                    "layer_id": layerId as Any? ?? NSNull()
                ]
            ]
        ])
        guard let text = store.sidebar.lastExportedPreset else { return }
        let env = ProcessInfo.processInfo.environment
        guard env["XCTestConfigurationFilePath"] == nil, env["MI_SYNTHETIC_SCENARIO"] == nil else {
            return
        }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.json]
        panel.nameFieldStringValue = defaultName
        if panel.runModal() == .OK, let url = panel.url {
            try? text.write(to: url, atomically: true, encoding: .utf8)
        }
    }
}

private struct WorkbenchFooterBarView: View {
    let store: CoreStore

    var body: some View {
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

private struct SidebarProfileRowButton: View, @MainActor Equatable {
    let store: CoreStore
    let row: SidebarRowModel
    let locale: ResolvedLocale
    var onBrowse: () -> Void

    static func == (lhs: SidebarProfileRowButton, rhs: SidebarProfileRowButton) -> Bool {
        lhs.row == rhs.row && lhs.locale == rhs.locale
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
                        Text(L10n.tr("使用中", locale))
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
        let locale = store.locale
        return [.init(value: Self.unassignedValue, title: store.tr("未分配"))]
            + board.profileChoices.map {
                .init(value: $0.profileId, title: DesktopAgent.displayName($0.displayName, locale: locale))
            }
    }

    var body: some View {
        let locale = store.locale
        let options = profileOptions
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(store.tr("本地六层方案")) {
                Button(store.tr("默认")) {
                    store.dispatch("sidebar", "ResetBoardLayerPlan")
                }
                .surfaceButton(.ghost)
                .help(store.tr("第 1 层保留 OAI 映射，USB 独占后由 Micro Input 当前方案控制；第 2 / 3 层为 Antigravity / DeepSeek Harness，4 / 5 / 6 层留空"))
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
                                        Text(L10n.layerNumber(row.layer, locale))
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
                                        Text(store.tr("由 Micro Input 控制"))
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
                                overrideLabel: DesktopAgent.displayName(row.displayName, locale: locale),
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

                SettingsRow(store.tr("USB 启动方案"), subtitle: board.observedStatus, showsDivider: true) {
                    HStack(spacing: 8) {
                        Button(store.tr("预览")) {
                            store.dispatch("sidebar", "PreviewUsbStartup")
                        }
                        .surfaceButton(.ghost)

                        SelectDropdown(
                            options: options,
                            selection: board.usbStartupProfileId ?? Self.unassignedValue,
                            overrideLabel: DesktopAgent.displayName(board.usbStartupDisplayName, locale: locale),
                            minTriggerWidth: 148
                        ) { chosen in
                            let payload: Any = chosen == Self.unassignedValue ? NSNull() : chosen
                            store.dispatch("sidebar", ["SetUsbStartupProfile": payload])
                        }
                    }
                }

                SettingsRow(
                    store.tr("同步六层到小键盘"),
                    subtitle: board.applyBlockReason.isEmpty ? store.tr("托管层需要 Karabiner 保持运行。") : board.applyBlockReason
                ) {
                    HStack(spacing: 8) {
                        if board.restoreAvailable {
                            Button(store.tr("恢复原键盘配置")) {
                                store.dispatch("sidebar", "RestoreBoardLayers")
                            }
                            .surfaceButton(.ghost)
                            .accessibilityIdentifier("board-restore")
                        }
                        Button(store.tr("应用到键盘")) {
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

public enum WorkbenchDestination: String, CaseIterable, Identifiable {
    case keyboard = "Creator Micro"
    case lighting = "灯光"
    case device = "设备与连接"
    case profiles = "应用方案"
    case radial = "轮盘菜单"
    case general = "通用"

    public var id: String { rawValue }
    public var symbol: String {
        switch self {
        case .keyboard: return "keyboard"
        case .lighting: return "lightbulb"
        case .device: return "cable.connector"
        case .profiles: return "square.stack"
        case .radial: return "arrow.up.left.and.arrow.down.right"
        case .general: return "gearshape"
        }
    }
}

private struct WorkbenchNavigationView: View {
    @Binding var selection: WorkbenchDestination
    let versionLabel: String
    let locale: ResolvedLocale

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Micro Input")
                .font(UITheme.bodySemibold)
                .foregroundStyle(UITheme.textPrimary)
                .padding(.horizontal, 12)
            navigationGroup(L10n.tr("小键盘", locale), [.keyboard, .lighting, .device])
            navigationGroup(L10n.tr("自定义", locale), [.profiles, .radial])
            navigationGroup("Micro Input", [.general])
            Spacer(minLength: 0)
            Button { selection = .general } label: {
                HStack(spacing: 6) {
                    Text(L10n.tr("版本", locale)).font(UITheme.micro).foregroundStyle(UITheme.textMuted)
                    Spacer(minLength: 4)
                    Text(versionLabel).font(UITheme.monoBadge).foregroundStyle(UITheme.textSecondary)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 6)
                .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(selection == .general ? UITheme.surface : Color.clear))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .focusEffectDisabled()
            .accessibilityIdentifier("navigation-version-badge")
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
                        Text(L10n.destinationTitle(item.rawValue, locale))
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
    let updater: SoftwareUpdateController
    let launchAtLogin: LaunchAtLoginController
    let destination: WorkbenchDestination
    var onSelectDestination: (WorkbenchDestination) -> Void = { _ in }

    var body: some View {
        let locale = store.locale
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                Text(L10n.destinationTitle(destination.rawValue, locale))
                    .font(UITheme.title)
                    .foregroundStyle(UITheme.textPrimary)

                switch destination {
                case .lighting:
                    SimpleLightingControlCard(store: store)
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("按键层灯光"))
                        SettingsCard {
                            SettingsRow(
                                store.tr("按键层双区 RGB 灯效"),
                                subtitle: store.canvas.canAddLighting
                                    ? store.tr("独立配置所选按键层的键帽背光与底盘光效参数。")
                                    : (store.sidebar.selectedProfileId == nil
                                        ? store.tr("先选择已分配的层或应用方案。")
                                        : store.tr("当前层由原生应用控制灯光，请在键盘页面选择可编辑的层。"))
                            ) {
                                Button(store.tr("编辑层灯光")) {
                                    store.dispatch("editor", "OpenLightingEditor")
                                }
                                .surfaceButton(.secondary)
                                .disabled(!store.canvas.canAddLighting)
                            }
                        }
                    }

                case .device:
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("连接"))
                        SettingsCard {
                            SettingsRow(store.tr("当前设备状态"), subtitle: store.header.deviceSummary, showsDivider: true) {
                                Button(store.tr("查找键盘")) {
                                    store.dispatch("header", "ConnectKeyboard")
                                }
                                .surfaceButton(.primary)
                            }
                            SettingsRow(store.tr("物理层观测"), subtitle: store.sidebar.boardLayers.observedStatus)
                        }
                    }
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("硬件键帽布局"))
                        SettingsCard {
                            SettingsRow(store.tr("底行按键形态"), subtitle: store.tr("匹配物理拆分为两颗 1u 按键或合并为单颗 2u 宽键")) {
                                SelectDropdown(
                                    options: [
                                        .init(value: "split", title: store.tr("双 1u 分体")),
                                        .init(value: "merged", title: store.tr("单 2u 合并")),
                                    ],
                                    selection: Binding(
                                        get: { store.canvas.bottomRowLayout },
                                        set: { store.dispatch("canvas", ["SetBottomRowLayout": $0]) }
                                    ),
                                    minTriggerWidth: 128
                                )
                                .accessibilityIdentifier("device-bottom-row-layout")
                            }
                        }
                    }
                    V070HardwareSettingsSection(model: store.v070Hardware, locale: locale) {
                        store.dispatchV070Hardware($0)
                    }
                    FirmwareUpdateSettingsSection(
                        model: store.firmware,
                        locale: locale,
                        onConnectKeyboard: { store.dispatch("header", "ConnectKeyboard") }
                    ) { store.dispatch("firmware", $0) }
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("诊断与支持"))
                        SettingsCard {
                            SettingsRow(store.tr("运行诊断归档"), subtitle: store.tr("导出脱敏连接与状态诊断快照，并在 Finder 中显示")) {
                                Button(store.tr("导出诊断")) {
                                    store.dispatch("device", "ExportDiagnostics")
                                    if let path = store.footer.lastExportedDiagnosticsPath?
                                        .trimmingCharacters(in: .whitespacesAndNewlines),
                                       !path.isEmpty
                                    {
                                        let env = ProcessInfo.processInfo.environment
                                        if env["XCTestConfigurationFilePath"] == nil,
                                           env["MI_SYNTHETIC_SCENARIO"] == nil
                                        {
                                            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                                        }
                                    }
                                }
                                .surfaceButton(.secondary)
                            }
                        }
                    }

                case .radial:
                    let slots = store.statusMenu.radialSlots
                    let directions = L10n.radialDirections(locale)
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("八向轮盘"))
                        Text(store.tr("当前展示内置的固定八向快捷动作"))
                            .font(UITheme.caption)
                            .foregroundStyle(UITheme.textSecondary)
                        SettingsCard {
                            SettingsRow(
                                store.tr("启用轮盘菜单"),
                                subtitle: store.statusMenu.radialConfirmMode == "spring_back"
                                    ? store.tr("移动摇杆选择，回到中心确认。")
                                    : store.tr("移动摇杆选择，按下旋钮确认；回到中心取消。"),
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
                                    Text(slot.label.isEmpty ? store.tr("未分配") : slot.label)
                                        .font(UITheme.captionMedium)
                                        .foregroundStyle(slot.label.isEmpty ? UITheme.textMuted : UITheme.textPrimary)
                                }
                            }
                        }
                    }

                case .general:
                    StartupAndLanguageSettingsSection(store: store, launchAtLogin: launchAtLogin)
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("硬件与指针"))
                        SettingsCard {
                            SettingsRow(
                                store.tr("鼠标速度与摇杆死区"),
                                subtitle: store.tr("在「设备」页调整摇杆鼠标四档预设、无级速度滑块与死区")
                            ) {
                                Button(store.tr("前往调整")) {
                                    onSelectDestination(.device)
                                }
                                .surfaceButton(.secondary)
                                .accessibilityIdentifier("general-open-mouse-speed")
                            }
                        }
                    }
                    AboutAndUpdatesSettingsSection(
                        updater: updater,
                        engineVersion: store.header.engineVersion,
                        locale: locale
                    )
                    VStack(alignment: .leading, spacing: 10) {
                        SettingsSectionHeader(store.tr("系统权限"))
                        SettingsCard {
                            SettingsRow(
                                store.tr("辅助功能权限"),
                                subtitle: store.header.accessibilityTrusted
                                    ? store.tr("已授权，可正常向目标软件发送快捷键与指令")
                                    : store.tr("缺少 macOS 辅助功能权限，软件按键映射将被系统拦截")
                            ) {
                                if store.header.accessibilityTrusted {
                                    HStack(spacing: 6) {
                                        Circle()
                                            .fill(UITheme.statusSuccess)
                                            .frame(width: 7, height: 7)
                                        Text(store.tr("已就绪"))
                                            .font(UITheme.captionMedium)
                                            .foregroundStyle(UITheme.textPrimary)
                                    }
                                    .padding(.horizontal, 10)
                                    .padding(.vertical, 5)
                                    .background(Capsule().fill(UITheme.subtleSurface))
                                    .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
                                } else {
                                    Button(store.tr("打开辅助功能设置")) {
                                        store.dispatch("header", "OpenAccessibilitySettings")
                                    }
                                    .surfaceButton(.primary)
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
        let locale = store.locale
        VStack(alignment: .leading, spacing: 12) {
            SettingsSectionHeader(store.tr("键盘层"))
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
                            Text(L10n.layerNumber(layer.layer, locale))
                                .font(UITheme.caption)
                                .foregroundStyle(UITheme.textSecondary)
                            Text(DesktopAgent.displayName(layer.displayName, locale: locale))
                                .font(UITheme.bodySemibold)
                                .foregroundStyle(UITheme.textPrimary)
                                .fixedSize(horizontal: false, vertical: true)
                                .multilineTextAlignment(.center)
                            if board.runtimeReady && board.observedLayer == layer.layer {
                                HStack(spacing: 4) {
                                    Circle().fill(UITheme.statusSuccess).frame(width: 5, height: 5)
                                    Text(store.tr("使用中")).font(UITheme.micro).foregroundStyle(UITheme.textPrimary)
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
