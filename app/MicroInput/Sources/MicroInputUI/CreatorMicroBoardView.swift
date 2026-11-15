import AppKit
import SwiftUI

struct CreatorMicroStageView: View {
    let store: CoreStore
    var onOpenFullActionLibrary: () -> Void
    var onOpenMacroLibrary: () -> Void
    var onPromptNewLayer: () -> Void
    var onPromptRenameLayer: () -> Void

    var body: some View {
        let _ = ViewRenderTelemetry.record("CreatorMicroStageView")
        let s = store.sidebar
        let h = store.header
        let locale = store.locale
        let agentKind = AgentProfileKind.from(profileName: s.selectedRawName)
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 6) {
                    Text(store.tr("当前控制"))
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)
                    Text(h.hostExecutionEnabled ? DesktopAgent.displayName(store.statusMenu.activeProfileName, locale: locale) : store.tr("已暂停软件按键映射"))
                        .font(UITheme.title)
                        .foregroundStyle(UITheme.textPrimary)
                    Text(s.boardLayers.observedStatus)
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }
                Rectangle().fill(UITheme.rowDivider).frame(height: 1)
                BoardLayerOverview(store: store, board: s.boardLayers)
                if h.bannerKind != "Ready" { healthWarningBanner(h: h) }
                if s.selectedProfileId != nil {
                    HStack(alignment: .center, spacing: 10) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(DesktopAgent.displayName(s.selectedDisplayName, locale: locale))
                                .font(UITheme.sectionHeader)
                                .foregroundStyle(UITheme.textPrimary)
                            Text(locale == .en ? "Editing · \(s.inspectorLayerName)" : "正在编辑 · \(s.inspectorLayerName)")
                                .font(UITheme.caption)
                                .foregroundStyle(UITheme.textSecondary)
                        }
                        Spacer(minLength: 0)
                        Menu {
                            ForEach(s.layerTabs) { tab in
                                Button(tab.name) { store.dispatch("canvas", ["SelectLayer": tab.id]) }
                            }
                            Divider()
                            Button(store.tr("新建层")) { onPromptNewLayer() }
                            Button(store.tr("重命名当前层")) { onPromptRenameLayer() }.disabled(s.isProtectedLayer)
                            Button(store.tr("复制当前层")) {
                                store.dispatch("inspector", ["DuplicateSelectedLayer": ["name": ""]])
                            }
                            Button(store.tr("删除当前层"), role: .destructive) { store.dispatch("inspector", "DeleteSelectedLayer") }
                                .disabled(s.isProtectedLayer || s.layerTabs.count <= 1)
                        } label: {
                            CapsuleMenuLabel(store.tr("按键层"), icon: "square.3.layers.3d")
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .focusEffectDisabled()
                        .fixedSize()

                        if let profileId = s.selectedProfileId, profileId != s.activeHostProfileId {
                            Button(store.tr("启用此方案")) {
                                store.dispatch("sidebar", ["SelectProfile": profileId])
                            }
                            .surfaceButton(.primary)
                        }
                        if s.selectedProfileId != nil {
                            let isLockedToSelected = s.isProfileLocked && s.lockedProfileId == s.selectedProfileId
                            Button(isLockedToSelected ? store.tr("已锁定（点击解锁）") : store.tr("锁定此方案")) {
                                store.dispatch("sidebar", ["SetProfileLocked": !isLockedToSelected])
                            }
                            .surfaceButton(isLockedToSelected ? .primary : .secondary)
                        }
                        if let agent = DesktopAgent.named(s.selectedRawName), !agent.isGeneralFallback {
                            Button(store.tr("打开应用")) {
                                if let url = DesktopAgent.cachedAppURL(for: agent.bundleID) {
                                    NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
                                }
                            }
                            .surfaceButton(.secondary)
                        }
                    }
                    CreatorMicroBoardView(store: store, agentKind: agentKind)
                        .frame(maxWidth: .infinity)
                        .frame(height: 420)
                        .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 16, style: .continuous)
                                .stroke(UITheme.hairlineBorder, lineWidth: 1)
                        )
                    CreatorMicroSessionListView(store: store, agentKind: agentKind)
                    CreatorMicroInlineInspectorView(
                        store: store,
                        agentKind: agentKind,
                        onOpenFullActionLibrary: onOpenFullActionLibrary,
                        onOpenMacroLibrary: onOpenMacroLibrary
                    )
                    .frame(minHeight: 400, idealHeight: 540, maxHeight: 540)
                    .background(UITheme.surface, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .stroke(UITheme.hairlineBorder, lineWidth: 1)
                    )
                } else {
                    SettingsCard {
                        SettingsRow(store.tr("这一层尚未分配应用"), subtitle: store.tr("在应用方案中分配后，即可查看与编辑对应按键。"))
                    }
                }
            }
            .padding(UITheme.Layout.pagePadding)
            .frame(maxWidth: UITheme.Layout.contentMaxWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .top)
        }
        .background(UITheme.pageBackground)
    }

    private func healthWarningBanner(h: HeaderSliceModel) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "exclamationmark.circle")
                .foregroundStyle(UITheme.textSecondary)
            Text(h.bannerText)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textPrimary)
                .fixedSize(horizontal: false, vertical: true)
            Spacer()
            if !h.accessibilityTrusted {
                Button(store.tr("授权辅助功能")) {
                    store.dispatch("header", "OpenAccessibilitySettings")
                }
                .surfaceButton(.primary)
            }
            if !h.deviceConnected {
                Button(store.tr("重新检测键盘")) {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .surfaceButton(.primary)
            }
            if !h.hostExecutionEnabled {
                Button(store.tr("开启按键映射")) {
                    store.dispatch("header", ["SetExecutionEnabled": true])
                }
                .surfaceButton(.secondary)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(UITheme.surface, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(UITheme.hairlineBorder, lineWidth: 1)
        )
    }
}

