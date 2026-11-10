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
                VStack(alignment: .leading, spacing: 8) {
                    Text("当前控制").font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                    Text(h.hostExecutionEnabled ? store.statusMenu.activeProfileName : "按键控制已暂停")
                        .font(UITheme.title).foregroundStyle(UITheme.textPrimary)
                    Text(s.boardLayers.observedStatus)
                        .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                }
                Divider()
                BoardLayerOverview(store: store, board: s.boardLayers)
                if h.bannerKind != "Ready" { healthWarningBanner(h: h) }
                if s.selectedProfileId != nil {
                    HStack(alignment: .top, spacing: 16) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(s.selectedDisplayName).font(UITheme.heading)
                            Text("正在编辑，\(s.inspectorLayerName)")
                                .font(UITheme.caption).foregroundStyle(UITheme.textSecondary)
                        }
                        Spacer(minLength: 0)
                        Menu("按键层") {
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
                        }
                        .fixedSize()
                        if let profileId = s.selectedProfileId, profileId != s.activeHostProfileId {
                            Button("启用此方案") { store.dispatch("sidebar", ["SelectProfile": profileId]) }
                        }
                        if let agent = DesktopAgent.named(s.selectedRawName), !agent.isGeneralFallback {
                            Button("打开应用") {
                                if let url = DesktopAgent.cachedAppURL(for: agent.bundleID) {
                                    NSWorkspace.shared.openApplication(at: url, configuration: NSWorkspace.OpenConfiguration())
                                }
                            }
                        }
                    }
                    CreatorMicroBoardView(store: store, agentKind: agentKind)
                        .frame(maxWidth: .infinity).frame(height: 420)
                        .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: 12))
                    CreatorMicroInlineInspectorView(
                        store: store, agentKind: agentKind,
                        onOpenFullActionLibrary: onOpenFullActionLibrary,
                        onOpenMacroLibrary: onOpenMacroLibrary
                    )
                    .frame(minHeight: 400, idealHeight: 540, maxHeight: 540)
                } else {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("这一层尚未分配应用").font(UITheme.heading)
                        Text("在应用方案中分配后，即可查看对应按键。")
                            .foregroundStyle(UITheme.textSecondary)
                    }
                    .padding(.vertical, 32)
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
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(UITheme.caution)
            Text(h.bannerText)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textPrimary)
                .fixedSize(horizontal: false, vertical: true)
            Spacer()
            if !h.deviceConnected {
                Button("重新检测键盘") {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .buttonStyle(.borderedProminent)
                .tint(UITheme.accent)
                .controlSize(.small)
            }
            if !h.hostExecutionEnabled {
                Button("开启按键控制") {
                    store.dispatch("header", ["SetExecutionEnabled": true])
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 8)
        .background(UITheme.cautionSoft)
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
        .background(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(UITheme.surface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .stroke(UITheme.strongBorder, lineWidth: 1.5)
        )
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
                            colors: [
                                Color(red: 0.25, green: 0.25, blue: 0.27),
                                Color(red: 0.14, green: 0.14, blue: 0.15)
                            ],
                            startPoint: .topLeading,
                            endPoint: .bottomTrailing
                        )
                    )
                    .frame(width: cellSize, height: cellSize)
                    .overlay(
                        Circle()
                            .stroke(
                                isSelected ? UITheme.accent : Color.black.opacity(0.35),
                                lineWidth: isSelected ? 2.5 : 1
                            )
                    )

                VStack(spacing: 0) {
                    Capsule()
                        .fill(Color.white.opacity(0.92))
                        .frame(width: 2.2, height: 12)
                        .padding(.top, 7)
                    Spacer()
                }
                .frame(width: cellSize, height: cellSize)
                .rotationEffect(
                    .degrees(
                        c.selectedSurface == "EncoderCcw"
                            ? -32
                            : (c.selectedSurface == "EncoderCw" ? 32 : 0)
                    )
                )

                if isSelected {
                    Text(
                        c.selectedSurface == "EncoderCcw"
                            ? "左旋"
                            : (c.selectedSurface == "EncoderCw" ? "右旋" : "按下")
                    )
                    .font(UITheme.micro)
                    .foregroundStyle(Color.white.opacity(0.92))
                    .padding(.top, 14)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
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
                store.dispatch(
                    "canvas",
                    ["SelectJoystickDirection": activeDir.isEmpty ? "North" : activeDir]
                )
            } else {
                let dirs = ["North", "West", "South", "East"]
                if let idx = dirs.firstIndex(of: activeDir) {
                    let nextDir = dirs[(idx + 1) % dirs.count]
                    store.dispatch("canvas", ["SelectJoystickDirection": nextDir])
                } else {
                    store.dispatch("canvas", ["SelectJoystickDirection": "North"])
                }
            }
        } label: {
            ZStack {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(UITheme.mutedSurface)
                    .frame(width: cellSize, height: cellSize)
                    .overlay(
                        RoundedRectangle(cornerRadius: 12, style: .continuous)
                            .stroke(
                                isSelected ? UITheme.accent : UITheme.border,
                                lineWidth: isSelected ? 2.5 : 1
                            )
                    )

                ZStack {
                    Circle()
                        .fill(
                            LinearGradient(
                                colors: [
                                    Color(red: 0.24, green: 0.24, blue: 0.26),
                                    Color(red: 0.13, green: 0.13, blue: 0.15)
                                ],
                                startPoint: .topLeading,
                                endPoint: .bottomTrailing
                            )
                        )
                        .frame(width: 42, height: 42)
                        .overlay(
                            Circle().stroke(Color.black.opacity(0.35), lineWidth: 1)
                        )
                    if isSelected {
                        Text(dirArrow)
                            .font(UITheme.captionMedium)
                            .foregroundStyle(Color.white.opacity(0.92))
                    }
                }
                .offset(joyOffset)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("8 向摇杆（点击切换上 / 左 / 下 / 右方向配置）")
    }

    private func statusIndicatorClusterCell(c: CanvasSliceModel) -> some View {
        let board = store.sidebar.boardLayers
        let matchesLayer = board.layers.contains {
            $0.layer == c.boardPreviewLayer && $0.profileId == c.selectedProfileId
        }
        let matchesUsb = c.boardPreviewLayer == nil && board.usbStartupProfileId == c.selectedProfileId
        let isBoardPreview = c.selectedProfileId != nil && (matchesLayer || matchesUsb)
        let previewLabel = isBoardPreview ? c.boardPreviewLabel : "本机方案，没有选择物理层"
        let leds = isBoardPreview && c.touchLedStates.count == 3 ? c.touchLedStates : [false, false, false]
        return VStack(spacing: 6) {
            HStack(spacing: 6) {
                ForEach(0..<3, id: \.self) { index in
                    Circle()
                        .fill(leds[index] ? UITheme.accent : UITheme.strongBorder)
                        .frame(width: 5.5, height: 5.5)
                }
            }
            Text(isBoardPreview ? (c.boardPreviewLayer.map { "L\($0)" } ?? "USB") : "本机")
                .font(UITheme.monoBadge)
                .foregroundStyle(UITheme.textPrimary)
            Text(isBoardPreview ? "预览" : "方案").font(UITheme.micro).foregroundStyle(UITheme.textMuted)
        }
        .frame(width: cellSize, height: cellSize)
        .background(RoundedRectangle(cornerRadius: 12).fill(UITheme.mutedSurface.opacity(0.55)))
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
        let sublabel: String = {
            if c.protectedLock != nil {
                return "\(c.selectedDisplayName) 直通"
            }
            return cell?.conciseCaption ?? "空白"
        }()

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
            Toggle("显示键帽文字", isOn: Binding(
                get: { c.showKeycapLabels },
                set: { store.dispatch("canvas", ["SetShowKeycapLabels": $0]) }
            ))
            .toggleStyle(.checkbox)
            .font(UITheme.caption)
            Spacer(minLength: 0)
            Menu("布局选项") {
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
            }
            .fixedSize()
        }
        .foregroundStyle(UITheme.textSecondary)
    }

}

