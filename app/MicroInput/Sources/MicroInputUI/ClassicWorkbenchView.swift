import AppKit
import SwiftUI

public struct ClassicWorkbenchView: View {
    let store: CoreStore
    @State private var showSettingsSheet = false
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
            WorkbenchHeaderBarView(
                store: store,
                onOpenSettings: { showSettingsSheet = true }
            )
            Rectangle().fill(UITheme.border).frame(height: 1)
            HStack(spacing: 0) {
                WorkbenchSidebarView(
                    store: store,
                    onPromptNewProfile: {
                        newProfileDraftName = ""
                        showNewProfileDialog = true
                    },
                    onPromptRenameProfile: {
                        renameProfileDraftName = store.sidebar.selectedRawName
                        showRenameProfileDialog = true
                    }
                )
                .frame(width: 244)
                Rectangle().fill(UITheme.border).frame(width: 1)
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
            }
            Rectangle().fill(UITheme.border).frame(height: 1)
            WorkbenchFooterBarView(store: store)
        }
        .background(UITheme.pageBackground)
        .sheet(isPresented: $showSettingsSheet) {
            ClassicMicroInputSettingsSheet(store: store)
        }
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
    var onOpenSettings: () -> Void

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

                Button("设置…") {
                    onOpenSettings()
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
    var onPromptNewProfile: () -> Void
    var onPromptRenameProfile: () -> Void

    var body: some View {
        let _ = ViewRenderTelemetry.record("Sidebar")
        let s = store.sidebar
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("常用软件方案")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.top, 14)
            .padding(.bottom, 10)

            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(s.sidebarRows) { row in
                        SidebarProfileRowButton(store: store, row: row)
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
        .background(UITheme.subtleSurface)
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
                .lineLimit(1)
            Spacer()
            Text(f.footerLimitText)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textMuted)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(UITheme.surface)
    }
}

private struct SidebarProfileRowButton: View, Equatable {
    let store: CoreStore
    let row: SidebarRowModel

    static func == (lhs: SidebarProfileRowButton, rhs: SidebarProfileRowButton) -> Bool {
        lhs.row == rhs.row
    }

    var body: some View {
        Button {
            store.dispatch("sidebar", ["SelectProfile": row.profileId])
        } label: {
            HStack(spacing: 10) {
                AgentBadge(name: row.rawName, size: 22)
                Text(row.displayName)
                    .font(row.isSelected ? UITheme.bodySemibold : UITheme.body)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(1)
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

private struct BoardLayersPanel: View {
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
                            Text(row.displayName).font(UITheme.micro).lineLimit(1)
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
        .background(UITheme.subtleSurface)
    }
}