struct CreatorMicroBoardView: View {
    let store: CoreStore
    let agentKind: AgentProfileKind
    @AppStorage("mi.jumpDirectlyOnGlowingSlotTap") private var jumpDirectlyOnGlowingSlotTap = true

    private let cellSize: CGFloat = 68
    private let gridSpacing: CGFloat = 8

    var body: some View {
        let _ = ViewRenderTelemetry.record("CreatorMicroBoardView")
        let c = store.canvas
        let ss = store.sessionStrip
        VStack(spacing: 14) {
            Spacer(minLength: 8)
            physicalBoardChassis(c: c, ss: ss)
            compactBoardToolbar(c: c, ss: ss)
            Spacer(minLength: 8)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    private func physicalBoardChassis(c: CanvasSliceModel, ss: SessionStripSliceModel) -> some View {
        VStack(spacing: gridSpacing) {
            HStack(spacing: gridSpacing) {
                rotaryEncoderCell(c: c)
                programmableKeyCell(slot: 0, c: c, ss: ss)
                programmableKeyCell(slot: 1, c: c, ss: ss)
                analogJoystickCell(c: c)
            }
            HStack(spacing: gridSpacing) {
                ForEach(2..<6, id: \.self) { slot in
                    programmableKeyCell(slot: slot, c: c, ss: ss)
                }
            }
            HStack(spacing: gridSpacing) {
                ForEach(6..<10, id: \.self) { slot in
                    programmableKeyCell(slot: slot, c: c, ss: ss)
                }
            }
            HStack(spacing: gridSpacing) {
                statusIndicatorClusterCell(c: c)
                if c.bottomRowLayout == "split" {
                    programmableKeyCell(slot: 10, c: c, ss: ss)
                    programmableKeyCell(slot: 11, c: c, ss: ss)
                } else {
                    wideProgrammableKeyCell(c: c, ss: ss)
                }
                programmableKeyCell(slot: 12, c: c, ss: ss)
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 20, style: .continuous).fill(UITheme.surface))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).stroke(UITheme.strongBorder, lineWidth: 1))
    }