struct SimpleLightingControlCard: View {
    let store: CoreStore

    var body: some View {
        let ss = store.sessionStrip
        let lp = ss.simpleLighting
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("会话灯光")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Toggle(
                    "开启灯光",
                    isOn: Binding(
                        get: { lp.lightingEnabled },
                        set: { store.dispatch("session_strip", ["SetLightingEnabled": $0]) }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
            }

            HStack(spacing: 8) {
                Circle()
                    .fill(Color(hex: "#0088FF"))
                    .frame(width: 7, height: 7)
                Text("未读 \(ss.unreadCompletedCount)")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)
                Text("·")
                    .foregroundStyle(UITheme.textMuted)
                Circle()
                    .fill(Color(hex: "#00FF66"))
                    .frame(width: 7, height: 7)
                Text("进行中 \(ss.workingCount)")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)
            }

            if lp.lightingEnabled {
                Toggle(
                    "随会话状态变色",
                    isOn: Binding(
                        get: { lp.autoSessionColor },
                        set: { store.dispatch("session_strip", ["SetAutoSessionColor": $0]) }
                    )
                )
                .font(UITheme.caption)

                Toggle(
                    "进行中的会话使用呼吸灯",
                    isOn: Binding(
                        get: { lp.glowOnWorking },
                        set: { store.dispatch("session_strip", ["SetGlowOnWorking": $0]) }
                    )
                )
                .font(UITheme.caption)

                Toggle(
                    "仅在有新完成或进行中的会话时亮起",
                    isOn: Binding(
                        get: { lp.onlyGlowOnAttentionOrCompleted },
                        set: { store.dispatch("session_strip", ["SetOnlyGlowOnAttentionOrCompleted": $0]) }
                    )
                )
                .font(UITheme.caption)

                Divider()

                Picker(
                    "完成与空闲时的颜色",
                    selection: Binding(
                        get: { lp.ambientColor },
                        set: { store.dispatch("session_strip", ["SetAmbientColor": $0]) }
                    )
                ) {
                    Text("海洋蓝 (默认蓝光)").tag("OceanBlue")
                    Text("青碧").tag("CyanTeal")
                    Text("纯白").tag("PureWhite")
                    Text("翡翠绿").tag("EmeraldGreen")
                    Text("琥珀金").tag("AmberGold")
                }
                .controlSize(.small)

                Picker(
                    "进行中时的颜色",
                    selection: Binding(
                        get: { lp.workingColor },
                        set: { store.dispatch("session_strip", ["SetWorkingColor": $0]) }
                    )
                ) {
                    Text("翡翠绿 (默认)").tag("EmeraldGreen")
                    Text("琥珀金").tag("AmberGold")
                    Text("青碧").tag("CyanTeal")
                    Text("纯白").tag("PureWhite")
                    Text("海洋蓝").tag("OceanBlue")
                }
                .controlSize(.small)

                Picker(
                    "整体亮度",
                    selection: Binding(
                        get: { lp.brightness },
                        set: { store.dispatch("session_strip", ["SetBrightnessPreset": $0]) }
                    )
                ) {
                    Text("柔和 (35%)").tag("Soft")
                    Text("标准 (65%)").tag("Standard")
                    Text("明亮 (95%)").tag("Bright")
                }
                .controlSize(.small)

                Picker(
                    "自动熄灯",
                    selection: Binding(
                        get: { lp.autoOffTimeout },
                        set: { store.dispatch("session_strip", ["SetAutoOffTimeout": $0]) }
                    )
                ) {
                    Text("1 分钟").tag("OneMinute")
                    Text("3 分钟").tag("ThreeMinutes")
                    Text("5 分钟").tag("FiveMinutes")
                    Text("常亮").tag("Never")
                }
                .controlSize(.small)

                Divider()

                VStack(alignment: .leading, spacing: 6) {
                    Text("在键盘上预览")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textSecondary)
                    HStack(spacing: 6) {
                        Button("完成与空闲") {
                            store.dispatch("session_strip", ["TriggerTestLighting": "completed"])
                        }
                        .controlSize(.mini)
                        Button("进行中") {
                            store.dispatch("session_strip", ["TriggerTestLighting": "thinking"])
                        }
                        .controlSize(.mini)
                        Button("待确认") {
                            store.dispatch("session_strip", ["TriggerTestLighting": "awaiting_input"])
                        }
                        .controlSize(.mini)
                        Button("恢复自动") {
                            store.dispatch("session_strip", ["TriggerTestLighting": NSNull()])
                        }
                        .controlSize(.mini)
                    }
                }

