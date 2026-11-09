import SwiftUI

struct CreatorMicroInlineInspectorView: View {
    let store: CoreStore
    let agentKind: AgentProfileKind
    var onOpenFullActionLibrary: () -> Void
    var onOpenMacroLibrary: () -> Void

    var body: some View {
        let _ = ViewRenderTelemetry.record("CreatorMicroInlineInspectorView")
        let insp = store.inspector
        let slot = insp.selectedLogicalSlot
        let isTaskRole = (slot < 6) && (insp.selectedSlotRole == "TaskKey") && agentKind.supportsLiveSessionSlots

        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(titleForSlot(slot: slot, isSplit: insp.bottomRowLayout == "split"))
                        .font(UITheme.heading)
                        .foregroundStyle(UITheme.textPrimary)
                    Spacer()
                    Text(isTaskRole ? "会话键" : "动作键")
                        .font(UITheme.micro)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(UITheme.subtleSurface))
                        .foregroundStyle(UITheme.textSecondary)
                }
                Text("当前绑定：\(insp.editableInspector?.boundActionName ?? "未绑定")")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.accent)
                    .lineLimit(1)
            }
            .padding(14)

            Rectangle().fill(UITheme.border).frame(height: 1)

            if let lock = insp.protectedLock {
                VStack(alignment: .leading, spacing: 8) {
                    Text(lock.lockBadgeText)
                        .font(UITheme.bodySemibold)
                        .foregroundStyle(UITheme.caution)
                    Text(lock.explanationText)
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)
                    Button(lock.unlockButtonTitle) {
                        store.dispatch("canvas", "UnlockBySwitchingOrCreatingCustomLayer")
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(UITheme.accent)
                    .controlSize(.small)
                }
                .padding(14)
                .background(UITheme.cautionSoft)
                Rectangle().fill(UITheme.border).frame(height: 1)
            }

            let edit = insp.editableInspector ?? EditableInspectorModel()
            let hasEditable = (insp.editableInspector != nil)
            let showActionControls = hasEditable && !isTaskRole
            let showGlyphGrid = showActionControls && slot < 13
            let showTaskControls = hasEditable && isTaskRole

            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if hasEditable {
                        slotSpecificControls(slot: slot, isTaskRole: isTaskRole, insp: insp)
                    }

                    if showTaskControls {
                        sessionSlotExplanationCard(slot: slot, ss: store.sessionStrip)
                    }

                    VStack(alignment: .leading, spacing: 14) {
                        customShortcutInspectorCard(edit: edit)
                        KeycapGlyphGridSectionView(
                            store: store,
                            isVisible: showGlyphGrid,
                            keycapSearchQuery: edit.keycapSearchQuery,
                            filteredKeycaps: edit.filteredKeycaps,
                            bindDefaultAction: edit.bindKeycapDefaultActionOnSelect
                        )
                        .equatable()
                        .frame(maxHeight: showGlyphGrid ? .infinity : 0)
                        .clipped()
                        .opacity(showGlyphGrid ? 1 : 0)

                        AgentActionCatalogSectionView(
                            store: store,
                            isVisible: showActionControls,
                            boundActionId: edit.boundActionId,
                            actionSearchQuery: edit.actionSearchQuery,
                            filteredActions: Array(edit.filteredActions.prefix(16))
                        )
                        .equatable()
                    }
                    .frame(maxHeight: showActionControls ? .infinity : 0)
                    .clipped()
                    .opacity(showActionControls ? 1 : 0)
                    .allowsHitTesting(showActionControls)
                }
                .padding(14)
            }
            .opacity(hasEditable ? 1 : 0)
            .allowsHitTesting(hasEditable)

            Rectangle().fill(UITheme.border).frame(height: 1)

            HStack(spacing: 8) {
                Button("更多动作…") {
                    onOpenFullActionLibrary()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

                Button("绑定宏…") {
                    onOpenMacroLibrary()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

                Spacer()
            }
            .padding(12)
            .background(UITheme.surface)
        }
    }

    private func titleForSlot(slot: Int, isSplit: Bool) -> String {
        switch slot {
        case 0: return "左上键（按键 01）"
        case 1: return "右上键（按键 02）"
        case 2..<10: return String(format: "按键 %02d", slot + 1)
        case 10: return isSplit ? "底行左键（开关 11）" : "底部宽键（2U，开关 11+12）"
        case 11: return isSplit ? "底行中键（开关 12）" : "底部宽键右开关"
        case 12: return "底行右键（开关 13）"
        case 13: return "旋钮左旋"
        case 14: return "旋钮右旋"
        case 15: return "旋钮按下"
        case 16: return "四向摇杆（上）"
        case 17: return "四向摇杆（左）"
        case 18: return "四向摇杆（下）"
        case 19: return "四向摇杆（右）"
        default: return "按键 \(slot + 1)"
        }
    }

    @ViewBuilder
    private func slotSpecificControls(slot: Int, isTaskRole: Bool, insp: InspectorSliceModel) -> some View {
        if slot < 6 && agentKind.supportsLiveSessionSlots {
            VStack(alignment: .leading, spacing: 6) {
                Text("按键用途")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Picker(
                    "按键用途",
                    selection: Binding(
                        get: { isTaskRole ? "TaskKey" : "ActionKey" },
                        set: { role in
                            store.dispatch("inspector", ["SetSlotRole": role])
                        }
                    )
                ) {
                    Text("会话键（自动跟灯）").tag("TaskKey")
                    Text("普通动作键").tag("ActionKey")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            }
        } else if slot == 10 || slot == 11 {
            VStack(alignment: .leading, spacing: 8) {
                Toggle(
                    "拆分为两个按键（1U + 1U）",
                    isOn: Binding(
                        get: { insp.bottomRowLayout == "split" },
                        set: { isSplit in
                            store.dispatch("canvas", ["SetBottomRowLayout": isSplit ? "split" : "merged"])
                        }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
                .font(UITheme.captionMedium)

                if insp.bottomRowLayout == "split" {
                    Picker(
                        "底行按键",
                        selection: Binding(
                            get: { slot },
                            set: { store.dispatch("canvas", ["SelectLogicalSlot": $0]) }
                        )
                    ) {
                        Text("左键（开关 11）").tag(10)
                        Text("中键（开关 12）").tag(11)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }
            }
        } else if (13...15).contains(slot) || insp.selectedSurface.hasPrefix("Encoder") {
            VStack(alignment: .leading, spacing: 8) {
                Text("无极旋钮动作方向")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Picker(
                    "旋钮方向",
                    selection: Binding(
                        get: { insp.selectedSurface },
                        set: { store.dispatch("canvas", ["SelectSurface": $0]) }
                    )
                ) {
                    Text("左旋").tag("EncoderCcw")
                    Text("右旋").tag("EncoderCw")
                    Text("按下").tag("EncoderPress")
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                HStack(spacing: 6) {
                    Button("切活跃/蓝点会话") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CycleWorkingAndBlueDot"])
                    }
                    .controlSize(.mini)
                    Button("双滚轮预设") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                    }
                    .controlSize(.mini)
                }
            }
        } else if (16...19).contains(slot) || insp.selectedSurface == "Joystick" {
            VStack(alignment: .leading, spacing: 8) {
                Text("四向摇杆方向")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Picker(
                    "摇杆方向",
                    selection: Binding(
                        get: { insp.selectedJoystickDirection },
                        set: {
                            store.dispatch("canvas", ["SelectSurface": "Joystick"])
                            store.dispatch("canvas", ["SelectJoystickDirection": $0])
                        }
                    )
                ) {
                    Text("上").tag("North")
                    Text("左").tag("West")
                    Text("下").tag("South")
                    Text("右").tag("East")
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                HStack(spacing: 6) {
                    Button("滚对话 + 左右切会话") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                    }
                    .controlSize(.mini)
                }
            }
        }
    }

    private func customShortcutInspectorCard(edit: EditableInspectorModel) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("自定义快捷键映射")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Spacer()
                Button("设为 🌐 苹果 Fn") {
                    store.dispatch("inspector", "BindAppleFnKey")
                }
                .buttonStyle(.bordered)
                .controlSize(.mini)

                Button("设为空键") {
                    store.dispatch("inspector", "UnassignSelectedSlot")
                }
                .buttonStyle(.bordered)
                .controlSize(.mini)
            }

            if edit.shortcutRecorder.isRecording {
                HStack(spacing: 6) {
                    Text("录制：")
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.accent)
                    ForEach([("r", "⌘R"), ("k", "⌘K"), ("l", "⌘L")], id: \.0) { key, label in
                        Button(label) {
                            store.dispatch("inspector", [
                                "CaptureShortcutChord": [
                                    "key": key,
                                    "modifiers": ["Command"]
                                ]
                            ])
                            store.dispatch("inspector", "CommitRecordedShortcut")
                        }
                        .controlSize(.mini)
                    }
                    Spacer()
                    Button("取消") {
                        store.dispatch("inspector", "CancelShortcutRecording")
                    }
                    .controlSize(.mini)
                }
                .padding(8)
                .background(
                    RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                        .fill(UITheme.accentSoft)
                )
            } else {
                Button {
                    store.dispatch("inspector", "StartShortcutRecording")
                } label: {
                    HStack {
                        Image(systemName: "keyboard")
                            .foregroundStyle(UITheme.textSecondary)
                        Text(
                            edit.shortcutRecorder.previewBadge.isEmpty
                                ? "点击录制键盘快捷键…"
                                : "已录制：\(edit.shortcutRecorder.previewBadge)"
                        )
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textPrimary)
                        Spacer()
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 7)
                    .background(
                        RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                            .fill(UITheme.subtleSurface)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                            .stroke(UITheme.border, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func sessionSlotExplanationCard(slot: Int, ss: SessionStripSliceModel) -> some View {
        let sessionSlot = ss.sessionSlots.first(where: { $0.slotIndex == slot })
        return VStack(alignment: .leading, spacing: 8) {
            Text("实时会话槽位（自动跟灯）")
                .font(UITheme.bodySemibold)
                .foregroundStyle(UITheme.textPrimary)
            Text("当前槽位：\(sessionSlot?.title.isEmpty == false ? sessionSlot!.title : "等待关联活跃会话")")
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)
            HStack(spacing: 8) {
                Button("立即跳转到此会话") {
                    store.dispatch("session_strip", ["ActivateSlot": slot])
                }
                .buttonStyle(.borderedProminent)
                .tint(UITheme.accent)
                .controlSize(.small)

                if sessionSlot?.isUnreadBlueDot == true {
                    Button("标为已读熄灯") {
                        store.dispatch("session_strip", "AcknowledgeAllCompleted")
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: UITheme.cardCornerRadius, style: .continuous)
                .fill(UITheme.subtleSurface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: UITheme.cardCornerRadius, style: .continuous)
                .stroke(UITheme.border, lineWidth: 1)
        )
    }
}

private struct KeycapGlyphGridSectionView: View, Equatable {
    let store: CoreStore
    let isVisible: Bool
    let keycapSearchQuery: String
    let filteredKeycaps: [KeycapCatalogItemModel]
    let bindDefaultAction: Bool

    static func == (lhs: KeycapGlyphGridSectionView, rhs: KeycapGlyphGridSectionView) -> Bool {
        if !lhs.isVisible && !rhs.isVisible {
            return true
        }
        return lhs.isVisible == rhs.isVisible
            && lhs.keycapSearchQuery == rhs.keycapSearchQuery
            && lhs.filteredKeycaps == rhs.filteredKeycaps
            && lhs.bindDefaultAction == rhs.bindDefaultAction
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("键帽丝印图标")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Spacer()
                TextField(
                    "搜索图标…",
                    text: Binding(
                        get: { keycapSearchQuery },
                        set: { store.dispatch("inspector", ["SetKeycapSearchQuery": $0]) }
                    )
                )
                .textFieldStyle(.roundedBorder)
                .controlSize(.small)
                .frame(width: 120)
            }

            LazyVGrid(
                columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 6),
                spacing: 6
            ) {
                ForEach(filteredKeycaps) { item in
                    KeycapGlyphCellButton(
                        store: store,
                        item: item,
                        bindDefaultAction: bindDefaultAction
                    )
                    .equatable()
                }
            }
        }
    }
}

private struct KeycapGlyphCellButton: View, Equatable {
    let store: CoreStore
    let item: KeycapCatalogItemModel
    let bindDefaultAction: Bool

    static func == (lhs: KeycapGlyphCellButton, rhs: KeycapGlyphCellButton) -> Bool {
        lhs.item == rhs.item && lhs.bindDefaultAction == rhs.bindDefaultAction
    }

    var body: some View {
        let cap = CodexKeycapId(rustVariant: item.keycapId)
        Button {
            store.dispatch("inspector", [
                "ApplyKeycapIcon": [
                    "keycap": item.keycapId,
                    "bind_default_action": bindDefaultAction
                ]
            ])
        } label: {
            VStack(spacing: 4) {
                CodexKeycapGlyphView(
                    keycap: cap,
                    size: 17,
                    color: item.isSelected ? UITheme.accent : UITheme.textPrimary
                )
                .equatable()
                Text(item.shortLabel)
                    .font(.system(size: 9, weight: .medium))
                    .foregroundStyle(item.isSelected ? UITheme.accent : UITheme.textSecondary)
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 48)
            .background(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(item.isSelected ? UITheme.accentSoft : UITheme.subtleSurface)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .stroke(item.isSelected ? UITheme.accent : UITheme.border, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
        .help(item.chineseHint)
    }
}

private struct AgentActionCatalogSectionView: View, Equatable {
    let store: CoreStore
    let isVisible: Bool
    let boundActionId: String?
    let actionSearchQuery: String
    let filteredActions: [ActionRowModel]

    static func == (lhs: AgentActionCatalogSectionView, rhs: AgentActionCatalogSectionView) -> Bool {
        if !lhs.isVisible && !rhs.isVisible {
            return true
        }
        return lhs.isVisible == rhs.isVisible
            && lhs.boundActionId == rhs.boundActionId
            && lhs.actionSearchQuery == rhs.actionSearchQuery
            && lhs.filteredActions == rhs.filteredActions
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("常用功能动作")
                .font(UITheme.captionMedium)
                .foregroundStyle(UITheme.textSecondary)

            TextField(
                "搜索动作名称或快捷键…",
                text: Binding(
                    get: { actionSearchQuery },
                    set: { store.dispatch("inspector", ["SetActionSearchQuery": $0]) }
                )
            )
            .textFieldStyle(.roundedBorder)
            .controlSize(.small)

            LazyVStack(spacing: 4) {
                ForEach(filteredActions) { act in
                    let isBound = act.isCurrentlyBound || (act.actionId == boundActionId)
                    ActionCatalogRowButton(store: store, act: act, isBound: isBound)
                        .equatable()
                }
            }
        }
    }
}

private struct ActionCatalogRowButton: View, Equatable {
    let store: CoreStore
    let act: ActionRowModel
    let isBound: Bool

    static func == (lhs: ActionCatalogRowButton, rhs: ActionCatalogRowButton) -> Bool {
        lhs.act == rhs.act && lhs.isBound == rhs.isBound
    }

    var body: some View {
        Button {
            store.dispatch("inspector", ["AssignCatalogAction": act.actionId])
        } label: {
            HStack(spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(act.name)
                        .font(isBound ? UITheme.bodySemibold : UITheme.body)
                        .foregroundStyle(UITheme.textPrimary)
                        .lineLimit(1)
                    Text(act.category)
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textSecondary)
                }
                Spacer()
                if isBound {
                    Text("已启用")
                        .font(UITheme.micro)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(UITheme.accentSoft))
                        .foregroundStyle(UITheme.accent)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(
                RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                    .fill(isBound ? UITheme.accentSoft.opacity(0.55) : UITheme.subtleSurface)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.controlCornerRadius, style: .continuous)
                    .stroke(isBound ? UITheme.accent : UITheme.border, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

struct ClassicMicroInputSettingsSheet: View {
    let store: CoreStore
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let h = store.header
        let s = store.sidebar
        let sm = store.statusMenu
        let f = store.footer
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("Micro Input 设置与诊断")
                    .font(UITheme.title)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button("完成") { dismiss() }
                    .buttonStyle(.borderedProminent)
                    .tint(UITheme.accent)
            }

            GroupBox("前台软件与按键控制") {
                VStack(alignment: .leading, spacing: 8) {
                    Toggle(
                        "启用小键盘动作分发控制",
                        isOn: Binding(
                            get: { h.hostExecutionEnabled },
                            set: { store.dispatch("header", ["SetExecutionEnabled": $0]) }
                        )
                    )
                    Toggle(
                        "切换前台软件时自动切换配套方案",
                        isOn: Binding(
                            get: { s.autoFollowEnabled },
                            set: { store.dispatch("sidebar", ["SetAutoFollow": $0]) }
                        )
                    )
                    Toggle(
                        "开启屏幕中央轮盘菜单",
                        isOn: Binding(
                            get: { sm.radialMenuEnabled },
                            set: { store.dispatch("status_menu", ["SetRadialMenuEnabled": $0]) }
                        )
                    )
                }
                .padding(8)
            }

            GroupBox("硬件同步与连接仿真") {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text(h.deviceSummary)
                            .font(UITheme.bodySemibold)
                        Spacer()
                        Button("检测键盘") {
                            store.dispatch("header", "ConnectKeyboard")
                        }
                        .controlSize(.small)
                    }
                    Text("文档修订 #\(h.documentRevision) · 宿主路由修订 #\(h.hostRevision) · \(f.statusMessage)")
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)

                    HStack(spacing: 8) {
                        Button("预览并备份当前配置…") {
                            dismiss()
                            store.dispatch("device", "OpenApplyPreparation")
                        }
                        .controlSize(.small)

                        Button("模拟设备重连三方比较…") {
                            dismiss()
                            store.dispatch("device", "OpenReconnectComparison")
                        }
                        .controlSize(.small)
                    }
                }
                .padding(8)
            }
        }
        .padding(20)
        .frame(width: 560)
        .background(UITheme.pageBackground)
    }
}

