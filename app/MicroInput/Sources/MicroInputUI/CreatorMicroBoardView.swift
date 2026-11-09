import AppKit
import SwiftUI

struct CreatorMicroStageView: View {
    let store: CoreStore
    let model: ProjectionModel
    let rawProfileName: String
    let agentKind: AgentProfileKind
    var onOpenFullActionLibrary: () -> Void
    var onOpenMacroLibrary: () -> Void
    var onPromptNewLayer: () -> Void
    var onPromptRenameLayer: () -> Void

    var body: some View {
        let m = model
        let selectedRow = m.sidebarRows.first(where: { $0.isSelected })
        let isCurrentlyActive = selectedRow?.isActiveHost ?? false

        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text(m.selectedDisplayName)
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

                if m.layerTabs.count > 1 {
                    Picker(
                        "层",
                        selection: Binding(
                            get: { m.layerTabs.first(where: { $0.isSelected })?.id ?? "" },
                            set: { store.dispatch("canvas", ["SelectLayer": $0]) }
                        )
                    ) {
                        ForEach(m.layerTabs) { tab in
                            Text(tab.name).tag(tab.id)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 260)
                }

                Menu("图层") {
                    Button("新建层…") { onPromptNewLayer() }
                    Button("重命名当前层…") { onPromptRenameLayer() }
                        .disabled(m.isProtectedLayer)
                    Button("复制当前层") {
                        store.dispatch(
                            "inspector",
                            ["DuplicateSelectedLayer": ["name": "\(m.inspectorLayerName) 副本"]]
                        )
                    }
                    Divider()
                    Button("删除当前层", role: .destructive) {
                        store.dispatch("inspector", "DeleteSelectedLayer")
                    }
                    .disabled(m.isProtectedLayer || m.layerTabs.count <= 1)
                }
                .controlSize(.small)
                .fixedSize()

                if let agent = DesktopAgent.named(rawProfileName),
                   !agent.isGeneralFallback
                {
                    Button("打开 \(m.selectedDisplayName)") {
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

            if !m.deviceConnected || !m.hostExecutionEnabled || m.bannerKind == "Warning" {
                healthWarningBanner(m: m)
                Rectangle().fill(UITheme.border).frame(height: 1)
            }

            HStack(alignment: .top, spacing: 0) {
                CreatorMicroBoardView(store: store, model: m, agentKind: agentKind)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(UITheme.pageBackground)

                Rectangle().fill(UITheme.border).frame(width: 1)

                CreatorMicroInlineInspectorView(
                    store: store,
                    model: m,
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

    private func healthWarningBanner(m: ProjectionModel) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "exclamationmark.circle.fill")
                .foregroundStyle(UITheme.caution)
            Text(m.bannerText)
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textPrimary)
                .lineLimit(1)
            Spacer()
            if !m.deviceConnected {
                Button("重新检测键盘") {
                    store.dispatch("header", "ConnectKeyboard")
                }
                .buttonStyle(.borderedProminent)
                .tint(UITheme.accent)
                .controlSize(.small)
            }
            if !m.hostExecutionEnabled {
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
    let model: ProjectionModel
    let agentKind: AgentProfileKind
    @State private var jumpDirectlyOnGlowingSlotTap = true
    @State private var showLightingPopover = false

    private let cellSize: CGFloat = 68
    private let gridSpacing: CGFloat = 8

    var body: some View {
        let m = model
        VStack(spacing: 14) {
            Spacer(minLength: 8)
            physicalBoardChassis(m: m)
            compactBoardToolbar(m: m)
            Spacer(minLength: 8)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    private func physicalBoardChassis(m: ProjectionModel) -> some View {
        VStack(spacing: gridSpacing) {
            HStack(spacing: gridSpacing) {
                rotaryEncoderCell(m: m)
                programmableKeyCell(slot: 0, m: m)
                programmableKeyCell(slot: 1, m: m)
                analogJoystickCell(m: m)
            }
            HStack(spacing: gridSpacing) {
                ForEach(2..<6, id: \.self) { slot in
                    programmableKeyCell(slot: slot, m: m)
                }
            }
            HStack(spacing: gridSpacing) {
                ForEach(6..<10, id: \.self) { slot in
                    programmableKeyCell(slot: slot, m: m)
                }
            }
            HStack(spacing: gridSpacing) {
                statusIndicatorClusterCell(m: m)
                if m.bottomRowLayout == "split" {
                    programmableKeyCell(slot: 10, m: m)
                    programmableKeyCell(slot: 11, m: m)
                } else {
                    wideProgrammableKeyCell(m: m)
                }
                programmableKeyCell(slot: 12, m: m)
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

    private func rotaryEncoderCell(m: ProjectionModel) -> some View {
        let isSelected = m.selectedSurface.hasPrefix("Encoder") || (13...15).contains(m.selectedLogicalSlot)
        return Button {
            if !m.selectedSurface.hasPrefix("Encoder") {
                store.dispatch("canvas", ["SelectSurface": "EncoderCcw"])
            } else if m.selectedSurface == "EncoderCcw" {
                store.dispatch("canvas", ["SelectSurface": "EncoderCw"])
            } else if m.selectedSurface == "EncoderCw" {
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
            }
        }
        .buttonStyle(.plain)
        .help("无极金属旋钮（点击切换左旋 / 右旋 / 按下配置）")
    }

    private func analogJoystickCell(m: ProjectionModel) -> some View {
        let isSelected = m.selectedSurface == "Joystick" || m.joystickIsSelected
        return Button {
            store.dispatch("canvas", ["SelectSurface": "Joystick"])
            let dirs = ["North", "West", "South", "East"]
            if let idx = dirs.firstIndex(of: m.selectedJoystickDirection) {
                let nextDir = dirs[(idx + 1) % dirs.count]
                store.dispatch("canvas", ["SelectJoystickDirection": nextDir])
            } else {
                store.dispatch("canvas", ["SelectJoystickDirection": "North"])
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
            }
        }
        .buttonStyle(.plain)
        .help("8 向摇杆（点击切换上 / 左 / 下 / 右方向配置）")
    }

    private func statusIndicatorClusterCell(m: ProjectionModel) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(UITheme.mutedSurface.opacity(0.55))
                .frame(width: cellSize, height: cellSize)
                .overlay(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .stroke(UITheme.border, lineWidth: 1)
                )

            VStack(spacing: 8) {
                HStack(spacing: 6) {
                    Circle()
                        .fill(m.deviceConnected ? UITheme.accent : UITheme.strongBorder)
                        .frame(width: 4.5, height: 4.5)
                    Circle()
                        .fill(m.hostExecutionEnabled ? UITheme.accent : UITheme.strongBorder)
                        .frame(width: 4.5, height: 4.5)
                    Circle()
                        .fill(UITheme.strongBorder)
                        .frame(width: 4.5, height: 4.5)
                }
                Circle()
                    .fill(Color(red: 0.18, green: 0.18, blue: 0.20))
                    .frame(width: 32, height: 32)
            }
        }
    }

    private func programmableKeyCell(slot: Int, m: ProjectionModel) -> some View {
        let cell = m.keycapCells.first(where: { $0.primarySlot == slot })
        return keycapButton(slot: slot, width: cellSize, cell: cell, m: m)
    }

    private func wideProgrammableKeyCell(m: ProjectionModel) -> some View {
        let activeSlot = (m.selectedLogicalSlot == 11) ? 11 : 10
        let cell = m.keycapCells.first(where: { $0.primarySlot == 10 })
        return keycapButton(
            slot: activeSlot,
            width: cellSize * 2 + gridSpacing,
            cell: cell,
            m: m,
            forceSelected: m.selectedLogicalSlot == 10 || m.selectedLogicalSlot == 11
        )
    }

    private func keycapButton(
        slot: Int,
        width: CGFloat,
        cell: KeycapCellModel?,
        m: ProjectionModel,
        forceSelected: Bool = false
    ) -> some View {
        let isSelected = forceSelected || (cell?.isSelected == true) || (m.selectedLogicalSlot == slot && m.selectedSurface.hasPrefix("Key"))
        let isTaskRole = (slot < 6) && (cell?.slotRole == "TaskKey") && agentKind.supportsLiveSessionSlots
        let sessionSlot = (slot < 6) ? m.sessionSlots.first(where: { $0.slotIndex == slot }) : nil
        let keycap = CodexKeycapId(rustVariant: cell?.keycapId ?? "Apps")
        let sublabel = cell?.conciseCaption ?? "空白"

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
            showLabels: m.showKeycapLabels,
            sublabel: sublabel,
            jumpDirectlyOnGlowingSlotTap: jumpDirectlyOnGlowingSlotTap
        )
        .equatable()
    }

    private func compactBoardToolbar(m: ProjectionModel) -> some View {
        let activeSessions = m.sessionSlots.filter {
            $0.isUnreadBlueDot || $0.isWorkingAnimated || !$0.title.isEmpty
        }

        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                Toggle(
                    "显示键帽文字",
                    isOn: Binding(
                        get: { m.showKeycapLabels },
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
                    HStack(spacing: 6) {
                        Circle()
                            .fill(
                                m.simpleLighting.lightingEnabled
                                    ? Color(hex: m.statusLedHex)
                                    : UITheme.textMuted
                            )
                            .frame(width: 7, height: 7)
                        Text("调整灯光")
                            .font(UITheme.captionMedium)
                            .foregroundStyle(UITheme.textPrimary)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(Capsule().fill(UITheme.surface))
                    .overlay(Capsule().stroke(UITheme.border, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .popover(isPresented: $showLightingPopover, arrowEdge: .bottom) {
                    SimpleLightingControlCard(store: store)
                        .frame(width: 340)
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
    @ObservedObject var store: CoreStore

    var body: some View {
        let lp = store.model.simpleLighting
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("键盘灯光与会话提醒")
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

            if lp.lightingEnabled {
                Toggle(
                    "随软件与会话状态自动变色",
                    isOn: Binding(
                        get: { lp.autoSessionColor },
                        set: { store.dispatch("session_strip", ["SetAutoSessionColor": $0]) }
                    )
                )
                .font(UITheme.caption)

                Toggle(
                    "仅在待确认或刚完成时亮起",
                    isOn: Binding(
                        get: { lp.onlyGlowOnAttentionOrCompleted },
                        set: { store.dispatch("session_strip", ["SetOnlyGlowOnAttentionOrCompleted": $0]) }
                    )
                )
                .font(UITheme.caption)

                Toggle(
                    "后台运行中呼吸提示",
                    isOn: Binding(
                        get: { lp.glowOnWorking },
                        set: { store.dispatch("session_strip", ["SetGlowOnWorking": $0]) }
                    )
                )
                .font(UITheme.caption)

                Divider()

                Picker(
                    "常驻氛围色",
                    selection: Binding(
                        get: { lp.ambientColor },
                        set: { store.dispatch("session_strip", ["SetAmbientColor": $0]) }
                    )
                ) {
                    Text("纯白").tag("PureWhite")
                    Text("暖琥珀").tag("WarmAmber")
                    Text("冰川蓝").tag("IceBlue")
                    Text("翡翠绿").tag("EmeraldGreen")
                }
                .controlSize(.small)

                Picker(
                    "整体亮度",
                    selection: Binding(
                        get: { lp.brightness },
                        set: { store.dispatch("session_strip", ["SetBrightnessPreset": $0]) }
                    )
                ) {
                    Text("柔和 35%").tag("Soft")
                    Text("标准 70%").tag("Standard")
                    Text("高亮 100%").tag("High")
                }
                .controlSize(.small)

                Divider()

                HStack {
                    Button("全部已读熄灯") {
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
            if isTaskRole {
                taskKeycapSurface
            } else {
                actionKeycapSurface
            }
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
                    keycap: .apps,
                    size: 20,
                    color: UITheme.textSecondary
                )
                .equatable()
            }
            Spacer(minLength: 2)
            if showLabels {
                Text(sessionSlot?.title.isEmpty == false ? sessionSlot!.title : "会话 \(slot + 1)")
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
    }
}
