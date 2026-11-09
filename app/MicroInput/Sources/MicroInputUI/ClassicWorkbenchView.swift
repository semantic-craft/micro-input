import AppKit
import SwiftUI

public struct ClassicWorkbenchView: View {
    @ObservedObject var store: CoreStore
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

    private var selectedRawName: String {
        store.model.sidebarRows.first(where: { $0.isSelected })?.rawName
            ?? store.model.selectedDisplayName
    }

    private var agentKind: AgentProfileKind {
        AgentProfileKind.from(profileName: selectedRawName)
    }

    public var body: some View {
        let m = store.model
        VStack(spacing: 0) {
            topStatusBar(m: m)
            Rectangle().fill(UITheme.border).frame(height: 1)
            HStack(spacing: 0) {
                leftSidebar(m: m)
                    .frame(width: 244)
                Rectangle().fill(UITheme.border).frame(width: 1)
                CreatorMicroStageView(
                    store: store,
                    model: m,
                    rawProfileName: selectedRawName,
                    agentKind: agentKind,
                    onOpenFullActionLibrary: { showFullActionLibrarySheet = true },
                    onOpenMacroLibrary: { showMacroLibrarySheet = true },
                    onPromptNewLayer: {
                        newLayerDraftName = ""
                        showNewLayerDialog = true
                    },
                    onPromptRenameLayer: {
                        renameLayerDraftName = m.inspectorLayerName
                        showRenameLayerDialog = true
                    }
                )
            }
            Rectangle().fill(UITheme.border).frame(height: 1)
            bottomFooterBar(m: m)
        }
        .background(UITheme.pageBackground)
        .sheet(isPresented: $showSettingsSheet) {
            ClassicMicroInputSettingsSheet(store: store)
        }
        .sheet(isPresented: $showFullActionLibrarySheet) {
            ClassicActionLibrarySheet(
                store: store,
                rawProfileName: selectedRawName,
                agentKind: agentKind
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
                if let pid = m.selectedProfileId, !name.isEmpty {
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

    private func topStatusBar(m: ProjectionModel) -> some View {
        let isReady = (m.readyKind == "Ready")
        let pillFg = isReady ? UITheme.accent : UITheme.caution
        let pillBg = isReady ? UITheme.accentSoft : UITheme.cautionSoft

        return HStack(spacing: 12) {
            HStack(spacing: 8) {
                Circle()
                    .fill(pillFg)
                    .frame(width: 7, height: 7)
                Text(m.readyText)
                    .font(UITheme.bodySemibold)
                    .foregroundStyle(pillFg)
                    .lineLimit(1)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(Capsule().fill(pillBg))

            if m.showConnectButton {
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
                    get: { m.hostExecutionEnabled },
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
                .disabled(!m.canUndo)
                .keyboardShortcut("z", modifiers: [.command])
                .help("撤销上一步修改 (⌘Z)")

                Button {
                    store.dispatch("header", "Redo")
                } label: {
                    Image(systemName: "arrow.uturn.forward")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .disabled(!m.canRedo)
                .keyboardShortcut("z", modifiers: [.command, .shift])
                .help("重做上一步修改 (⇧⌘Z)")

                Button(m.cheatSheetVisible ? "隐藏速查" : "按键速查") {
                    store.dispatch("overlay", "ToggleCheatSheet")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

                Button("设置…") {
                    showSettingsSheet = true
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(UITheme.surface)
    }

    private func leftSidebar(m: ProjectionModel) -> some View {
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
                    ForEach(m.sidebarRows) { row in
                        sidebarProfileRow(row: row)
                    }
                }
                .padding(.horizontal, 10)
                .padding(.bottom, 10)
            }

            Rectangle().fill(UITheme.border).frame(height: 1)

            VStack(alignment: .leading, spacing: 8) {
                Toggle(
                    "切换软件时自动切换方案",
                    isOn: Binding(
                        get: { m.autoFollowEnabled },
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
                        get: { m.showOtherProfiles },
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
                            newProfileDraftName = ""
                            showNewProfileDialog = true
                        }
                        if let pid = m.selectedProfileId {
                            Button("重命名配置…") {
                                renameProfileDraftName = selectedRawName
                                showRenameProfileDialog = true
                            }
                            Button("复制配置") {
                                store.dispatch("sidebar", [
                                    "Profile": [
                                        "Duplicate": [
                                            "profile_id": pid,
                                            "name": "\(m.selectedDisplayName) 副本"
                                        ]
                                    ]
                                ])
                            }
                            Divider()
                            Button("上移配置") {
                                store.dispatch("sidebar", ["Profile": ["MoveUp": ["profile_id": pid]]])
                            }
                            .disabled(!m.canMoveUp)
                            Button("下移配置") {
                                store.dispatch("sidebar", ["Profile": ["MoveDown": ["profile_id": pid]]])
                            }
                            .disabled(!m.canMoveDown)
                            Divider()
                            Button("删除配置…", role: .destructive) {
                                store.dispatch("sidebar", ["Profile": ["Delete": ["profile_id": pid]]])
                            }
                            .disabled(!m.canDelete)
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
                            if let text = store.model.lastExportedPreset {
                                NSPasteboard.general.clearContents()
                                NSPasteboard.general.setString(text, forType: .string)
                            }
                        }
                        if let pid = m.selectedProfileId {
                            Button("导出当前配置…") {
                                store.dispatch("sidebar", [
                                    "Backup": ["ExportPreset": ["profile_id": pid, "layer_id": NSNull()]]
                                ])
                                if let text = store.model.lastExportedPreset {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(text, forType: .string)
                                }
                            }
                            if let lid = m.selectedLayerId {
                                Button("导出当前层…") {
                                    store.dispatch("sidebar", [
                                        "Backup": ["ExportPreset": ["profile_id": pid, "layer_id": lid]]
                                    ])
                                    if let text = store.model.lastExportedPreset {
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

    private func sidebarProfileRow(row: SidebarRowModel) -> some View {
        SidebarProfileRowButton(store: store, row: row)
            .equatable()
    }

    private func bottomFooterBar(m: ProjectionModel) -> some View {
        HStack {
            Text(m.statusMessage)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)
                .lineLimit(1)
            Spacer()
            Text(m.footerLimitText)
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