struct ClassicActionLibrarySheet: View {
    let store: CoreStore
    let rawProfileName: String
    let agentKind: AgentProfileKind
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let edit = store.inspector.editableInspector
        let selectedDisplayName = store.sidebar.selectedDisplayName
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("全部可用动作库 · \(selectedDisplayName)")
                    .font(UITheme.title)
                Spacer()
                Button("关闭") { dismiss() }
            }

            if let edit {
                HStack(spacing: 8) {
                    TextField(
                        "搜索动作名称…",
                        text: Binding(
                            get: { edit.actionSearchQuery },
                            set: { store.dispatch("inspector", ["SetActionSearchQuery": $0]) }
                        )
                    )
                    .textFieldStyle(.roundedBorder)

                    Picker(
                        "分类",
                        selection: Binding(
                            get: { edit.actionCategoryFilter },
                            set: { val in
                                store.dispatch("inspector", ["SetActionCategoryFilter": val])
                            }
                        )
                    ) {
                        ForEach(["全部"] + edit.availableCategories.filter { $0 != "全部" }, id: \.self) { cat in
                            Text(cat).tag(cat)
                        }
                    }
                    .frame(width: 180)
                }

                List(edit.filteredActions) { act in
                    HStack {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(act.name).font(UITheme.bodySemibold)
                            Text("\(act.category) · \(act.detail)")
                                .font(UITheme.caption)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button(act.isCurrentlyBound ? "已绑定" : "绑定到当前按键") {
                            store.dispatch("inspector", ["AssignCatalogAction": act.actionId])
                            dismiss()
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(UITheme.accent)
                        .controlSize(.small)
                    }
                }
                .frame(height: 340)
            }
        }
        .padding(20)
        .frame(width: 600)
    }
}

