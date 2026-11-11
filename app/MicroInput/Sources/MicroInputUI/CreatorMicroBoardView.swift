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
        let agentKind = AgentProfileKind.from(profileName: s.selectedRawName)
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("当前控制")
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)
                    Text(h.hostExecutionEnabled ? store.statusMenu.activeProfileName : "按键控制已暂停")
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
                            Text(s.selectedDisplayName)
                                .font(UITheme.sectionHeader)
                                .foregroundStyle(UITheme.textPrimary)
                            Text("正在编辑 · \(s.inspectorLayerName)")
                                .font(UITheme.caption)
                                .foregroundStyle(UITheme.textSecondary)
                        }
                        Spacer(minLength: 0)
                        Menu {
                            ForEach(s.layerTabs) { tab in
                                Button(tab.name) { store.dispatch("canvas", ["SelectLayer": tab.id]) }
                            }
                            Divider()
                            Button("新建层") { onPromptNewLayer() }
                            Button("重命名当前层") { onPromptRenameLayer() }.disabled(s.isProtectedLayer)
                            Button("复制当前层") {
                                store.dispatch("inspector", ["DuplicateSelectedLayer": ["name": "\(s.inspectorLayerName) 副本"]])
                            }
                            Button("删除当前层", role: .destructive) { store.dispatch("inspector", "DeleteSelectedLayer") }
                                .disabled(s.isProtectedLayer || s.layerTabs.count <= 1)
                        } label: {
                            CapsuleMenuLabel("按键层", icon: "square.3.layers.3d")
                        }
                        .menuStyle(.borderlessButton)
                        .menuIndicator(.hidden)
                        .focusEffectDisabled()
                        .fixedSize()

                        if let profileId = s.selectedProfileId, profileId != s.activeHostProfileId {
                            Button("启用此方案") {
                                store.dispatch("sidebar", ["SelectProfile": profileId])
                            }
                            .surfaceButton(.primary)
                        }
                        if let agent = DesktopAgent.named(s.selectedRawName), !agent.isGeneralFallback {
                            Button("打开应用") {
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
                        SettingsRow("这一层尚未分配应用", subtitle: "在应用方案中分配后，即可查看与编辑对应按键。")
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
            if !h.deviceConnected {
                Button("重新检测键盘") {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .surfaceButton(.primary)
            }
            if !h.hostExecutionEnabled {
                Button("开启按键控制") {
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
    @State private var jumpDirectlyOnGlowingSlotTap = true

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
        let isSelected = c.selectedSurface.hasPrefix("Encoder") || (13...15).contains(c.selectedLogicalSlot)
        return Button {
            if !c.selectedSurface.hasPrefix("Encoder") {
                store.dispatch("canvas", ["SelectSurface": "EncoderCcw"])
            } else if c.selectedSurface == "EncoderCcw" {
                store.dispatch("canvas", ["SelectSurface": "EncoderCw"])
            } else if c.selectedSurface == "EncoderCw" {
                store.dispatch("canvas", ["SelectSurface": "EncoderPress"])
            } else {
                store.dispatch("canvas", ["SelectSurface": "EncoderCcw"])
            }
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
                    .overlay(
                        Circle()
                            .stroke(isSelected ? UITheme.accent : Color.black.opacity(0.35), lineWidth: isSelected ? 2.5 : 1)
                    )
                VStack(spacing: 0) {
                    Capsule().fill(Color.white.opacity(0.92)).frame(width: 2.2, height: 12).padding(.top, 7)
                    Spacer()
                }
                .frame(width: cellSize, height: cellSize)
                .rotationEffect(.degrees(c.selectedSurface == "EncoderCcw" ? -32 : (c.selectedSurface == "EncoderCw" ? 32 : 0)))

                if isSelected {
                    Text(c.selectedSurface == "EncoderCcw" ? "左旋" : (c.selectedSurface == "EncoderCw" ? "右旋" : "按下"))
                        .font(UITheme.micro)
                        .foregroundStyle(Color.white.opacity(0.92))
                        .padding(.top, 14)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .help("无极金属旋钮（点击切换左旋 / 右旋 / 按下配置）")
    }

    private func analogJoystickCell(c: CanvasSliceModel) -> some View {
        let isSelected = c.selectedSurface == "Joystick" || c.joystickIsSelected
        let activeDir = c.selectedJoystickDirection
        let joyOffset: CGSize = {
            guard isSelected else { return .zero }
            switch activeDir {
            case "North": return CGSize(width: 0, height: -4)
            case "West": return CGSize(width: -4, height: 0)
            case "South": return CGSize(width: 0, height: 4)
            case "East": return CGSize(width: 4, height: 0)
            default: return .zero
            }
        }()
        let dirArrow: String = {
            switch activeDir {
            case "North": return "↑"
            case "West": return "←"
            case "South": return "↓"
            case "East": return "→"
            default: return "↑"
            }
        }()
        return Button {
            if !isSelected {
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
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .stroke(isSelected ? UITheme.accent : UITheme.hairlineBorder, lineWidth: isSelected ? 2.5 : 1)
                    )
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
                    if isSelected {
                        Text(dirArrow).font(UITheme.captionMedium).foregroundStyle(Color.white.opacity(0.92))
                    }
                }
                .offset(joyOffset)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .help("8 向摇杆（点击切换上 / 左 / 下 / 右方向配置）")
    }

    private func statusIndicatorClusterCell(c: CanvasSliceModel) -> some View {
        let board = store.sidebar.boardLayers
        let matchesLayer = board.layers.contains { $0.layer == c.boardPreviewLayer && $0.profileId == c.selectedProfileId }
        let matchesUsb = c.boardPreviewLayer == nil && board.usbStartupProfileId == c.selectedProfileId
        let isBoardPreview = c.selectedProfileId != nil && (matchesLayer || matchesUsb)
        let previewLabel = isBoardPreview ? c.boardPreviewLabel : "本机方案，没有选择物理层"
        let leds = isBoardPreview && c.touchLedStates.count == 3 ? c.touchLedStates : [false, false, false]
        return VStack(spacing: 6) {
            HStack(spacing: 6) {
                ForEach(0..<3, id: \.self) { index in
                    Circle()
                        .fill(leds[index] ? UITheme.textPrimary : UITheme.strongBorder)
                        .frame(width: 5.5, height: 5.5)
                }
            }
            Text(isBoardPreview ? (c.boardPreviewLayer.map { "L\($0)" } ?? "USB") : "本机")
                .font(UITheme.monoBadge)
                .foregroundStyle(UITheme.textPrimary)
            Text(isBoardPreview ? "预览" : "方案")
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
        let sublabel = (c.protectedLock != nil) ? "原生直通" : (cell?.conciseCaption ?? "空白")

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
            jumpDirectlyOnGlowingSlotTap: jumpDirectlyOnGlowingSlotTap
        )
        .equatable()
    }

    private func compactBoardToolbar(c: CanvasSliceModel, ss: SessionStripSliceModel) -> some View {
        HStack(spacing: 16) {
            Toggle(
                "显示键帽文字",
                isOn: Binding(
                    get: { c.showKeycapLabels },
                    set: { store.dispatch("canvas", ["SetShowKeycapLabels": $0]) }
                )
            )
            .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
            Spacer(minLength: 0)
            Menu {
                if agentKind.supportsLiveSessionSlots {
                    Toggle("点击发光键打开会话", isOn: $jumpDirectlyOnGlowingSlotTap)
                    Button("上方六键设为会话键") { store.dispatch("session_strip", "BindTopSixAsSessionKeys") }
                }
                Button("恢复默认布局") { store.dispatch("session_strip", "RestoreDefaultLayout") }
                if agentKind == .antigravity {
                    Divider()
                    Button("旋钮切换会话，摇杆滚动对话") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CycleWorkingAndBlueDot"])
                    }
                    Button("旋钮滚动项目，摇杆滚动对话") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                    }
                }
                if !ss.sessionSlots.isEmpty {
                    Divider()
                    ForEach(ss.sessionSlots) { slot in
                        Button(slot.title.isEmpty ? "会话 \(slot.slotIndex + 1)" : slot.title) {
                            store.dispatch("session_strip", ["ActivateSlot": slot.slotIndex])
                        }
                    }
                    Button("全部标为已读") { store.dispatch("session_strip", "AcknowledgeAllCompleted") }
                }
            } label: {
                CapsuleMenuLabel("布局选项", icon: "slider.horizontal.3")
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

    private static let ambientColorOptions: [SelectOption<String>] = [
        .init(value: "OceanBlue", title: "海洋蓝", subtitle: "默认完成与空闲蓝光"),
        .init(value: "CyanTeal", title: "青碧", subtitle: "冷静低干扰青绿光"),
        .init(value: "PureWhite", title: "纯白", subtitle: "纯净中性单色背光"),
        .init(value: "EmeraldGreen", title: "翡翠绿", subtitle: "柔和醒目的运行态绿光"),
        .init(value: "AmberGold", title: "琥珀金", subtitle: "温暖待确认提示暖光"),
    ]

    private static let workingColorOptions: [SelectOption<String>] = [
        .init(value: "EmeraldGreen", title: "翡翠绿", subtitle: "柔和醒目的运行态绿光"),
        .init(value: "AmberGold", title: "琥珀金", subtitle: "温暖待确认提示暖光"),
        .init(value: "CyanTeal", title: "青碧", subtitle: "冷静低干扰青绿光"),
        .init(value: "PureWhite", title: "纯白", subtitle: "纯净中性单色背光"),
        .init(value: "OceanBlue", title: "海洋蓝", subtitle: "默认完成与空闲蓝光"),
    ]

    private static let brightnessOptions: [SelectOption<String>] = [
        .init(value: "Soft", title: "柔和 (35%)"),
        .init(value: "Standard", title: "标准 (65%)"),
        .init(value: "Bright", title: "明亮 (95%)"),
    ]

    private static let autoOffOptions: [SelectOption<String>] = [
        .init(value: "OneMinute", title: "1 分钟"),
        .init(value: "ThreeMinutes", title: "3 分钟"),
        .init(value: "FiveMinutes", title: "5 分钟"),
        .init(value: "Never", title: "常亮"),
    ]

    var body: some View {
        let ss = store.sessionStrip
        let lp = ss.simpleLighting

        VStack(alignment: .leading, spacing: compact ? 14 : 24) {
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader("会话灯光") {
                    HStack(spacing: 8) {
                        Circle()
                            .fill(ss.unreadCompletedCount > 0 ? UITheme.textPrimary : UITheme.textMuted.opacity(0.55))
                            .frame(width: 6, height: 6)
                        Text("未读 \(ss.unreadCompletedCount)")
                            .font(UITheme.micro)
                            .foregroundStyle(UITheme.textSecondary)
                        Text("·").foregroundStyle(UITheme.textMuted)
                        Circle()
                            .fill(ss.workingCount > 0 ? UITheme.statusSuccess : UITheme.textMuted.opacity(0.55))
                            .frame(width: 6, height: 6)
                        Text("进行中 \(ss.workingCount)")
                            .font(UITheme.micro)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                }

                SettingsCard {
                    SettingsRow(
                        "开启灯光",
                        subtitle: compact ? nil : "启用小键盘会话状态灯与环境底光反馈",
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
                            "随会话状态变色",
                            subtitle: compact ? nil : "按进行中、待确认与已完成状态自动切换灯光颜色",
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
                            "进行中的会话使用呼吸灯",
                            subtitle: compact ? nil : "任务运行期间以柔和呼吸灯效提示后台进展",
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

                        SettingsRow(
                            "仅在有新完成或进行中的会话时亮起",
                            subtitle: compact ? nil : "无未读或运行中任务时保持安静熄灯"
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
                    }
                }
            }

            if lp.lightingEnabled {
                VStack(alignment: .leading, spacing: 10) {
                    if !compact { SettingsSectionHeader("灯效选项") }
                    SettingsCard {
                        SettingsRow("完成与空闲时的颜色", subtitle: compact ? nil : "会话完成待检视或空闲待机时的默认灯光", showsDivider: true) {
                            SelectDropdown(
                                options: Self.ambientColorOptions,
                                selection: Binding(
                                    get: { lp.ambientColor },
                                    set: { store.dispatch("session_strip", ["SetAmbientColor": $0]) }
                                )
                            )
                        }
                        SettingsRow("进行中时的颜色", subtitle: compact ? nil : "后台代理持续生成或执行工具时的灯光", showsDivider: true) {
                            SelectDropdown(
                                options: Self.workingColorOptions,
                                selection: Binding(
                                    get: { lp.workingColor },
                                    set: { store.dispatch("session_strip", ["SetWorkingColor": $0]) }
                                )
                            )
                        }
                        SettingsRow("整体亮度", subtitle: compact ? nil : "调节 Creator Micro 会话灯光的整体亮度", showsDivider: true) {
                            SelectDropdown(
                                options: Self.brightnessOptions,
                                selection: Binding(
                                    get: { lp.brightness },
                                    set: { store.dispatch("session_strip", ["SetBrightnessPreset": $0]) }
                                ),
                                minTriggerWidth: 116
                            )
                        }
                        SettingsRow("自动熄灯", subtitle: compact ? nil : "无操作后自动熄灭灯光，按键或会话变化时自动亮起") {
                            SelectDropdown(
                                options: Self.autoOffOptions,
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
                                Text("在键盘上预览").font(.system(size: 13, weight: .medium)).foregroundStyle(UITheme.textPrimary)
                                Spacer()
                                Button("恢复自动") { store.dispatch("session_strip", ["TriggerTestLighting": NSNull()]) }
                                    .surfaceButton(.ghost)
                            }
                            HStack(spacing: 6) {
                                Button("完成与空闲") { store.dispatch("session_strip", ["TriggerTestLighting": "completed"]) }
                                    .surfaceButton(.secondary)
                                Button("进行中") { store.dispatch("session_strip", ["TriggerTestLighting": "thinking"]) }
                                    .surfaceButton(.secondary)
                                Button("待确认") { store.dispatch("session_strip", ["TriggerTestLighting": "awaiting_input"]) }
                                    .surfaceButton(.secondary)
                            }
                            Rectangle().fill(UITheme.rowDivider).frame(height: 1)
                            HStack {
                                Text("未读提醒").font(.system(size: 13, weight: .medium)).foregroundStyle(UITheme.textPrimary)
                                Spacer()
                                Button("全部标为已读") { store.dispatch("session_strip", "AcknowledgeAllCompleted") }
                                    .surfaceButton(.ghost)
                            }
                        }
                        .padding(16)
                    } else {
                        SettingsRow("在键盘上预览", subtitle: "临时点亮不同会话状态灯以核对色彩层次", showsDivider: true) {
                            HStack(spacing: 6) {
                                Button("完成与空闲") { store.dispatch("session_strip", ["TriggerTestLighting": "completed"]) }
                                    .surfaceButton(.secondary)
                                Button("进行中") { store.dispatch("session_strip", ["TriggerTestLighting": "thinking"]) }
                                    .surfaceButton(.secondary)
                                Button("待确认") { store.dispatch("session_strip", ["TriggerTestLighting": "awaiting_input"]) }
                                    .surfaceButton(.secondary)
                                Button("恢复自动") { store.dispatch("session_strip", ["TriggerTestLighting": NSNull()]) }
                                    .surfaceButton(.ghost)
                            }
                        }
                        SettingsRow("未读完成灯状态", subtitle: "清除所有已完成会话的未读蓝灯提示") {
                            Button("全部标为已读") { store.dispatch("session_strip", "AcknowledgeAllCompleted") }
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
    }

    var body: some View {
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
        .help(isTaskRole ? (sessionSlot?.title ?? "尚未分配会话") : sublabel)
        .accessibilityLabel(isTaskRole
            ? "会话 \(slot + 1)，\(sessionSlot?.title ?? "尚未分配会话")，\(SessionKeyStatusView.label(for: sessionSlot))"
            : "按键 \(slot + 1)，\(sublabel.isEmpty ? "未绑定" : sublabel)")
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

        return SessionKeycapContent(slotIndex: slot, sessionSlot: sessionSlot, showLabels: showLabels)
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
        let canvas = store.canvas
        let sessions = store.sessionStrip
        let keys = canvas.keycapCells.filter {
            agentKind.supportsLiveSessionSlots && $0.slotRole == "TaskKey" && (0..<6).contains($0.primarySlot)
        }
        if !keys.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
                HStack(spacing: 10) {
                    Text("会话")
                        .font(UITheme.sectionHeader)
                        .foregroundStyle(UITheme.textPrimary)
                    if sessions.workingCount > 0 || sessions.unreadCompletedCount > 0 {
                        Text("进行中 \(sessions.workingCount) · 已完成 \(sessions.unreadCompletedCount)")
                            .font(UITheme.caption)
                            .foregroundStyle(UITheme.textSecondary)
                    }
                    Spacer()
                    if sessions.unreadCompletedCount > 0 {
                        Button("已读熄灯") {
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
                            Text("灯光筛选")
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
                                title: session?.title.isEmpty == false ? session!.title : "尚未分配会话",
                                sessionSlot: session,
                                isSelected: canvas.selectedSurface == key.surface,
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
                SessionKeyStatusView(sessionSlot: sessionSlot)
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
        .accessibilityLabel("会话 \(slotIndex + 1)，\(title)，\(SessionKeyStatusView.label(for: sessionSlot))")
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }
}

struct SessionKeycapContent: View {
    let slotIndex: Int
    let sessionSlot: SessionSlotModel?
    let showLabels: Bool

    var body: some View {
        VStack(spacing: 6) {
            Text("\(slotIndex + 1)")
                .font(UITheme.heading)
                .foregroundStyle(UITheme.textPrimary)
            if showLabels {
                SessionKeyStatusView(sessionSlot: sessionSlot)
                    .font(UITheme.caption)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct SessionKeyStatusView: View {
    let sessionSlot: SessionSlotModel?

    static func label(for slot: SessionSlotModel?) -> String {
        guard let slot else { return "未分配" }
        if slot.title.isEmpty && (slot.placeRaw == nil || slot.placeRaw is NSNull) { return "未分配" }
        switch slot.kind {
        case "Working": return "进行中"
        case "BlueDotCompleted": return "已完成"
        case "NeedsAttention": return "待回应"
        case "Error": return "执行异常"
        case "Idle": return "就绪"
        default: return "状态未知"
        }
    }

    var body: some View {
        Text(Self.label(for: sessionSlot))
            .foregroundStyle(sessionSlot?.isUnreadBlueDot == true ? UITheme.textPrimary : UITheme.textSecondary)
    }
}
