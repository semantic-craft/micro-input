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
        let layerIdentity = "\(insp.selectedProfileId ?? "")-\(insp.inspectorLayerName)"
        let controlGroupIdentity: String = {
            if insp.selectedSurface.hasPrefix("Encoder") || (13...15).contains(slot) {
                return "\(layerIdentity)-encoder"
            }
            if insp.selectedSurface == "Joystick" || (16...19).contains(slot) {
                return "\(layerIdentity)-joystick"
            }
            if slot == 10 || slot == 11 {
                return "\(layerIdentity)-bottom-\(insp.bottomRowLayout)"
            }
            if slot < 6 {
                return "\(layerIdentity)-top6"
            }
            return "\(layerIdentity)-action"
        }()

        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Text(titleForSlot(slot: slot, isSplit: insp.bottomRowLayout == "split"))
                        .font(UITheme.heading)
                        .foregroundStyle(UITheme.textPrimary)
                    Spacer(minLength: 4)
                    Text("\(insp.selectedDisplayName) · \(insp.inspectorLayerName)")
                        .font(UITheme.micro)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(Capsule().fill(UITheme.subtleSurface))
                        .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
                        .foregroundStyle(UITheme.textPrimary)
                    Text(insp.isProtectedLayer ? "原生直通" : (isTaskRole ? "会话键" : "动作键"))
                        .font(UITheme.micro)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 3)
                        .background(Capsule().fill(UITheme.subtleSurface))
                        .foregroundStyle(UITheme.textSecondary)
                }
                Text(boundSummaryText(insp: insp, slot: slot))
                    .font(UITheme.bodyMedium)
                    .foregroundStyle(UITheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .id("inspector-header-\(layerIdentity)")

            Rectangle().fill(UITheme.hairlineBorder).frame(height: 1)

            if let lock = insp.protectedLock {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(lock.ownerClientName) · \(lock.lockBadgeText)")
                            .font(UITheme.bodySemibold)
                            .foregroundStyle(UITheme.textPrimary)
                        Text(lock.explanationText)
                            .font(UITheme.caption)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                    Spacer(minLength: 8)
                    Button(lock.unlockButtonTitle) {
                        store.dispatch("canvas", "UnlockBySwitchingOrCreatingCustomLayer")
                    }
                    .surfaceButton(.primary)
                }
                .padding(14)
                .background(UITheme.subtleSurface)
                Rectangle().fill(UITheme.hairlineBorder).frame(height: 1)
            }

            let edit = insp.editableInspector ?? EditableInspectorModel()
            let hasEditable = (insp.editableInspector != nil)
            let isJoystickSurface = (16...19).contains(slot) || insp.selectedSurface == "Joystick"
            let showActionControls = hasEditable && !isTaskRole && (!isJoystickSurface || edit.joystickMode == "logical_sectors")
            let showGlyphGrid = showActionControls && slot < 13
            let showTaskControls = hasEditable && isTaskRole

            ScrollViewReader { scrollProxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        Color.clear
                            .frame(height: 0)
                            .id("inspector-scroll-top")

                        if hasEditable || (13...19).contains(slot) || insp.selectedSurface.hasPrefix("Encoder") || insp.selectedSurface == "Joystick" {
                            slotSpecificControls(slot: slot, isTaskRole: isTaskRole, insp: insp)
                                .id(controlGroupIdentity)
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
                    .padding(16)
                }
                .onChange(of: layerIdentity) { _, _ in
                    scrollProxy.scrollTo("inspector-scroll-top", anchor: .top)
                }
            }
            .opacity(hasEditable || insp.isProtectedLayer ? 1 : 0)
            .allowsHitTesting(hasEditable || insp.isProtectedLayer)

            Rectangle().fill(UITheme.hairlineBorder).frame(height: 1)

            HStack(spacing: 8) {
                Button("更多动作…") {
                    onOpenFullActionLibrary()
                }
                .surfaceButton(.secondary)

                Button("绑定宏…") {
                    onOpenMacroLibrary()
                }
                .surfaceButton(.secondary)

                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
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
        case 17: return "四向摇杆（下）"
        case 18: return "四向摇杆（左）"
        case 19: return "四向摇杆（右）"
        default: return "按键 \(slot + 1)"
        }
    }

    private func boundSummaryText(insp: InspectorSliceModel, slot: Int) -> String {
        if let edit = insp.editableInspector {
            return edit.boundActionName
        }
        if insp.isProtectedLayer {
            let owner = insp.protectedLock?.ownerClientName ?? insp.selectedDisplayName
            let code: String = {
                switch slot {
                case 0..<13: return String(format: "KV_OAI_%02d / AG%02d", slot, slot)
                case 13: return "KV_OAI_ENC_CCW / AG13"
                case 14: return "KV_OAI_ENC_CW / AG14"
                case 15: return "KV_OAI_ENC_CLK / ENC_CLK"
                case 16: return "KV_OAI_JOY_N / AG15"
                case 17: return "KV_OAI_JOY_S / AG17"
                case 18: return "KV_OAI_JOY_W / AG16"
                case 19: return "KV_OAI_JOY_E / AG18"
                default: return "KV_OAI"
                }
            }()
            return "\(owner) 原生直通 (\(code))"
        }
        return "未绑定"
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
                        set: { store.dispatch("inspector", ["SetSlotRole": $0]) }
                    )
                ) {
                    Text("会话键（自动跟灯）").tag("TaskKey")
                    Text("普通动作键").tag("ActionKey")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            }
        } else if slot == 10 || slot == 11 {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(
                    "拆分为两个按键（1U + 1U）",
                    isOn: Binding(
                        get: { insp.bottomRowLayout == "split" },
                        set: { store.dispatch("canvas", ["SetBottomRowLayout": $0 ? "split" : "merged"]) }
                    )
                )
                .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))

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

                VStack(alignment: .leading, spacing: 6) {
                    if agentKind == .codex {
                        HStack(spacing: 8) {
                            Button("滚动对话预设") {
                                store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CodexScrollChatAndBottom"])
                            }
                            .surfaceButton(.secondary)
                            Button("调整推理档位") {
                                store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CodexReasoningEffortAndSpeed"])
                            }
                            .surfaceButton(.secondary)
                        }
                    } else if agentKind == .antigravity {
                        HStack(spacing: 8) {
                            Button("切活跃/蓝点会话") {
                                store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CycleWorkingAndBlueDot"])
                            }
                            .surfaceButton(.secondary)
                            Button("双滚轮预设") {
                                store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                            }
                            .surfaceButton(.secondary)
                        }
                    }
                    Button("摇杆鼠标 + 旋钮滚轮") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "MousePointerAndScroll"])
                    }
                    .surfaceButton(.secondary)
                }
            }
        } else if (16...19).contains(slot) || insp.selectedSurface == "Joystick" {
            let isEditable = insp.editableInspector != nil
            let currentMode = insp.editableInspector?.joystickMode ?? "logical_sectors"
            let modeSummary = insp.editableInspector?.joystickModeSummary ?? ""
            let mouseSupported = insp.editableInspector?.v070HardwareMouseSupported ?? true
            let isUnsupportedHardwareMode = !mouseSupported && (currentMode == "mouse" || currentMode == "scroll")

            VStack(alignment: .leading, spacing: 8) {
                Text("摇杆驱动模式")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Picker(
                    "摇杆驱动模式",
                    selection: Binding(
                        get: { currentMode },
                        set: { store.dispatch("inspector", ["SetJoystickMode": $0]) }
                    )
                ) {
                    Text("宿主逻辑扇区").tag("logical_sectors")
                    Text("硬件 MOUSE").tag("mouse")
                    Text("硬件 SCROLL").tag("scroll")
                    Text("保留来源").tag("source")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .disabled(!isEditable)
                .accessibilityIdentifier("inspector-joystick-mode-picker")

                if !modeSummary.isEmpty {
                    Text(modeSummary)
                        .font(UITheme.micro)
                        .foregroundStyle(isUnsupportedHardwareMode ? UITheme.textPrimary : UITheme.textSecondary)
                        .padding(isUnsupportedHardwareMode ? 8 : 0)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(
                            RoundedRectangle(cornerRadius: 8, style: .continuous)
                                .fill(isUnsupportedHardwareMode ? UITheme.cautionSoft : Color.clear)
                        )
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("inspector-joystick-mode-summary")
                }

                if currentMode == "logical_sectors" {
                    Text("四向摇杆方向")
                        .font(UITheme.captionMedium)
                        .foregroundStyle(UITheme.textSecondary)
                    Picker(
                        "摇杆方向",
                        selection: Binding(
                            get: { insp.selectedJoystickDirection },
                            set: { store.dispatch("canvas", ["SelectJoystickDirection": $0]) }
                        )
                    ) {
                        Text("上").tag("North")
                        Text("左").tag("West")
                        Text("下").tag("South")
                        Text("右").tag("East")
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()

                    HStack(spacing: 8) {
                        if agentKind == .antigravity {
                            Button("滚对话 + 左右切会话") {
                                store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                            }
                            .surfaceButton(.secondary)
                        }
                        Button("摇杆鼠标 + 旋钮滚轮") {
                            store.dispatch("canvas", ["ApplyKnobJoystickPreset": "MousePointerAndScroll"])
                        }
                        .surfaceButton(.secondary)
                    }
                }
            }
        }
    }

    private func customShortcutInspectorCard(edit: EditableInspectorModel) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("自定义快捷键映射")
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Spacer()
                Button("设为 Fn 听写键") {
                    store.dispatch("inspector", "BindAppleFnKey")
                }
                .surfaceButton(.secondary)

                Button("设为空键") {
                    store.dispatch("inspector", "UnassignSelectedSlot")
                }
                .surfaceButton(.ghost)
            }

            if edit.shortcutRecorder.isRecording {
                HStack(spacing: 8) {
                    Text("录制：")
                        .font(UITheme.captionMedium)
                        .foregroundStyle(UITheme.textPrimary)
                    ForEach([("r", "⌘R"), ("k", "⌘K"), ("l", "⌘L")], id: \.0) { key, label in
                        Button(label) {
                            store.dispatch("inspector", [
                                "CaptureShortcutChord": ["key": key, "modifiers": ["Command"]]
                            ])
                            store.dispatch("inspector", "CommitRecordedShortcut")
                        }
                        .surfaceButton(.secondary)
                    }
                    Spacer()
                    Button("取消") {
                        store.dispatch("inspector", "CancelShortcutRecording")
                    }
                    .surfaceButton(.ghost)
                }
                .padding(10)
                .background(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(UITheme.subtleSurface)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(UITheme.hairlineBorder, lineWidth: 1)
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
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(UITheme.subtleSurface)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .stroke(UITheme.hairlineBorder, lineWidth: 1)
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .focusEffectDisabled()
            }
        }
    }

    private func sessionSlotExplanationCard(slot: Int, ss: SessionStripSliceModel) -> some View {
        let sessionSlot = ss.sessionSlots.first(where: { $0.slotIndex == slot })
        return VStack(alignment: .leading, spacing: 10) {
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
                .surfaceButton(.primary)

                if sessionSlot?.isUnreadBlueDot == true {
                    Button("标为已读熄灯") {
                        store.dispatch("session_strip", "AcknowledgeAllCompleted")
                    }
                    .surfaceButton(.secondary)
                }
            }
        }
        .padding(14)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(UITheme.subtleSurface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(UITheme.hairlineBorder, lineWidth: 1)
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
                .frame(width: 128)
                .onSubmit {
                    if let first = filteredKeycaps.first {
                        store.dispatch("inspector", [
                            "ApplyKeycapIcon": [
                                "keycap": first.keycapId,
                                "bind_default_action": bindDefaultAction
                            ]
                        ])
                    }
                }
            }

            LazyVGrid(
                columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 6),
                spacing: 6
            ) {
                ForEach(filteredKeycaps) { item in
                    KeycapGlyphCellButton(store: store, item: item, bindDefaultAction: bindDefaultAction)
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
                "ApplyKeycapIcon": ["keycap": item.keycapId, "bind_default_action": bindDefaultAction]
            ])
        } label: {
            VStack(spacing: 4) {
                CodexKeycapGlyphView(keycap: cap, size: 17, color: UITheme.textPrimary)
                    .equatable()
                Text(item.shortLabel)
                    .font(UITheme.caption)
                    .foregroundStyle(item.isSelected ? UITheme.textPrimary : UITheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity)
            .frame(minHeight: 56)
            .background(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(item.isSelected ? UITheme.accentSoft : UITheme.subtleSurface)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .stroke(item.isSelected ? UITheme.textPrimary : UITheme.hairlineBorder, lineWidth: item.isSelected ? 1.5 : 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
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
            .onSubmit {
                if let first = filteredActions.first {
                    store.dispatch("inspector", ["AssignCatalogAction": first.actionId])
                }
            }

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
                        .fixedSize(horizontal: false, vertical: true)
                    Text(act.category)
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textSecondary)
                }
                Spacer()
                if isBound {
                    Text("已启用")
                        .font(UITheme.micro)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(UITheme.surface))
                        .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
                        .foregroundStyle(UITheme.textPrimary)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(isBound ? UITheme.accentSoft : UITheme.subtleSurface)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(isBound ? UITheme.textPrimary : UITheme.hairlineBorder, lineWidth: isBound ? 1.5 : 1)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
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
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("全部可用动作库 · \(selectedDisplayName)")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            if let edit {
                let categories = ["全部"] + edit.availableCategories.filter { $0 != "全部" }
                HStack(spacing: 10) {
                    TextField(
                        "搜索动作名称…",
                        text: Binding(
                            get: { edit.actionSearchQuery },
                            set: { store.dispatch("inspector", ["SetActionSearchQuery": $0]) }
                        )
                    )
                    .textFieldStyle(.roundedBorder)

                    SelectDropdown(
                        options: categories.map { SelectOption(value: $0, title: $0) },
                        selection: Binding(
                            get: { edit.actionCategoryFilter },
                            set: { store.dispatch("inspector", ["SetActionCategoryFilter": $0]) }
                        ),
                        minTriggerWidth: 132
                    )
                }

                ScrollView {
                    SettingsCard {
                        ForEach(Array(edit.filteredActions.enumerated()), id: \.element.id) { idx, act in
                            SettingsRow(
                                act.name,
                                subtitle: "\(act.category) · \(act.detail)",
                                showsDivider: idx + 1 < edit.filteredActions.count
                            ) {
                                Button(act.isCurrentlyBound ? "已绑定" : "绑定") {
                                    store.dispatch("inspector", ["AssignCatalogAction": act.actionId])
                                    dismiss()
                                }
                                .surfaceButton(act.isCurrentlyBound ? .ghost : .secondary)
                            }
                        }
                    }
                }
                .frame(height: 360)
            }
        }
        .padding(20)
        .frame(width: 620)
        .background(UITheme.pageBackground)
    }
}