struct ClassicMacroAndMultiLibrarySheet: View {
    let store: CoreStore
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let edit = store.inspector.editableInspector
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("键盘宏与四分支动作 (Multi)")
                    .font(UITheme.title)
                Spacer()
                Button("+ 新建宏…") {
                    dismiss()
                    store.dispatch("editor", ["OpenMacroEditor": ["macro_id": NSNull()]])
                }
                Button("+ 新建四分支…") {
                    dismiss()
                    store.dispatch("editor", ["OpenMultiEditor": ["multi_id": NSNull()]])
                }
                Button("关闭") { dismiss() }
            }

            if let edit {
                List {
                    Section("键盘宏序列") {
                        ForEach(edit.macros) { mac in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text(mac.name).font(UITheme.bodySemibold)
                                    Text("\(mac.stepOrBranchSummary) · \(mac.referenceSummary)")
                                        .font(UITheme.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Button("编辑") {
                                    dismiss()
                                    store.dispatch("editor", ["OpenMacroEditor": ["macro_id": mac.id]])
                                }
                                .controlSize(.small)
                                Button("绑定") {
                                    store.dispatch("editor", ["MacroBindExisting": ["macro_id": mac.id]])
                                    dismiss()
                                }
                                .buttonStyle(.borderedProminent)
                                .tint(UITheme.accent)
                                .controlSize(.small)
                            }
                        }
                    }
                    Section("四分支复用动作 (Multi)") {
                        ForEach(edit.multis) { mul in
                            HStack {
                                VStack(alignment: .leading) {
                                    Text(mul.name).font(UITheme.bodySemibold)
                                    Text("\(mul.stepOrBranchSummary) · \(mul.referenceSummary)")
                                        .font(UITheme.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Button("编辑") {
                                    dismiss()
                                    store.dispatch("editor", ["OpenMultiEditor": ["multi_id": mul.id]])
                                }
                                .controlSize(.small)
                                Button("绑定") {
                                    store.dispatch("editor", ["MultiBindExisting": ["multi_id": mul.id]])
                                    dismiss()
                                }
                                .buttonStyle(.borderedProminent)
                                .tint(UITheme.accent)
                                .controlSize(.small)
                            }
                        }
                    }
                }
                .frame(height: 320)
            }
        }
        .padding(20)
        .frame(width: 600)
    }
}