                Divider()

                HStack {
                    Button("全部标为已读") {
                        store.dispatch("session_strip", "AcknowledgeAllCompleted")
                    }
                    .controlSize(.small)
                    Spacer()
                    Button("编辑层灯光") {
                        store.dispatch("editor", "OpenLightingEditor")
                    }
                    .controlSize(.small)
                    .disabled(!store.canvas.canAddLighting)
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
        .accessibilityLabel("按键 \(slot + 1)，\(sublabel.isEmpty ? "未绑定" : sublabel)")
    }

    private var taskKeycapSurface: some View {
        let status = sessionSlot?.status ?? "Idle"
        let hasActiveState = (sessionSlot?.isUnreadBlueDot == true)
            || (sessionSlot?.isWorkingAnimated == true)
            || (status == "Attention")
        let ledColor = Color(hex: sessionSlot?.ledColorHex ?? "#36526D")

        let fillSurface: Color = {
            if sessionSlot?.isUnreadBlueDot == true || status == "Attention" {
                return UITheme.accentSoft
            }
            return UITheme.subtleSurface
        }()

        let borderColor: Color = {
            if isSelected { return UITheme.accent }
            if hasActiveState { return ledColor.opacity(0.85) }
            return UITheme.strongBorder
        }()

        return VStack(spacing: 4) {
            Spacer(minLength: 2)
            if sessionSlot?.isWorkingAnimated == true || status == "Working" {
                HStack(spacing: 4) {
                    Text("::")
                        .font(UITheme.captionMedium)
                        .foregroundStyle(UITheme.textSecondary)
                    Text("进行中")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textPrimary)
                }
            } else if sessionSlot?.isUnreadBlueDot == true || status == "Completed" {
                HStack(spacing: 4) {
                    Circle().fill(ledColor).frame(width: 7, height: 7)
                    Text("已完成")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.accent)
                }
            } else if status == "Attention" {
                HStack(spacing: 4) {
                    Circle().fill(ledColor).frame(width: 7, height: 7)
                    Text("待回应")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.caution)
                }
            } else {
                CodexKeycapGlyphView(
                    keycap: keycap == .blank ? .apps : keycap,
                    size: 20,
                    color: UITheme.textSecondary
                )
                .equatable()
            }
            Spacer(minLength: 2)
            if showLabels {
                let fallbackLabel = (sublabel != "空白" && !sublabel.isEmpty) ? sublabel : "会话 \(slot + 1)"
                Text(sessionSlot?.title.isEmpty == false ? sessionSlot!.title : fallbackLabel)
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 6)
        .frame(width: width, height: cellSize)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(fillSurface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(borderColor, lineWidth: isSelected ? 2.5 : (hasActiveState ? 2.0 : 1.0))
        )
        .contentShape(Rectangle())
    }

    private var actionKeycapSurface: some View {
        let keySurface = UITheme.surface
        return VStack(spacing: 4) {
            Spacer(minLength: 2)
            CodexKeycapGlyphView(
                keycap: keycap,
                size: showLabels ? 20 : 23,
                color: UITheme.textPrimary
            )
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
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(keySurface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(
                    isSelected ? UITheme.accent : Color.black.opacity(0.45),
                    lineWidth: isSelected ? 2.5 : 1.0
                )
        )
        .contentShape(Rectangle())
    }
}