    private func rotaryEncoderCell(c: CanvasSliceModel) -> some View {
        let locale = store.locale
        let isSelected = c.selectedSurface.hasPrefix("Encoder") || (13...15).contains(c.selectedLogicalSlot)
        let hasHold = !c.encoderHoldLabel.isEmpty && c.encoderHoldLabel != "未绑定" && c.encoderHoldLabel != "Unbound"
        let holdTip = hasHold ? (locale == .en ? " · Hold: \(c.encoderHoldLabel)" : " · 长按：\(c.encoderHoldLabel)") : ""
        let badge = store.tr(c.selectedSurface == "EncoderCcw" ? "左旋" : (c.selectedSurface == "EncoderCw" ? "右旋" : (c.selectedSurface == "EncoderHold" ? "长按" : "短按")))
        return Button {
            let next: String
            switch c.selectedSurface {
            case "EncoderCcw": next = "EncoderCw"
            case "EncoderCw": next = "EncoderPress"
            case "EncoderPress": next = "EncoderHold"
            default: next = "EncoderCcw"
            }
            store.dispatch("canvas", ["SelectSurface": next])
        } label: {
            ZStack {
                Circle()
                    .fill(
                        LinearGradient(
                            colors: [Color(red: 0.25, green: 0.25, blue: 0.27), Color(red: 0.14, green: 0.14, blue: 0.15)],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .frame(width: cellSize, height: cellSize)
                    .overlay(Circle().stroke(isSelected ? UITheme.accent : Color.black.opacity(0.35), lineWidth: isSelected ? 2.5 : 1))
                VStack(spacing: 0) {
                    Capsule().fill(Color.white.opacity(0.92)).frame(width: 2.2, height: 12).padding(.top, 7)
                    Spacer()
                }
                .frame(width: cellSize, height: cellSize)
                .rotationEffect(.degrees(c.selectedSurface == "EncoderCcw" ? -32 : (c.selectedSurface == "EncoderCw" ? 32 : 0)))

                if isSelected {
                    Text(badge).font(UITheme.micro).foregroundStyle(Color.white.opacity(0.92)).padding(.top, 14)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .help(locale == .en
            ? "Rotary knob (click to switch CCW / CW / Press / Hold\(holdTip))"
            : "无极金属旋钮（点击切换左旋 / 右旋 / 短按 / 长按配置\(holdTip)）")
    }

    private func analogJoystickCell(c: CanvasSliceModel) -> some View {
        let locale = store.locale
        let isSelected = c.selectedSurface == "Joystick" || c.joystickIsSelected
        let isHwMouse = c.joystickMode == "mouse"
        let isHwScroll = c.joystickMode == "scroll"
        let isHwMode = isHwMouse || isHwScroll
        let activeDir = c.selectedJoystickDirection
        let joyOffset: CGSize = {
            guard isSelected, !isHwMode else { return .zero }
            switch activeDir {
            case "North": return CGSize(width: 0, height: -4)
            case "West": return CGSize(width: -4, height: 0)
            case "South": return CGSize(width: 0, height: 4)
            case "East": return CGSize(width: 4, height: 0)
            default: return .zero
            }
        }()
        let capLabel: String = {
            if isHwMouse { return store.tr("鼠标") }
            if isHwScroll { return store.tr("滚轮") }
            switch activeDir {
            case "West": return "←"
            case "South": return "↓"
            case "East": return "→"
            default: return "↑"
            }
        }()
        return Button {
            if isHwMode {
                store.dispatch("canvas", ["SelectSurface": "Joystick"])
            } else if !isSelected {
                store.dispatch("canvas", ["SelectJoystickDirection": activeDir.isEmpty ? "North" : activeDir])
            } else {
                let dirs = ["North", "West", "South", "East"]
                let nextDir = dirs.firstIndex(of: activeDir).map { dirs[($0 + 1) % dirs.count] } ?? "North"
                store.dispatch("canvas", ["SelectJoystickDirection": nextDir])
            }
        } label: {
            ZStack {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(UITheme.mutedSurface)
                    .frame(width: cellSize, height: cellSize)
                    .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(isSelected ? UITheme.accent : UITheme.hairlineBorder, lineWidth: isSelected ? 2.5 : 1))
                ZStack {
                    Circle()
                        .fill(
                            LinearGradient(
                                colors: [Color(red: 0.24, green: 0.24, blue: 0.26), Color(red: 0.13, green: 0.13, blue: 0.15)],
                                startPoint: .topLeading,
                                endPoint: .bottomTrailing
                            )
                        )
                        .frame(width: 42, height: 42)
                        .overlay(Circle().stroke(Color.black.opacity(0.35), lineWidth: 1))
                    if isSelected || isHwMode {
                        Text(capLabel).font(isHwMode ? UITheme.micro : UITheme.captionMedium).foregroundStyle(Color.white.opacity(0.92))
                    }
                }
                .offset(joyOffset)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .help(isHwMode
            ? (locale == .en
                ? "Analog stick · \(c.joystickModeLabel) (click to configure; nudge right during Model Picker to adjust reasoning)"
                : "模拟摇杆 · \(c.joystickModeLabel)（点击配置；短按旋钮选模型时按右调力度）")
            : (locale == .en
                ? "8-way stick (click to cycle Up / Left / Down / Right)"
                : "8 向摇杆（点击切换上 / 左 / 下 / 右方向配置）"))
    }

    private func statusIndicatorClusterCell(c: CanvasSliceModel) -> some View {
        let board = store.sidebar.boardLayers
        let matchesLayer = board.layers.contains { $0.layer == c.boardPreviewLayer && $0.profileId == c.selectedProfileId }
        let matchesUsb = c.boardPreviewLayer == nil && board.usbStartupProfileId == c.selectedProfileId
        let isBoardPreview = c.selectedProfileId != nil && (matchesLayer || matchesUsb)
        let previewLabel = isBoardPreview ? c.boardPreviewLabel : store.tr("本机方案，没有选择物理层")
        let leds = isBoardPreview && c.touchLedStates.count == 3 ? c.touchLedStates : [false, false, false]
        return VStack(spacing: 6) {
            HStack(spacing: 6) {
                ForEach(0..<3, id: \.self) { index in
                    Circle()
                        .fill(leds[index] ? UITheme.textPrimary : UITheme.strongBorder)
                        .frame(width: 5.5, height: 5.5)
                }
            }
            Text(isBoardPreview ? (c.boardPreviewLayer.map { "L\($0)" } ?? "USB") : store.tr("本机"))
                .font(UITheme.monoBadge)
                .foregroundStyle(UITheme.textPrimary)
            Text(isBoardPreview ? store.tr("预览") : store.tr("方案"))
                .font(UITheme.micro)
                .foregroundStyle(UITheme.textMuted)
        }
        .frame(width: cellSize, height: cellSize)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(UITheme.subtleSurface))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
        .help(previewLabel)
        .accessibilityLabel(previewLabel)
    }

    private func programmableKeyCell(slot: Int, c: CanvasSliceModel, ss: SessionStripSliceModel) -> some View {
        let cell = c.keycapCells.first(where: { $0.primarySlot == slot })
        return keycapButton(slot: slot, width: cellSize, cell: cell, c: c, ss: ss)
    }

    private func wideProgrammableKeyCell(c: CanvasSliceModel, ss: SessionStripSliceModel) -> some View {
        let activeSlot = (c.selectedLogicalSlot == 11) ? 11 : 10
        let cell = c.keycapCells.first(where: { $0.primarySlot == 10 })
        return keycapButton(
            slot: activeSlot,
            width: cellSize * 2 + gridSpacing,
            cell: cell,
            c: c,
            ss: ss,
            forceSelected: c.selectedLogicalSlot == 10 || c.selectedLogicalSlot == 11
        )
    }

    private func keycapButton(
        slot: Int,
        width: CGFloat,
        cell: KeycapCellModel?,
        c: CanvasSliceModel,
        ss: SessionStripSliceModel,
        forceSelected: Bool = false
    ) -> some View {
        let isSelected = forceSelected || (cell?.isSelected == true) || (c.selectedLogicalSlot == slot && c.selectedSurface.hasPrefix("Key"))
        let isTaskRole = (slot < 6) && (cell?.slotRole == "TaskKey") && agentKind.supportsLiveSessionSlots
        let sessionSlot = (slot < 6) ? ss.sessionSlots.first(where: { $0.slotIndex == slot }) : nil
        let keycap = CodexKeycapId(rustVariant: cell?.keycapId ?? "Apps")
        let sublabel = (c.protectedLock != nil) ? store.tr("原生直通") : (cell?.conciseCaption ?? store.tr("空白"))

        return BoardKeycapCellButton(
            store: store,
            slot: slot,
            width: width,
            cellSize: cellSize,
            surface: cell?.surface,
            isSelected: isSelected,
            isTaskRole: isTaskRole,
            sessionSlot: sessionSlot,
            keycap: keycap,
            showLabels: c.showKeycapLabels,
            sublabel: sublabel,
            jumpDirectlyOnGlowingSlotTap: jumpDirectlyOnGlowingSlotTap,
            locale: store.locale
        )
        .equatable()
    }

    private func compactBoardToolbar(c: CanvasSliceModel, ss: SessionStripSliceModel) -> some View {
        let locale = store.locale
        return HStack(spacing: 16) {
            Toggle(
                store.tr("显示键帽文字"),
                isOn: Binding(
                    get: { c.showKeycapLabels },
                    set: { store.dispatch("canvas", ["SetShowKeycapLabels": $0]) }
                )
            )
            .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
            Spacer(minLength: 0)
            Menu {
                if agentKind.supportsLiveSessionSlots {
                    Toggle(store.tr("点击发光键打开会话"), isOn: $jumpDirectlyOnGlowingSlotTap)
                    Button(store.tr("上方六键设为会话键")) { store.dispatch("session_strip", "BindTopSixAsSessionKeys") }
                }
                Button(store.tr("恢复默认布局")) { store.dispatch("session_strip", "RestoreDefaultLayout") }
                Divider()
                Button(store.tr("模型选择（短按模型 · 长按搜会话）")) {
                    store.dispatch("canvas", ["ApplyKnobJoystickPreset": "ModelSelectionAndMouse"])
                }
                if agentKind == .antigravity {
                    Divider()
                    Button(store.tr("旋钮切换会话，摇杆滚动对话")) {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CycleWorkingAndBlueDot"])
                    }
                    Button(store.tr("旋钮滚动项目，摇杆滚动对话")) {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                    }
                }
                if agentKind == .codex {
                    Divider()
                    Button(store.tr("旋钮滚动对话")) {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CodexScrollChatAndBottom"])
                    }
                    Button(store.tr("旋钮调整推理档位")) {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CodexReasoningEffortAndSpeed"])
                    }
                }
                Divider()
                Button(store.tr("摇杆鼠标 + 旋钮滚轮")) {
                    store.dispatch("canvas", ["ApplyKnobJoystickPreset": "MousePointerAndScroll"])
                }
                if !ss.sessionSlots.isEmpty {
                    Divider()
                    ForEach(ss.sessionSlots) { slot in
                        Button(slot.title.isEmpty ? (locale == .en ? "Session \(slot.slotIndex + 1)" : "会话 \(slot.slotIndex + 1)") : slot.title) {
                            store.dispatch("session_strip", ["ActivateSlot": slot.slotIndex])
                        }
                    }
                    Button(store.tr("全部标为已读")) { store.dispatch("session_strip", "AcknowledgeAllCompleted") }
                }
            } label: {
                CapsuleMenuLabel(store.tr("布局选项"), icon: "slider.horizontal.3")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .focusEffectDisabled()
            .fixedSize()
        }
    }
}

struct SimpleLightingControlCard: View {
    let store: CoreStore
    var compact: Bool = false

    var body: some View {
        let locale = store.locale
        let ss = store.sessionStrip
        let lp = ss.simpleLighting

        VStack(alignment: .leading, spacing: compact ? 14 : 24) {
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader(store.tr("会话灯光")) {
                    HStack(spacing: 8) {
                        Circle()
                            .fill(ss.unreadCompletedCount > 0 ? UITheme.textPrimary : UITheme.textMuted.opacity(0.55))
                            .frame(width: 6, height: 6)
                        Text(locale == .en ? "Unread \(ss.unreadCompletedCount)" : "未读 \(ss.unreadCompletedCount)")
                            .font(UITheme.micro)
                            .foregroundStyle(UITheme.textSecondary)
                        Text("·").foregroundStyle(UITheme.textMuted)
                        Circle()
                            .fill(ss.workingCount > 0 ? UITheme.statusSuccess : UITheme.textMuted.opacity(0.55))
                            .frame(width: 6, height: 6)
                        Text(locale == .en ? "Running \(ss.workingCount)" : "进行中 \(ss.workingCount)")
                            .font(UITheme.micro)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                }

                SettingsCard {
                    SettingsRow(
                        store.tr("开启灯光"),
                        subtitle: compact ? nil : store.tr("启用小键盘会话状态灯与环境底光反馈"),
                        showsDivider: lp.lightingEnabled
                    ) {
                        Toggle(
                            "",
                            isOn: Binding(
                                get: { lp.lightingEnabled },
                                set: { store.dispatch("session_strip", ["SetLightingEnabled": $0]) }
                            )
                        )
                        .labelsHidden()
                        .toggleStyle(NeutralSwitchToggleStyle())
                    }

                    if lp.lightingEnabled {
                        SettingsRow(
                            store.tr("随会话状态变色"),
                            subtitle: compact ? nil : store.tr("按进行中、待确认与已完成状态自动切换灯光颜色"),
                            showsDivider: true
                        ) {
                            Toggle(
                                "",
                                isOn: Binding(
                                    get: { lp.autoSessionColor },
                                    set: { store.dispatch("session_strip", ["SetAutoSessionColor": $0]) }
                                )
                            )
                            .labelsHidden()
                            .toggleStyle(NeutralSwitchToggleStyle())
                        }

                        SettingsRow(
                            store.tr("进行中的会话使用呼吸灯"),
                            subtitle: compact ? nil : store.tr("任务运行期间以柔和呼吸灯效提示后台进展"),
                            showsDivider: true
                        ) {
                            Toggle(
                                "",
                                isOn: Binding(
                                    get: { lp.glowOnWorking },
                                    set: { store.dispatch("session_strip", ["SetGlowOnWorking": $0]) }
                                )
                            )
                            .labelsHidden()
                            .toggleStyle(NeutralSwitchToggleStyle())
                        }

                        let showSessionSaRgbSync = !compact
                            && (store.v070Hardware.supported || store.v070Hardware.sessionSaRgbSyncEnabled)
                        SettingsRow(
                            store.tr("仅在有新完成或进行中的会话时亮起"),
                            subtitle: compact ? nil : store.tr("无未读或运行中任务时保持安静熄灯"),
                            showsDivider: showSessionSaRgbSync
                        ) {
                            Toggle(
                                "",
                                isOn: Binding(
                                    get: { lp.onlyGlowOnAttentionOrCompleted },
                                    set: { store.dispatch("session_strip", ["SetOnlyGlowOnAttentionOrCompleted": $0]) }
                                )
                            )
                            .labelsHidden()
                            .toggleStyle(NeutralSwitchToggleStyle())
                        }

                        if showSessionSaRgbSync {
                            SettingsRow(
                                store.tr("单键状态灯同步"),
                                subtitle: store.tr("按会话状态自动映射自定义单键灯效，退出或休眠时自动恢复默认"),
                                showsDivider: false
                            ) {
                                Toggle(
                                    "",
                                    isOn: Binding(
                                        get: { store.v070Hardware.sessionSaRgbSyncEnabled },
                                        set: { store.setV070SessionSaRgbSync($0) }
                                    )
                                )
                                .labelsHidden()
                                .toggleStyle(NeutralSwitchToggleStyle())
                                .disabled(store.v070Hardware.busy)
                                .accessibilityIdentifier("v070-session-sa-rgb-sync")
                            }
                        }
                    }
                }
            }

            if lp.lightingEnabled {
                VStack(alignment: .leading, spacing: 10) {
                    if !compact { SettingsSectionHeader(store.tr("灯效选项")) }
                    SettingsCard {
                        SettingsRow(store.tr("完成与空闲时的颜色"), subtitle: compact ? nil : store.tr("会话完成待检视或空闲待机时的默认灯光"), showsDivider: true) {
                            SelectDropdown(
                                options: L10n.ambientLightingColors(locale),
                                selection: Binding(
                                    get: { lp.ambientColor },
                                    set: { store.dispatch("session_strip", ["SetAmbientColor": $0]) }
                                )
                            )
                        }
                        SettingsRow(store.tr("进行中时的颜色"), subtitle: compact ? nil : store.tr("后台代理持续生成或执行工具时的灯光"), showsDivider: true) {
                            SelectDropdown(
                                options: L10n.workingLightingColors(locale),
                                selection: Binding(
                                    get: { lp.workingColor },
                                    set: { store.dispatch("session_strip", ["SetWorkingColor": $0]) }
                                )
                            )
                        }
                        SettingsRow(store.tr("整体亮度"), subtitle: compact ? nil : store.tr("调节 Creator Micro 会话灯光的整体亮度"), showsDivider: true) {
                            SelectDropdown(
                                options: L10n.brightnessOptions(locale),
                                selection: Binding(
                                    get: { lp.brightness },
                                    set: { store.dispatch("session_strip", ["SetBrightnessPreset": $0]) }
                                ),
                                minTriggerWidth: 116
                            )
                        }
                        SettingsRow(store.tr("自动熄灯"), subtitle: compact ? nil : store.tr("无操作后自动熄灭灯光，按键或会话变化时自动亮起")) {
                            SelectDropdown(
                                options: L10n.autoOffOptions(locale),
                                selection: Binding(
                                    get: { lp.autoOffTimeout },
                                    set: { store.dispatch("session_strip", ["SetAutoOffTimeout": $0]) }
                                ),
                                minTriggerWidth: 116
                            )
                        }
                    }
                }

                SettingsCard {
                    if compact {
                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                Text(store.tr("在键盘上预览")).font(.system(size: 13, weight: .medium)).foregroundStyle(UITheme.textPrimary)
                                Spacer()
                                Button(store.tr("恢复自动")) { store.dispatch("session_strip", ["TriggerTestLighting": NSNull()]) }
                                    .surfaceButton(.ghost)
                            }
                            HStack(spacing: 6) {
                                Button(store.tr("完成与空闲")) { store.dispatch("session_strip", ["TriggerTestLighting": "completed"]) }
                                    .surfaceButton(.secondary)
                                Button(store.tr("进行中")) { store.dispatch("session_strip", ["TriggerTestLighting": "thinking"]) }
                                    .surfaceButton(.secondary)
                                Button(store.tr("待确认")) { store.dispatch("session_strip", ["TriggerTestLighting": "awaiting_input"]) }
                                    .surfaceButton(.secondary)
                            }
                            Rectangle().fill(UITheme.rowDivider).frame(height: 1)
                            HStack {
                                Text(store.tr("未读提醒")).font(.system(size: 13, weight: .medium)).foregroundStyle(UITheme.textPrimary)
                                Spacer()
                                Button(store.tr("全部标为已读")) { store.dispatch("session_strip", "AcknowledgeAllCompleted") }
                                    .surfaceButton(.ghost)
                            }
                        }
                        .padding(16)
                    } else {
                        SettingsRow(store.tr("在键盘上预览"), subtitle: store.tr("临时点亮不同会话状态灯以核对色彩层次"), showsDivider: true) {
                            HStack(spacing: 6) {
                                Button(store.tr("完成与空闲")) { store.dispatch("session_strip", ["TriggerTestLighting": "completed"]) }
                                    .surfaceButton(.secondary)
                                Button(store.tr("进行中")) { store.dispatch("session_strip", ["TriggerTestLighting": "thinking"]) }
                                    .surfaceButton(.secondary)
                                Button(store.tr("待确认")) { store.dispatch("session_strip", ["TriggerTestLighting": "awaiting_input"]) }
                                    .surfaceButton(.secondary)
                                Button(store.tr("恢复自动")) { store.dispatch("session_strip", ["TriggerTestLighting": NSNull()]) }
                                    .surfaceButton(.ghost)
                            }
                        }
                        SettingsRow(store.tr("未读完成灯状态"), subtitle: store.tr("清除所有已完成会话的未读蓝灯提示")) {
                            Button(store.tr("全部标为已读")) { store.dispatch("session_strip", "AcknowledgeAllCompleted") }
                                .surfaceButton(.secondary)
                        }
                    }
                }
            }
        }
    }
}

private struct BoardKeycapCellButton: View, Equatable {
    let store: CoreStore
    let slot: Int
    let width: CGFloat
    let cellSize: CGFloat
    let surface: String?
    let isSelected: Bool
    let isTaskRole: Bool
    let sessionSlot: SessionSlotModel?
    let keycap: CodexKeycapId
    let showLabels: Bool
    let sublabel: String
    let jumpDirectlyOnGlowingSlotTap: Bool
    let locale: ResolvedLocale

    static func == (lhs: BoardKeycapCellButton, rhs: BoardKeycapCellButton) -> Bool {
        lhs.slot == rhs.slot
            && lhs.width == rhs.width
            && lhs.cellSize == rhs.cellSize
            && lhs.surface == rhs.surface
            && lhs.isSelected == rhs.isSelected
            && lhs.isTaskRole == rhs.isTaskRole
            && lhs.sessionSlot == rhs.sessionSlot
            && lhs.keycap == rhs.keycap
            && lhs.showLabels == rhs.showLabels
            && lhs.sublabel == rhs.sublabel
            && lhs.jumpDirectlyOnGlowingSlotTap == rhs.jumpDirectlyOnGlowingSlotTap
            && lhs.locale == rhs.locale
    }

    var body: some View {
        let unassignedSession = L10n.tr("尚未分配会话", locale)
        let unboundKey = L10n.tr("未绑定", locale)
        Button {
            if let surface {
                store.dispatch("canvas", ["SelectSurface": surface])
            } else {
                store.dispatch("canvas", ["SelectLogicalSlot": slot])
            }
            if isTaskRole,
               jumpDirectlyOnGlowingSlotTap,
               let s = sessionSlot,
               s.status != "Empty"
            {
                store.dispatch("session_strip", ["ActivateSlot": slot])
            }
        } label: {
            Group {
                if isTaskRole {
                    taskKeycapSurface
                } else {
                    actionKeycapSurface
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .help(isTaskRole ? (sessionSlot?.title ?? unassignedSession) : sublabel)
        .accessibilityLabel(isTaskRole
            ? (locale == .en
                ? "Session \(slot + 1), \(sessionSlot?.title ?? unassignedSession), \(SessionKeyStatusView.label(for: sessionSlot, locale: locale))"
                : "会话 \(slot + 1)，\(sessionSlot?.title ?? unassignedSession)，\(SessionKeyStatusView.label(for: sessionSlot, locale: locale))")
            : (locale == .en
                ? "Key \(slot + 1), \(sublabel.isEmpty ? unboundKey : sublabel)"
                : "按键 \(slot + 1)，\(sublabel.isEmpty ? unboundKey : sublabel)"))
    }

    private var taskKeycapSurface: some View {
        let status = sessionSlot?.kind ?? "Idle"
        let hasActiveState = (sessionSlot?.isUnreadBlueDot == true)
            || (sessionSlot?.isWorkingAnimated == true)
            || (status == "NeedsAttention")
        let ledColor = Color(hex: sessionSlot?.ledColorHex ?? "#36526D")
        let fillSurface = (sessionSlot?.isUnreadBlueDot == true || status == "NeedsAttention")
            ? UITheme.accentSoft
            : UITheme.subtleSurface
        let borderColor: Color = {
            if isSelected { return UITheme.accent }
            if hasActiveState { return ledColor.opacity(0.85) }
            return UITheme.strongBorder
        }()

        return SessionKeycapContent(slotIndex: slot, sessionSlot: sessionSlot, showLabels: showLabels, locale: locale)
            .padding(.horizontal, 6)
            .padding(.vertical, 6)
            .frame(width: width, height: cellSize)
            .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(fillSurface))
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .stroke(borderColor, lineWidth: isSelected ? 2.5 : (hasActiveState ? 2.0 : 1.0))
            )
            .contentShape(Rectangle())
    }

    private var actionKeycapSurface: some View {
        VStack(spacing: 4) {
            Spacer(minLength: 2)
            CodexKeycapGlyphView(keycap: keycap, size: showLabels ? 20 : 23, color: UITheme.textPrimary)
                .equatable()
            Spacer(minLength: 2)
            if showLabels {
                Text(sublabel)
                    .font(UITheme.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 6)
        .frame(width: width, height: cellSize)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(UITheme.surface))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(isSelected ? UITheme.accent : UITheme.strongBorder, lineWidth: isSelected ? 2.5 : 1.0)
        )
        .contentShape(Rectangle())
    }
}

struct CreatorMicroSessionListView: View {
    let store: CoreStore
    let agentKind: AgentProfileKind
    @State private var isLightingPopoverPresented = false

    var body: some View {
        let locale = store.locale
        let canvas = store.canvas
        let sessions = store.sessionStrip
        let keys = canvas.keycapCells.filter {
            agentKind.supportsLiveSessionSlots && $0.slotRole == "TaskKey" && (0..<6).contains($0.primarySlot)
        }
        if !keys.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Text(store.tr("会话"))
                        .font(UITheme.sectionHeader)
                        .foregroundStyle(UITheme.textPrimary)
                    if sessions.workingCount > 0 || sessions.unreadCompletedCount > 0 {
                        Text(locale == .en
                            ? "Running \(sessions.workingCount) · Completed \(sessions.unreadCompletedCount)"
                            : "进行中 \(sessions.workingCount) · 已完成 \(sessions.unreadCompletedCount)")
                            .font(UITheme.caption)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                    Spacer()
                    if sessions.unreadCompletedCount > 0 {
                        Button(store.tr("已读熄灯")) {
                            store.dispatch("session_strip", "AcknowledgeAllCompleted")
                        }
                        .surfaceButton(.ghost)
                        .accessibilityIdentifier("session-acknowledge-all")
                    }
                    Button {
                        isLightingPopoverPresented.toggle()
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: "lightbulb")
                                .font(.system(size: 12, weight: .medium))
                            Text(store.tr("灯光筛选"))
                        }
                    }
                    .surfaceButton(.secondary)
                    .accessibilityIdentifier("session-lighting-filter")
                    .popover(isPresented: $isLightingPopoverPresented, arrowEdge: .bottom) {
                        ScrollView {
                            SimpleLightingControlCard(store: store, compact: true)
                                .padding(14)
                        }
                        .frame(width: 380, height: 430)
                        .background(UITheme.pageBackground)
                    }
                }
                SettingsCard {
                    VStack(spacing: 0) {
                        ForEach(Array(keys.enumerated()), id: \.element.id) { idx, key in
                            let session = sessions.sessionSlots.first { $0.slotIndex == key.primarySlot }
                            SessionTitleRow(
                                slotIndex: key.primarySlot,
                                title: session?.title.isEmpty == false ? session!.title : store.tr("尚未分配会话"),
                                sessionSlot: session,
                                isSelected: canvas.selectedSurface == key.surface,
                                locale: locale,
                                onSelect: { store.dispatch("canvas", ["SelectSurface": key.surface]) }
                            )
                            if idx + 1 < keys.count {
                                Rectangle().fill(UITheme.rowDivider).frame(height: 1).padding(.horizontal, 16)
                            }
                        }
                    }
                }
            }
        }
    }
}

struct SessionTitleRow: View {
    let slotIndex: Int
    let title: String
    let sessionSlot: SessionSlotModel?
    let isSelected: Bool
    var locale: ResolvedLocale = .zhHans
    var onSelect: () -> Void