struct ClassicMacroAndMultiLibrarySheet: View {
    let store: CoreStore
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let edit = store.inspector.editableInspector
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Text("键盘宏与四分支动作")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button("+ 新建宏…") {
                    dismiss()
                    store.dispatch("editor", ["OpenMacroEditor": ["macro_id": NSNull()]])
                }
                .surfaceButton(.secondary)
                Button("+ 新建四分支…") {
                    dismiss()
                    store.dispatch("editor", ["OpenMultiEditor": ["multi_id": NSNull()]])
                }
                .surfaceButton(.secondary)
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            if let edit {
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        VStack(alignment: .leading, spacing: 8) {
                            SettingsSectionHeader("键盘宏序列")
                            SettingsCard {
                                if edit.macros.isEmpty {
                                    SettingsRow("暂无已保存的键盘宏", subtitle: "点击右上角「新建宏」录制或编写按键序列")
                                } else {
                                    ForEach(Array(edit.macros.enumerated()), id: \.element.id) { idx, mac in
                                        SettingsRow(
                                            mac.name,
                                            subtitle: "\(mac.stepOrBranchSummary) · \(mac.referenceSummary)",
                                            showsDivider: idx + 1 < edit.macros.count
                                        ) {
                                            HStack(spacing: 6) {
                                                Button("编辑") {
                                                    dismiss()
                                                    store.dispatch("editor", ["OpenMacroEditor": ["macro_id": mac.id]])
                                                }
                                                .surfaceButton(.ghost)
                                                Button("绑定") {
                                                    store.dispatch("editor", ["MacroBindExisting": ["macro_id": mac.id]])
                                                    dismiss()
                                                }
                                                .surfaceButton(.secondary)
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        VStack(alignment: .leading, spacing: 8) {
                            SettingsSectionHeader("四分支复用动作 (Multi)")
                            SettingsCard {
                                if edit.multis.isEmpty {
                                    SettingsRow("暂无四分支复用动作", subtitle: "支持同一按键区分单击、双击、长按与单击后按住")
                                } else {
                                    ForEach(Array(edit.multis.enumerated()), id: \.element.id) { idx, mul in
                                        SettingsRow(
                                            mul.name,
                                            subtitle: "\(mul.stepOrBranchSummary) · \(mul.referenceSummary)",
                                            showsDivider: idx + 1 < edit.multis.count
                                        ) {
                                            HStack(spacing: 6) {
                                                Button("编辑") {
                                                    dismiss()
                                                    store.dispatch("editor", ["OpenMultiEditor": ["multi_id": mul.id]])
                                                }
                                                .surfaceButton(.ghost)
                                                Button("绑定") {
                                                    store.dispatch("editor", ["MultiBindExisting": ["multi_id": mul.id]])
                                                    dismiss()
                                                }
                                                .surfaceButton(.secondary)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                .frame(height: 360)
            }
        }
        .padding(20)
        .frame(width: 620)
        .background(UITheme.pageBackground)
    }
}
