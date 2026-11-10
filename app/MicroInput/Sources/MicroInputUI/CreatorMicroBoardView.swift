import AppKit
import SwiftUI

struct CreatorMicroStageView: View {
    let store: CoreStore
    var onOpenFullActionLibrary: () -> Void
    var onOpenMacroLibrary: () -> Void
    var onPromptNewLayer: () -> Void
    var onPromptRenameLayer: () -> Void
    @State private var showStageLightingPopover = false

    var body: some View {
        let _ = ViewRenderTelemetry.record("CreatorMicroStageView")
        let s = store.sidebar
        let h = store.header
        let rawProfileName = s.selectedRawName
        let agentKind = AgentProfileKind.from(profileName: rawProfileName)
        let selectedRow = s.sidebarRows.first(where: { $0.isSelected })
        let isCurrentlyActive = selectedRow?.isActiveHost ?? false

        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(s.selectedDisplayName)
                            .font(UITheme.title)
                            .foregroundStyle(UITheme.textPrimary)
                        if isCurrentlyActive {
                            Text("当前生效")
                                .font(UITheme.micro)
                                .padding(.horizontal, 7)
                                .padding(.vertical, 2)
                                .background(Capsule().fill(UITheme.accentSoft))
                                .foregroundStyle(UITheme.accent)
                        } else {
                            Button("启用此方案") {
                                if let id = selectedRow?.profileId {
                                    store.dispatch("sidebar", ["SelectProfile": id])
                                }
                            }
                            .buttonStyle(.borderedProminent)
                            .tint(UITheme.accent)
                            .controlSize(.mini)
                        }
                    }
                    Text(agentKind.bridgeDescription)
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)
                        .lineLimit(1)
                }

                Spacer()

                if s.layerTabs.count > 1 {
                    Picker(
                        "层",
                        selection: Binding(
                            get: { s.layerTabs.first(where: { $0.isSelected })?.id ?? "" },
                            set: { store.dispatch("canvas", ["SelectLayer": $0]) }
                        )
                    ) {
                        ForEach(s.layerTabs) { tab in
                            Text(tab.name).tag(tab.id)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 260)
                }

                Menu("图层") {
                    Button("新建层…") { onPromptNewLayer() }
                    Button("重命名当前层…") { onPromptRenameLayer() }
                        .disabled(s.isProtectedLayer)
                    Button("复制当前层") {
                        store.dispatch(
                            "inspector",
                            ["DuplicateSelectedLayer": ["name": "\(s.inspectorLayerName) 副本"]]
                        )
                    }
                    Divider()
                    Button("删除当前层", role: .destructive) {
                        store.dispatch("inspector", "DeleteSelectedLayer")
                    }
                    .disabled(s.isProtectedLayer || s.layerTabs.count <= 1)
                }
                .controlSize(.small)
                .fixedSize()

                Button("💡 灯光设置") {
                    showStageLightingPopover.toggle()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                .popover(isPresented: $showStageLightingPopover, arrowEdge: .bottom) {
                    SimpleLightingControlCard(store: store)
                        .frame(width: 360)
                        .padding(14)
                }

                if let agent = DesktopAgent.named(rawProfileName),
                   !agent.isGeneralFallback
                {
                    Button("打开 \(s.selectedDisplayName)") {
                        if let appURL = DesktopAgent.cachedAppURL(for: agent.bundleID) {
                            NSWorkspace.shared.openApplication(
                                at: appURL,
                                configuration: NSWorkspace.OpenConfiguration()
                            )
                        }
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .background(UITheme.surface)

            Rectangle().fill(UITheme.border).frame(height: 1)

            if !h.deviceConnected || !h.hostExecutionEnabled || h.bannerKind == "Warning" {
                healthWarningBanner(h: h)
                Rectangle().fill(UITheme.border).frame(height: 1)
            }

            HStack(alignment: .top, spacing: 0) {
                CreatorMicroBoardView(store: store, agentKind: agentKind)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(UITheme.pageBackground)

                Rectangle().fill(UITheme.border).frame(width: 1)

                CreatorMicroInlineInspectorView(
                    store: store,
                    agentKind: agentKind,
                    onOpenFullActionLibrary: onOpenFullActionLibrary,
                    onOpenMacroLibrary: onOpenMacroLibrary
                )
                .frame(width: 332)
                .frame(maxHeight: .infinity)
                .background(UITheme.surface)
            }
        }
    }

    private func healthWarningBanner(h: HeaderSliceModel) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(UITheme.caution)
            Text(h.bannerText)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textPrimary)
                .lineLimit(1)
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
    @State private var showLightingPopover = false

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
                            .font(.system(size: 12, weight: .bold, design: .rounded))
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
        let leds = c.touchLedStates.count == 3 ? c.touchLedStates : [true, false, false]
        let slotBadge = "L\(max(c.activeHardwareSlot, 1))"
        let managed = store.sidebar.boardProfileManaged
        return ZStack {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(UITheme.mutedSurface.opacity(0.55))
                .frame(width: cellSize, height: cellSize)
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .stroke(
                            c.activeHardwareSlot > 0 ? UITheme.accent.opacity(0.65) : UITheme.border,
                            lineWidth: 1
                        )
                )

            VStack(spacing: 6) {
                HStack(spacing: 6) {
                    ForEach(0..<3, id: \.self) { idx in
                        Button {
                            store.dispatch("canvas", ["ActivateHardwareSlot": idx + 1])
                        } label: {
                            Circle()
                                .fill(leds[idx] ? UITheme.accent : UITheme.strongBorder)
                                .frame(width: 5.5, height: 5.5)
                                .padding(3)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(managed)
                        .help(managed ? "用键盘左下角换层" : "预览第 \(idx + 1) 层")
                    }
                }
                Button {
                    store.dispatch("canvas", "CycleTouchProfileSlot")
                } label: {
                    ZStack {
                        Circle()
                            .fill(Color(red: 0.18, green: 0.18, blue: 0.20))
                            .frame(width: 34, height: 34)
                            .overlay(
                                Circle()
                                    .stroke(
                                        c.activeHardwareSlot > 0 ? UITheme.accent : UITheme.strongBorder,
                                        lineWidth: 1.2
                                    )
                            )
                        Text(slotBadge)
                            .font(UITheme.monoBadge)
                            .foregroundStyle(UITheme.textPrimary)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(managed)
                .help(managed ? c.activeHardwareSlotLabel : "预览三个板载层的轮换顺序")
            }
        }
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
        let activeSessions = ss.sessionSlots.filter {
            $0.isUnreadBlueDot || $0.isWorkingAnimated || !$0.title.isEmpty
        }

        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                Toggle(
                    "显示键帽文字",
                    isOn: Binding(
                        get: { c.showKeycapLabels },
                        set: { store.dispatch("canvas", ["SetShowKeycapLabels": $0]) }
                    )
                )
                .toggleStyle(.checkbox)
                .controlSize(.small)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)

                if agentKind.supportsLiveSessionSlots {
                    Toggle("点击发光键直接跳转会话", isOn: $jumpDirectlyOnGlowingSlotTap)
                        .toggleStyle(.checkbox)
                        .controlSize(.small)
                        .font(UITheme.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }

                Spacer()

                if !activeSessions.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(activeSessions.prefix(3)) { s in
                            Button {
                                store.dispatch("session_strip", ["ActivateSlot": s.slotIndex])
                            } label: {
                                HStack(spacing: 4) {
                                    Circle()
                                        .fill(Color(hex: s.ledColorHex))
                                        .frame(width: 6, height: 6)
                                    Text(s.title.isEmpty ? "会话 \(s.slotIndex + 1)" : s.title)
                                        .font(UITheme.micro)
                                        .foregroundStyle(UITheme.textPrimary)
                                        .lineLimit(1)
                                }
                                .padding(.horizontal, 7)
                                .padding(.vertical, 3)
                                .background(Capsule().fill(UITheme.subtleSurface))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }

                        Button("已读熄灯") {
                            store.dispatch("session_strip", "AcknowledgeAllCompleted")
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.mini)
                    }
                }

                Button {
                    showLightingPopover.toggle()
                } label: {
                    let statusLabel: String = {
                        if !ss.simpleLighting.lightingEnabled {
                            return "灯光已关"
                        }
                        if ss.unreadCompletedCount > 0 {
                            return "💡 灯光设置 · Idle 蓝光 (\(ss.unreadCompletedCount))"
                        }
                        if ss.workingCount > 0 {
                            return "💡 灯光设置 · In Progress (\(ss.workingCount))"
                        }
                        return "💡 灯光设置 · Idle 蓝光"
                    }()
                    HStack(spacing: 6) {
                        Circle()
                            .fill(
                                ss.simpleLighting.lightingEnabled
                                    ? Color(hex: c.underglowHex)
                                    : UITheme.textMuted
                            )
                            .frame(width: 8, height: 8)
                        Text(statusLabel)
                            .font(UITheme.captionMedium)
                            .foregroundStyle(UITheme.textPrimary)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(Capsule().fill(UITheme.surface))
                    .overlay(Capsule().stroke(UITheme.border, lineWidth: 1))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .popover(isPresented: $showLightingPopover, arrowEdge: .bottom) {
                    SimpleLightingControlCard(store: store)
                        .frame(width: 360)
                        .padding(14)
                }
            }

            HStack(spacing: 8) {
                if agentKind.supportsLiveSessionSlots {
                    Button("上方 6 键设为会话键") {
                        store.dispatch("session_strip", "BindTopSixAsSessionKeys")
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }

                Button("恢复默认布局") {
                    store.dispatch("session_strip", "RestoreDefaultLayout")
                }
                .buttonStyle(.bordered)
                .controlSize(.small)

                if agentKind == .antigravity {
                    Button("左旋钮切 Working/蓝点 + 右摇杆滚对话") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "CycleWorkingAndBlueDot"])
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)

                    Button("左旋钮滚项目 + 右摇杆滚对话") {
                        store.dispatch("canvas", ["ApplyKnobJoystickPreset": "DualScrollProjectsAndChat"])
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(
            RoundedRectangle(cornerRadius: UITheme.cardCornerRadius, style: .continuous)
                .fill(UITheme.surface)
        )
        .overlay(
            RoundedRectangle(cornerRadius: UITheme.cardCornerRadius, style: .continuous)
                .stroke(UITheme.border, lineWidth: 1)
        )
    }
}

struct SimpleLightingControlCard: View {
    let store: CoreStore

    var body: some View {
        let ss = store.sessionStrip
        let lp = ss.simpleLighting
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Antigravity 灯光设置 (Idle / In Progress)")
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
                Text("Idle 蓝光 (未读 \(ss.unreadCompletedCount))")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)
                Text("·")
                    .foregroundStyle(UITheme.textMuted)
                Circle()
                    .fill(Color(hex: "#00FF66"))
                    .frame(width: 7, height: 7)
                Text("In Progress 进行中 (\(ss.workingCount))")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)
            }

            if lp.lightingEnabled {
                Toggle(
                    "随 Antigravity 会话状态自动变色 (Idle 蓝光 / In Progress 变色)",
                    isOn: Binding(
                        get: { lp.autoSessionColor },
                        set: { store.dispatch("session_strip", ["SetAutoSessionColor": $0]) }
                    )
                )
                .font(UITheme.caption)

                Toggle(
                    "In Progress（进行中）对话呼吸亮灯提示",
                    isOn: Binding(
                        get: { lp.glowOnWorking },
                        set: { store.dispatch("session_strip", ["SetGlowOnWorking": $0]) }
                    )
                )
                .font(UITheme.caption)

                Toggle(
                    "仅在新完成或进行中时亮起（关闭则 Idle 常亮蓝光）",
                    isOn: Binding(
                        get: { lp.onlyGlowOnAttentionOrCompleted },
                        set: { store.dispatch("session_strip", ["SetOnlyGlowOnAttentionOrCompleted": $0]) }
                    )
                )
                .font(UITheme.caption)

                Divider()

                Picker(
                    "Idle（完成/空闲）光色",
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
                    "In Progress（进行中）光色",
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
                    Text("一键试灯（立即点亮实体键盘）")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textSecondary)
                    HStack(spacing: 6) {
                        Button("Idle 蓝光") {
                            store.dispatch("session_strip", ["TriggerTestLighting": "completed"])
                        }
                        .controlSize(.mini)
                        Button("In Progress 光") {
                            store.dispatch("session_strip", ["TriggerTestLighting": "thinking"])
                        }
                        .controlSize(.mini)
                        Button("待确认黄光") {
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
                    Button("双区 RGB 高级设置…") {
                        store.dispatch("editor", "OpenLightingEditor")
                    }
                    .controlSize(.small)
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
                        .font(.system(size: 12, weight: .bold, design: .monospaced))
                        .foregroundStyle(UITheme.textSecondary)
                    Text("Working")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textPrimary)
                }
            } else if sessionSlot?.isUnreadBlueDot == true || status == "Completed" {
                HStack(spacing: 4) {
                    Circle().fill(ledColor).frame(width: 7, height: 7)
                    Text("● 蓝点完成")
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
                    .lineLimit(1)
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
        let darkCharcoal = Color(red: 0.165, green: 0.17, blue: 0.19)
        return VStack(spacing: 4) {
            Spacer(minLength: 2)
            CodexKeycapGlyphView(
                keycap: keycap,
                size: showLabels ? 20 : 23,
                color: Color.white.opacity(0.95)
            )
            .equatable()
            Spacer(minLength: 2)
            if showLabels {
                Text(sublabel)
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(Color.white.opacity(0.85))
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 6)
        .padding(.vertical, 6)
        .frame(width: width, height: cellSize)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(darkCharcoal)
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