    var body: some View {
        Button(action: onSelect) {
            HStack(alignment: .top, spacing: 12) {
                Text("\(slotIndex + 1)")
                    .font(UITheme.bodySemibold)
                    .foregroundStyle(isSelected ? UITheme.textPrimary : UITheme.textSecondary)
                    .frame(width: 28, height: 28)
                    .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                Text(title)
                    .font(UITheme.body)
                    .foregroundStyle(UITheme.textPrimary)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 4)
                SessionKeyStatusView(sessionSlot: sessionSlot, locale: locale)
                    .font(UITheme.caption)
                    .fixedSize()
                    .padding(.top, 4)
            }
            .padding(12)
            .background(isSelected ? UITheme.accentSoft : Color.clear, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .accessibilityIdentifier("session-title-\(slotIndex)")
        .accessibilityLabel(locale == .en
            ? "Session \(slotIndex + 1), \(title), \(SessionKeyStatusView.label(for: sessionSlot, locale: locale))"
            : "会话 \(slotIndex + 1)，\(title)，\(SessionKeyStatusView.label(for: sessionSlot, locale: locale))")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

struct SessionKeycapContent: View {
    let slotIndex: Int
    let sessionSlot: SessionSlotModel?
    let showLabels: Bool
    var locale: ResolvedLocale = .zhHans

    var body: some View {
        VStack(spacing: 6) {
            Text("\(slotIndex + 1)")
                .font(UITheme.heading)
                .foregroundStyle(UITheme.textPrimary)
            if showLabels {
                SessionKeyStatusView(sessionSlot: sessionSlot, locale: locale)
                    .font(UITheme.caption)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct SessionKeyStatusView: View {
    let sessionSlot: SessionSlotModel?
    var locale: ResolvedLocale = .zhHans

    static func label(for slot: SessionSlotModel?, locale: ResolvedLocale = .zhHans) -> String {
        guard let slot else { return L10n.tr("未分配", locale) }
        if slot.title.isEmpty && !slot.hasAssignedPlace { return L10n.tr("未分配", locale) }
        switch slot.kind {
        case "Working": return L10n.tr("进行中", locale)
        case "BlueDotCompleted": return L10n.tr("已完成", locale)
        case "NeedsAttention": return L10n.tr("待回应", locale)
        case "Error": return L10n.tr("执行异常", locale)
        case "Idle": return L10n.tr("就绪", locale)
        default: return L10n.tr("状态未知", locale)
        }
    }

    var body: some View {
        Text(Self.label(for: sessionSlot, locale: locale))
            .foregroundStyle(sessionSlot?.isUnreadBlueDot == true ? UITheme.textPrimary : UITheme.textSecondary)
    }
}
