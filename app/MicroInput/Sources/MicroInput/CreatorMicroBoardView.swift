import SwiftUI

@MainActor
final class KeycapVisualPreferences: ObservableObject {
    static let shared = KeycapVisualPreferences()

    @Published private var roleOverrides: [String: KeycapRole] = [:]
    @Published private var glyphOverrides: [String: CodexKeycapId] = [:]
    @Published var singleTapFocusWindow: Bool = true
    @Published var showKeycapSublabels: Bool = true
    @Published var autoDimMinutes: Int = 3

    private func storageKey(profileId: String, slot: Int) -> String {
        "\(profileId):\(slot)"
    }

    func role(
        for profileId: String,
        slot: Int,
        agent: AgentProfileKind,
        action: CatalogAction?,
        rawBindingLabel: String = "",
        isProtectedLayer: Bool = false
    ) -> KeycapRole {
        guard (0..<6).contains(slot) && agent.supportsLiveSessionSlots else {
            return .actionKey
        }
        if isProtectedLayer {
            return .taskKey
        }
        if action != nil {
            return KeycapPresentationResolver.resolveRole(
                slot: slot,
                agent: agent,
                boundAction: action,
                rawBindingLabel: rawBindingLabel,
                isProtectedLayer: isProtectedLayer
            )
        }
        let key = storageKey(profileId: profileId, slot: slot)
        if let explicit = roleOverrides[key] {
            return explicit
        }
        return KeycapPresentationResolver.resolveRole(
            slot: slot,
            agent: agent,
            boundAction: nil,
            rawBindingLabel: rawBindingLabel,
            isProtectedLayer: isProtectedLayer
        )
    }

    func setRole(_ role: KeycapRole, for profileId: String, slot: Int) {
        roleOverrides[storageKey(profileId: profileId, slot: slot)] = role
    }

    func clearRoleOverride(for profileId: String, slot: Int) {
        roleOverrides.removeValue(forKey: storageKey(profileId: profileId, slot: slot))
    }

    func glyph(for profileId: String, slot: Int, agent: AgentProfileKind, action: CatalogAction?) -> CodexKeycapId {
        let key = storageKey(profileId: profileId, slot: slot)
        if let explicit = glyphOverrides[key] {
            return explicit
        }
        return CodexKeycapCatalog.inferKeycap(slot: slot, action: action, agent: agent)
    }

    func setGlyph(_ glyph: CodexKeycapId, for profileId: String, slot: Int) {
        glyphOverrides[storageKey(profileId: profileId, slot: slot)] = glyph
    }

    func resetProfile(_ profileId: String) {
        let prefix = "\(profileId):"
        roleOverrides = roleOverrides.filter { !$0.key.hasPrefix(prefix) }
        glyphOverrides = glyphOverrides.filter { !$0.key.hasPrefix(prefix) }
    }
}

struct CreatorMicroStageView: View {
    @ObservedObject var store: Store
    @ObservedObject var prefs = KeycapVisualPreferences.shared
    let agentKind: AgentProfileKind
    let actionForSlot: (Int) -> CatalogAction?
    let labelForSlot: (Int) -> String
    let titleForSlot: (Int) -> String
    let onOpenFullActionLibrary: () -> Void
    let onOpenMacroLibrary: () -> Void

    private var profileDisplayName: String {
        DesktopAgent.displayName(store.profile?.name ?? (store.snapshot == nil ? "正在载入" : "暂无方案"))
    }

    private var needsHealthBanner: Bool {
        store.connectedID == nil
            || !store.accessibilityTrusted
            || !store.hostExecutionEnabled
            || !store.nativeOwnership.allowsExecution
    }

    var body: some View {
        VStack(spacing: 0) {
            stageHeaderBar
            Rectangle().fill(UITheme.border).frame(height: 1)

            if needsHealthBanner {
                conditionalHealthBanner
                Rectangle().fill(UITheme.border).frame(height: 1)
            }

            HStack(spacing: 0) {
                CreatorMicroBoardView(
                    store: store,
                    lighting: store.fourAppLighting,
                    agentKind: agentKind,
                    actionForSlot: actionForSlot,
                    labelForSlot: labelForSlot,
                    titleForSlot: titleForSlot
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .background(UITheme.pageBackground)

                Rectangle().fill(UITheme.border).frame(width: 1)

                CreatorMicroInlineInspectorView(
                    store: store,
                    agentKind: agentKind,
                    actionForSlot: actionForSlot,
                    labelForSlot: labelForSlot,
                    titleForSlot: titleForSlot,
                    onOpenFullActionLibrary: onOpenFullActionLibrary,
                    onOpenMacroLibrary: onOpenMacroLibrary
                )
                .frame(width: 380)
                .frame(maxHeight: .infinity)
                .background(UITheme.surface)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var stageHeaderBar: some View {
        HStack(alignment: .center, spacing: 14) {
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 10) {
                    Text(profileDisplayName)
                        .font(UITheme.Typography.title)
                        .foregroundStyle(UITheme.textPrimary)
                    if store.hostActiveProfileId == store.profile?.id.text {
                        Text("当前生效")
                            .font(UITheme.Typography.captionMedium)
                            .foregroundStyle(UITheme.accent)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 2)
                            .background(UITheme.accentSoft, in: Capsule())
                    } else if DesktopAgent.named(store.profile?.name ?? "") != nil {
                        Button("启用此方案") {
                            Task { await store.setHostActiveProfile(store.profile?.id.text) }
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.small)
                        .disabled(store.busy)
                    }
                }
                Text(agentKind.bridgeDescription)
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(UITheme.textSecondary)
            }

            Spacer()

            if let profile = store.profile, profile.layers.count > 1 {
                Picker("图层", selection: Binding(get: { store.layerIndex }, set: { store.selectLayer($0) })) {
                    ForEach(profile.layers.indices, id: \.self) { i in
                        Text(profile.layers[i].name).tag(i)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(maxWidth: 220)
            }

            if let agent = DesktopAgent.named(store.profile?.name ?? ""), !agent.isGeneralFallback {
                Button("打开 \(DesktopAgent.displayName(agent.name))") {
                    Task { await store.openDesktopAgent(agent) }
                }
                .controlSize(.small)
                .disabled(!agent.installed)
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .background(UITheme.surface)
    }

    private var conditionalHealthBanner: some View {
        HStack(spacing: 12) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(UITheme.caution)

            if store.connectedID == nil {
                Text("键盘未连接")
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.caution)
                Spacer()
                Button("连接键盘") {
                    Task { await store.autoSetupHostExecutionIfNeeded() }
                }
                .controlSize(.small)
                .disabled(store.busy)
            } else if !store.accessibilityTrusted {
                Text("需要开启辅助功能权限才能向目标应用发送按键与语音")
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.caution)
                Spacer()
                HStack(spacing: 8) {
                    Button("开启权限…") { store.requestAccessibilityAuthorization() }
                        .controlSize(.small)
                    Button("重置权限登记…") { store.resetAndRequestAccessibilityAuthorization() }
                        .controlSize(.small)
                }
            } else if !store.hostExecutionEnabled {
                Text("按键控制已暂停")
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.caution)
                Spacer()
                Button("恢复控制") {
                    Task { await store.setHostExecutionEnabled(true) }
                }
                .controlSize(.small)
                .disabled(store.busy)
            } else if !store.nativeOwnership.allowsExecution {
                Text(store.nativeOwnership.summary.replacingOccurrences(of: " · ", with: "，"))
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.caution)
                Spacer()
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 10)
        .background(UITheme.cautionSoft)
    }
}

struct CreatorMicroBoardView: View {
    @ObservedObject var store: Store
    @ObservedObject var lighting: FourAppLightingController
    @ObservedObject var prefs = KeycapVisualPreferences.shared
    let agentKind: AgentProfileKind
    let actionForSlot: (Int) -> CatalogAction?
    let labelForSlot: (Int) -> String
    let titleForSlot: (Int) -> String
    @State private var showingLightingPopover = false
    @AppStorage("mi_ag_dual_scroll_v2_applied") private var dualScrollPresetApplied = false

    private let cellSize: CGFloat = 68
    private let gridSpacing: CGFloat = 8

    private var profileId: String {
        store.profile?.id.text ?? "default"
    }

    var body: some View {
        VStack(spacing: 16) {
            Spacer(minLength: 8)

            physicalBoardChassis

            compactBoardToolbar

            Spacer(minLength: 8)
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task(id: store.profile?.id.text) {
            guard agentKind == .antigravity,
                  !dualScrollPresetApplied,
                  !store.isProtectedLayerSelected,
                  store.canEditDraft,
                  ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] == nil,
                  !(store.actionExecutor is RecordingHostActionExecutor) else {
                return
            }
            dualScrollPresetApplied = true
            await applyDualScrollAndProjectNavigationPreset()
        }
    }

    private func applyDualScrollAndProjectNavigationPreset() async {
        let savedSlot = store.slot
        let bundle = "com.google.antigravity"
        for (slotIdx, id, label, cmd) in [
            (13, "ag-sidebar-scroll-up", "在项目中向上滚动", "sidebar-scroll-up"),
            (14, "ag-sidebar-scroll-down", "在项目中向下滚动", "sidebar-scroll-down"),
            (15, "desktop-project", "选项目 / 新建项目", "project"),
            (16, "desktop-scroll-up", "向上滚动对话", "scroll-up"),
            (17, "desktop-prev-chat", "上一会话", "prev-conversation"),
            (18, "desktop-scroll-down", "向下滚动对话", "scroll-down"),
            (19, "desktop-next-chat", "下一会话", "next-conversation")
        ] {
            store.slot = slotIdx
            _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                id: id, name: label, detail: bundle, adapter: "antigravity", command: cmd
            ))
        }
        store.slot = savedSlot
    }

    private func applyWorkingAndCompletedKnobPreset() async {
        let savedSlot = store.slot
        let bundle = "com.google.antigravity"
        for (slotIdx, id, label, cmd) in [
            (13, "ag-cycle-attention-prev", "上一个 Working 或蓝点会话", "cycle-attention-prev"),
            (14, "ag-cycle-attention-next", "下一个 Working 或蓝点会话", "cycle-attention-next"),
            (15, "cockpit-attention", "四应用全局待办跳转", "four-app-attention"),
            (16, "desktop-scroll-up", "向上滚动对话", "scroll-up"),
            (17, "ag-prev-project", "切换到上一项目", "prev-project"),
            (18, "desktop-scroll-down", "向下滚动对话", "scroll-down"),
            (19, "ag-next-project", "切换到下一项目", "next-project")
        ] {
            store.slot = slotIdx
            let adapter = cmd == "four-app-attention" ? "four-app" : "antigravity"
            _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                id: id, name: label, detail: bundle, adapter: adapter, command: cmd
            ))
        }
        store.slot = savedSlot
    }

    private var compactBoardToolbar: some View {
        VStack(spacing: 8) {
            HStack(spacing: 16) {
                Toggle("显示键帽文字", isOn: $prefs.showKeycapSublabels)
                    .toggleStyle(.checkbox)
                    .font(UITheme.Typography.caption)
                    .fixedSize()

                Toggle("点击发光键直接跳转会话", isOn: $prefs.singleTapFocusWindow)
                    .toggleStyle(.checkbox)
                    .font(UITheme.Typography.caption)
                    .fixedSize()
            }

            if agentKind.supportsLiveSessionSlots {
                let glowingSlots = lighting.glowingAttentionSlots(in: store)
                let activeWorkingSlots = lighting.workingSlots(in: store)
                if !glowingSlots.isEmpty || !activeWorkingSlots.isEmpty {
                    HStack(spacing: 8) {
                        ForEach(glowingSlots.prefix(2)) { gSlot in
                            let isAwaiting = gSlot.status != "completed"
                            let badgeColor = isAwaiting ? UITheme.caution : Color(red: 0.08, green: 0.78, blue: 0.42)
                            let shortName = gSlot.title.isEmpty ? "会话 \(gSlot.slotIndex + 1)" : String(gSlot.title.prefix(12))
                            Button {
                                store.slot = gSlot.slotIndex
                                Task { await store.jumpToFourAppSlot(gSlot.slotIndex) }
                            } label: {
                                HStack(spacing: 5) {
                                    Circle()
                                        .fill(badgeColor)
                                        .frame(width: 7, height: 7)
                                    Text("键 \(gSlot.slotIndex + 1)「\(shortName)」\(isAwaiting ? "待回应" : "🔵 已完事")，点击跳转")
                                        .font(UITheme.Typography.captionMedium)
                                        .foregroundStyle(UITheme.textPrimary)
                                        .lineLimit(1)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(badgeColor.opacity(0.14), in: Capsule())
                                .overlay(Capsule().strokeBorder(badgeColor.opacity(0.65), lineWidth: 1))
                            }
                            .buttonStyle(.plain)
                        }

                        ForEach(activeWorkingSlots.prefix(2)) { wSlot in
                            let shortName = wSlot.title.isEmpty ? "会话 \(wSlot.slotIndex + 1)" : String(wSlot.title.prefix(12))
                            Button {
                                store.slot = wSlot.slotIndex
                                Task { await store.jumpToFourAppSlot(wSlot.slotIndex) }
                            } label: {
                                HStack(spacing: 5) {
                                    Text("::")
                                        .font(.system(size: 10, weight: .bold, design: .monospaced))
                                        .foregroundStyle(UITheme.accent)
                                    Text("键 \(wSlot.slotIndex + 1)「\(shortName)」Working 中，点击观察")
                                        .font(UITheme.Typography.captionMedium)
                                        .foregroundStyle(UITheme.textPrimary)
                                        .lineLimit(1)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(UITheme.accent.opacity(0.14), in: Capsule())
                                .overlay(Capsule().strokeBorder(UITheme.accent.opacity(0.65), lineWidth: 1))
                            }
                            .buttonStyle(.plain)
                        }

                        if !glowingSlots.isEmpty {
                            Button("已读熄灯") {
                                Task { await lighting.acknowledgeAllCompleted(in: store) }
                            }
                            .font(UITheme.Typography.caption)
                            .controlSize(.mini)
                        }
                    }
                }

                Button {
                    showingLightingPopover.toggle()
                } label: {
                    HStack(spacing: 6) {
                        Circle()
                            .fill(boardPhaseHighlightColor)
                            .frame(width: 7, height: 7)
                        Text(boardLightingStatusLabel)
                            .font(UITheme.Typography.caption)
                            .foregroundStyle(UITheme.textPrimary)
                            .fixedSize()
                        Text("调整灯光")
                            .font(UITheme.Typography.captionMedium)
                            .foregroundStyle(UITheme.accent)
                            .fixedSize()
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(UITheme.subtleSurface, in: Capsule())
                    .overlay(Capsule().strokeBorder(UITheme.border, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .popover(isPresented: $showingLightingPopover, arrowEdge: .bottom) {
                    SimpleLightingControlCard(store: store, lighting: store.fourAppLighting)
                        .padding(16)
                        .frame(width: 340)
                        .background(UITheme.surface)
                }
            }

            if !store.isProtectedLayerSelected {
                HStack(spacing: 10) {
                    if agentKind.supportsLiveSessionSlots {
                        Button("上方 6 键设为会话键") {
                            Task {
                                for s in 0..<6 {
                                    prefs.clearRoleOverride(for: profileId, slot: s)
                                    store.slot = s
                                    _ = await store.saveAction(CodexKeycapCatalog.makeSlotOpenAction(slotIndex: s))
                                }
                            }
                        }
                        .font(UITheme.Typography.captionMedium)
                        .controlSize(.small)
                        .fixedSize()
                        .disabled(store.busy || !store.canEditDraft)
                    }

                    if agentKind == .antigravity {
                        Button("左旋钮切 Working/蓝点 + 右摇杆滚对话") {
                            Task { await applyWorkingAndCompletedKnobPreset() }
                        }
                        .font(UITheme.Typography.captionMedium)
                        .controlSize(.small)
                        .fixedSize()
                        .disabled(store.busy || !store.canEditDraft)

                        Button("左旋钮滚项目 + 右摇杆滚对话") {
                            Task { await applyDualScrollAndProjectNavigationPreset() }
                        }
                        .font(UITheme.Typography.captionMedium)
                        .controlSize(.small)
                        .fixedSize()
                        .disabled(store.busy || !store.canEditDraft)
                    }

                    Button("恢复默认布局") {
                        prefs.resetProfile(profileId)
                        Task { await store.installCodingAgentPresets() }
                    }
                    .font(UITheme.Typography.captionMedium)
                    .controlSize(.small)
                    .fixedSize()
                    .disabled(store.busy || !store.canEditDraft || store.editingDevice)
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .background(UITheme.surface, in: RoundedRectangle(cornerRadius: UITheme.Radius.panelOuter))
        .overlay(
            RoundedRectangle(cornerRadius: UITheme.Radius.panelOuter)
                .strokeBorder(UITheme.border, lineWidth: 1)
        )
    }

    private var boardPhaseHighlightColor: Color {
        if !store.fourAppLighting.lightingEnabled {
            return UITheme.textMuted
        }
        switch store.fourAppLighting.activePhase {
        case .completed:
            return Color(red: 0.08, green: 0.78, blue: 0.42)
        case .thinking:
            return UITheme.accent
        case .awaitingInput, .error:
            return UITheme.caution
        case .backgroundActive:
            return Color(red: 0.56, green: 0.36, blue: 0.90)
        case .idle:
            return SimpleLightingControlCard.color(forHex: store.fourAppLighting.idleColorHex)
        }
    }

    private var boardLightingStatusLabel: String {
        if !store.fourAppLighting.lightingEnabled {
            return "键盘灯光已关闭"
        }
        if let manual = store.fourAppLighting.manualPreviewPhase {
            switch manual {
            case .completed:
                return "正在试灯：会话完成（翠绿呼吸发光）"
            case .thinking:
                return "正在试灯：思考中（海蓝呼吸发光）"
            case .awaitingInput:
                return "正在试灯：待确认（琥珀呼吸发光）"
            default:
                return "正在试灯：日常常亮发光"
            }
        }
        if !store.fourAppLighting.sessionLightingLinked {
            return "键盘灯光：固定常亮（\(SimpleLightingControlCard.presetName(forHex: store.fourAppLighting.idleColorHex))）"
        }
        let glowingCount = lighting.glowingAttentionSlots(in: store).count
        let workingCount = lighting.workingSlots(in: store).count
        if lighting.onlyGlowOnAttentionOrCompleted {
            if glowingCount > 0 || (lighting.glowOnWorkingSessions && workingCount > 0) {
                return "状态提醒：\(glowingCount) 个蓝点已完成 · \(workingCount) 个 Working 运行中，点击发光键或用滚轮即可跳转"
            }
            return "状态提醒：蓝点完成亮绿光 · Working 运行中亮蓝光 · 已读会话保持熄灯"
        }
        switch store.fourAppLighting.activePhase {
        case .completed:
            return "灯光联动：会话已完成（翠绿呼吸发光）"
        case .thinking:
            return "灯光联动：会话生成中（海蓝呼吸发光）"
        case .awaitingInput:
            return "灯光联动：待审批或输入（琥珀呼吸发光）"
        case .error:
            return "灯光联动：会话异常（醒目提醒）"
        case .backgroundActive:
            return "灯光联动：后台任务运行中"
        case .idle:
            return "灯光联动：已开启（完成时自动亮绿光）"
        }
    }

    private var physicalBoardChassis: some View {
        VStack(spacing: gridSpacing) {
            HStack(spacing: gridSpacing) {
                rotaryEncoderCell
                programmableKeyCell(slot: 0)
                programmableKeyCell(slot: 1)
                analogJoystickCell
            }

            HStack(spacing: gridSpacing) {
                ForEach(2..<6, id: \.self) { slot in
                    programmableKeyCell(slot: slot)
                }
            }

            HStack(spacing: gridSpacing) {
                ForEach(6..<10, id: \.self) { slot in
                    programmableKeyCell(slot: slot)
                }
            }

            HStack(spacing: gridSpacing) {
                statusIndicatorClusterCell
                if store.isBottomRowSplit {
                    programmableKeyCell(slot: 10)
                    programmableKeyCell(slot: 11)
                } else {
                    wideProgrammableKeyCell
                }
                programmableKeyCell(slot: 12)
            }
        }
        .padding(UITheme.Radius.chassisPadding)
        .background(
            UITheme.surface,
            in: RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter)
        )
        .overlay(
            RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter)
                .strokeBorder(
                    agentKind.supportsLiveSessionSlots && store.fourAppLighting.activePhase != .idle
                        ? boardPhaseHighlightColor.opacity(0.65)
                        : UITheme.strongBorder,
                    lineWidth: agentKind.supportsLiveSessionSlots && store.fourAppLighting.activePhase != .idle ? 2.0 : 1.0
                )
        )
    }

    private var rotaryEncoderCell: some View {
        let isSelected = (13..<16).contains(store.slot)
        let activeSlot = (13..<16).contains(store.slot) ? store.slot : 15
        return Button {
            store.slot = activeSlot
        } label: {
            ZStack(alignment: .top) {
                Circle()
                    .fill(
                        LinearGradient(
                            colors: [Color(red: 0.22, green: 0.23, blue: 0.26), Color(red: 0.14, green: 0.15, blue: 0.17)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )
                    .frame(width: cellSize, height: cellSize)
                    .overlay(
                        Circle()
                            .strokeBorder(isSelected ? UITheme.accent : Color.white.opacity(0.14), lineWidth: isSelected ? 2.5 : 1)
                    )

                Capsule()
                    .fill(Color.white.opacity(0.88))
                    .frame(width: 2.2, height: 12)
                    .padding(.top, 6)
            }
        }
        .buttonStyle(.plain)
        .help("左上旋钮：\(labelForSlot(13))，\(labelForSlot(14))，按下：\(labelForSlot(15))")
        .accessibilityLabel("\(titleForSlot(activeSlot))，\(labelForSlot(activeSlot))")
    }

    private var analogJoystickCell: some View {
        let isSelected = store.slot >= 16
        let targetSlot = max(16, store.slot >= 16 ? store.slot : 16)
        return Button {
            store.slot = targetSlot
        } label: {
            ZStack {
                RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                    .fill(UITheme.mutedSurface)
                    .frame(width: cellSize, height: cellSize)
                    .overlay(
                        RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                            .strokeBorder(isSelected ? UITheme.accent : UITheme.border, lineWidth: isSelected ? 2.2 : 1)
                    )

                Circle()
                    .fill(
                        LinearGradient(
                            colors: [Color(red: 0.20, green: 0.21, blue: 0.24), Color(red: 0.12, green: 0.13, blue: 0.15)],
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )
                    .frame(width: 42, height: 42)
            }
        }
        .buttonStyle(.plain)
        .help("右上四向摇杆：点击在右侧调整四个方向动作")
        .accessibilityLabel("四向摇杆")
    }

    private var statusIndicatorClusterCell: some View {
        HStack(spacing: 7) {
            VStack(spacing: 4) {
                Circle()
                    .fill(store.connectedID != nil ? UITheme.accent : UITheme.textMuted.opacity(0.45))
                    .frame(width: 4.5, height: 4.5)
                Circle()
                    .fill(store.hostExecutionEnabled ? UITheme.textPrimary.opacity(0.85) : UITheme.textMuted.opacity(0.45))
                    .frame(width: 4.5, height: 4.5)
                Circle()
                    .fill(!store.nativeOwnership.allowsExecution ? UITheme.caution : UITheme.textMuted.opacity(0.45))
                    .frame(width: 4.5, height: 4.5)
            }
            Circle()
                .fill(Color(red: 0.15, green: 0.16, blue: 0.18))
                .frame(width: 32, height: 32)
                .overlay(Circle().strokeBorder(Color.white.opacity(0.12), lineWidth: 1))
        }
        .frame(width: cellSize, height: cellSize)
        .accessibilityHidden(true)
    }

    private func programmableKeyCell(slot: Int) -> some View {
        let isSelected = store.slot == slot
        let currentAction = actionForSlot(slot)
        let rawLabel = labelForSlot(slot)
        let role = prefs.role(
            for: profileId,
            slot: slot,
            agent: agentKind,
            action: currentAction,
            rawBindingLabel: rawLabel,
            isProtectedLayer: store.isProtectedLayerSelected
        )
        let glyph = prefs.glyph(for: profileId, slot: slot, agent: agentKind, action: currentAction)
        let shortText = conciseCapCaption(slot: slot, role: role, rawLabel: rawLabel)

        return Button {
            store.slot = slot
            if role == .taskKey && (0..<6).contains(slot) {
                let entry = store.fourAppSlots.slots.indices.contains(slot) ? store.fourAppSlots.slots[slot] : nil
                if let entry, entry.place != nil,
                   (prefs.singleTapFocusWindow || lighting.isSlotAttentionNeeded(entry, in: store) || lighting.isSlotWorking(entry)) {
                    Task { await store.jumpToFourAppSlot(slot) }
                }
            }
        } label: {
            Group {
                if role == .taskKey && (0..<6).contains(slot) {
                    taskKeycapSurface(slot: slot, isSelected: isSelected, caption: shortText)
                } else {
                    actionKeycapSurface(glyph: glyph, isSelected: isSelected, caption: shortText, width: cellSize)
                }
            }
        }
        .buttonStyle(.plain)
        .help("\(titleForSlot(slot))：\(rawLabel)")
        .accessibilityLabel("\(titleForSlot(slot))，\(rawLabel)")
    }

    private var wideProgrammableKeyCell: some View {
        let isSelected = store.slot == 10 || store.slot == 11
        let preferredSlot = (actionForSlot(10) == nil && actionForSlot(11) != nil) ? 11 : 10
        let displaySlot = store.slot == 11 ? 11 : preferredSlot
        let currentAction = actionForSlot(displaySlot)
        let rawLabel = labelForSlot(displaySlot)
        let glyph = prefs.glyph(for: profileId, slot: displaySlot, agent: agentKind, action: currentAction)
        let shortText = conciseCapCaption(slot: displaySlot, role: .actionKey, rawLabel: rawLabel)
        let wideWidth = cellSize * 2 + gridSpacing

        return Button {
            store.slot = preferredSlot
        } label: {
            actionKeycapSurface(glyph: glyph, isSelected: isSelected, caption: shortText, width: wideWidth)
        }
        .buttonStyle(.plain)
        .help("\(titleForSlot(displaySlot))：\(rawLabel)")
        .accessibilityLabel("\(titleForSlot(displaySlot))，\(rawLabel)")
    }

    private func taskKeycapSurface(slot: Int, isSelected: Bool, caption: String) -> some View {
        let slotEntry = store.fourAppSlots.slots.indices.contains(slot) ? store.fourAppSlots.slots[slot] : nil
        let isAttention = slotEntry.map { lighting.isSlotAttentionNeeded($0, in: store) } ?? false
        let isWorking = slotEntry.map { lighting.isSlotWorking($0) } ?? false
        let isGlowing = isAttention || (isWorking && lighting.glowOnWorkingSessions)
        let ledColor = taskSlotLedColor(slot: slot, entry: slotEntry, isGlowing: isGlowing, isWorking: isWorking)

        return ZStack {
            RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                .fill(
                    isGlowing
                        ? ledColor.opacity(0.16)
                        : (isSelected ? UITheme.accentSoft : UITheme.subtleSurface)
                )
                .frame(width: cellSize, height: cellSize)
                .overlay(
                    RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                        .strokeBorder(
                            isGlowing
                                ? ledColor
                                : (isSelected ? UITheme.accent : UITheme.strongBorder),
                            lineWidth: (isGlowing || isSelected) ? 2.2 : 1
                        )
                )

            VStack(spacing: 2) {
                if isWorking {
                    Text("::")
                        .font(.system(size: 11, weight: .heavy, design: .monospaced))
                        .foregroundStyle(UITheme.accent)
                    Text("Working")
                        .font(.system(size: 8.5, weight: .semibold))
                        .foregroundStyle(UITheme.accent)
                } else {
                    Circle()
                        .fill(ledColor)
                        .frame(width: isGlowing ? 11 : 8, height: isGlowing ? 11 : 8)
                    if isAttention {
                        Text(slotEntry?.status == "completed" ? "● 蓝点完成" : "待回应")
                            .font(.system(size: 8.5, weight: .semibold))
                            .foregroundStyle(ledColor)
                    }
                }
            }
            .offset(y: prefs.showKeycapSublabels ? -4 : 0)

            if prefs.showKeycapSublabels {
                VStack {
                    Spacer()
                    Text(caption)
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.textPrimary)
                        .lineLimit(1)
                        .padding(.horizontal, 4)
                        .padding(.bottom, 5)
                }
                .frame(width: cellSize, height: cellSize)
            }
        }
    }

    private func actionKeycapSurface(glyph: CodexKeycapId, isSelected: Bool, caption: String, width: CGFloat) -> some View {
        ZStack {
            RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                .fill(Color(red: 0.165, green: 0.17, blue: 0.19))
                .frame(width: width, height: cellSize)
                .overlay(
                    RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner)
                        .strokeBorder(isSelected ? UITheme.accent : Color.white.opacity(0.12), lineWidth: isSelected ? 2.4 : 1)
                )

            CodexKeycapGlyphView(keycap: glyph, size: 22, color: .white)
                .offset(y: prefs.showKeycapSublabels ? -5 : 0)

            if prefs.showKeycapSublabels {
                VStack {
                    Spacer()
                    Text(caption)
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(Color.white.opacity(0.85))
                        .lineLimit(1)
                        .padding(.horizontal, 5)
                        .padding(.bottom, 4)
                }
                .frame(width: width, height: cellSize)
            }
        }
    }

    private func taskSlotLedColor(slot: Int, entry: FourAppSlot?, isGlowing: Bool, isWorking: Bool = false) -> Color {
        guard let entry, entry.place != nil else {
            return UITheme.textMuted.opacity(0.35)
        }
        if isWorking {
            return UITheme.accent
        }
        if lighting.onlyGlowOnAttentionOrCompleted && !isGlowing {
            return UITheme.textMuted.opacity(0.35)
        }
        switch entry.status {
        case "needs-interaction", "needs_interaction", "waiting", "waiting_approval":
            return UITheme.caution
        case "running", "active", "runningTool", "generating", "busy":
            return UITheme.accent
        case "completed":
            return Color(red: 0.08, green: 0.78, blue: 0.42)
        case "error", "failed":
            return UITheme.caution
        default:
            return UITheme.textMuted.opacity(0.35)
        }
    }

    private func conciseCapCaption(slot: Int, role: KeycapRole, rawLabel: String) -> String {
        let liveTitle = store.fourAppSlots.slots.indices.contains(slot) ? store.fourAppSlots.slots[slot].title : nil
        return KeycapPresentationResolver.resolveCaption(
            slot: slot,
            role: role,
            actionDisplayName: rawLabel,
            liveSessionTitle: liveTitle
        )
    }
}

struct CreatorMicroInlineInspectorView: View {
    @ObservedObject var store: Store
    @ObservedObject var prefs = KeycapVisualPreferences.shared
    let agentKind: AgentProfileKind
    let actionForSlot: (Int) -> CatalogAction?
    let labelForSlot: (Int) -> String
    let titleForSlot: (Int) -> String
    let onOpenFullActionLibrary: () -> Void
    let onOpenMacroLibrary: () -> Void

    @State private var glyphSearchQuery: String = ""

    private var profileId: String {
        store.profile?.id.text ?? "default"
    }

    private var slot: Int {
        store.slot
    }

    private var controlSlot: ControlSlot {
        ControlSlot(rawSlot: slot)
    }

    private var currentAction: CatalogAction? {
        actionForSlot(slot)
    }

    private var currentRawLabel: String {
        labelForSlot(slot)
    }

    private var resolvedRole: KeycapRole {
        prefs.role(
            for: profileId,
            slot: slot,
            agent: agentKind,
            action: currentAction,
            rawBindingLabel: currentRawLabel,
            isProtectedLayer: store.isProtectedLayerSelected
        )
    }

    private var currentGlyph: CodexKeycapId {
        prefs.glyph(for: profileId, slot: slot, agent: agentKind, action: currentAction)
    }

    private var groups: [AgentActionGroup] {
        CodexKeycapCatalog.actionGroups(for: agentKind, slot: slot)
    }

    private var filteredKeycaps: [CodexKeycapId] {
        CodexKeycapId.allCases.filter { $0.matchesSearch(glyphSearchQuery) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            inspectorHeader
            Rectangle().fill(UITheme.border).frame(height: 1)

            if store.isProtectedLayerSelected {
                protectedLayerNotice
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        slotSpecificControls

                        if resolvedRole == .actionKey {
                            customShortcutInspectorCard
                        }

                        if controlSlot.isKeycap && resolvedRole == .actionKey {
                            glyphGridSection
                        }

                        if resolvedRole == .taskKey && controlSlot.supportsTaskRole {
                            sessionSlotExplanationCard
                        } else {
                            agentActionCatalogSection
                        }
                    }
                    .padding(16)
                }
            }

            Rectangle().fill(UITheme.border).frame(height: 1)
            inspectorFooter
        }
    }

    private var inspectorHeader: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .center, spacing: 8) {
                Text(titleForSlot(slot))
                    .font(UITheme.Typography.heading)
                    .foregroundStyle(UITheme.textPrimary)

                if controlSlot.isKeycap {
                    Text(resolvedRole == .taskKey ? "会话键" : "动作键")
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.accent)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 2)
                        .background(UITheme.accentSoft, in: Capsule())
                }

                Spacer()

                if DesktopAgent.named(store.profile?.name ?? "") != nil, !store.browsingDeviceSnapshot, !store.isProtectedLayerSelected {
                    Button("测试动作") {
                        Task { await store.tryDesktopAction() }
                    }
                    .controlSize(.small)
                    .disabled(store.busy || !store.canTryDesktopAction)
                }
            }

            Text("当前绑定：\(currentRawLabel)")
                .font(UITheme.Typography.captionMedium)
                .foregroundStyle(UITheme.accent)
                .lineLimit(1)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(UITheme.subtleSurface)
    }

    private var protectedLayerNotice: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("当前图层由目标客户端直接驱动")
                .font(UITheme.Typography.heading)
                .foregroundStyle(UITheme.textPrimary)
            Text("此图层用于兼容客户端自带的硬件控制协议。如需在 Micro Input 中自由修改键帽图标与动作，请创建一个可自定义层。")
                .font(UITheme.Typography.body)
                .foregroundStyle(UITheme.textSecondary)
            Button("创建可自定义层") {
                Task {
                    await store.createLayer(name: "\(DesktopAgent.displayName(store.profile?.name ?? "常用")) 自定义层")
                }
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.small)
            .disabled(store.busy || !store.canEditDraft)
            Spacer()
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder
    private var slotSpecificControls: some View {
        switch controlSlot {
        case .topRowKey(let s):
            if agentKind.supportsLiveSessionSlots {
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        Text("按键用途")
                            .font(UITheme.Typography.bodyMedium)
                            .foregroundStyle(UITheme.textPrimary)
                        Spacer()
                        Picker("按键用途", selection: Binding(
                            get: { resolvedRole },
                            set: { newRole in
                                Task {
                                    prefs.setRole(newRole, for: profileId, slot: s)
                                    if newRole == .taskKey {
                                        _ = await store.saveAction(CodexKeycapCatalog.makeSlotOpenAction(slotIndex: s))
                                    } else {
                                        let fallback = CodexKeycapCatalog.defaultAction(for: currentGlyph, agent: agentKind, slot: s)
                                        _ = await store.saveAction(fallback)
                                    }
                                }
                            }
                        )) {
                            Text("会话键").tag(KeycapRole.taskKey)
                            Text("动作键").tag(KeycapRole.actionKey)
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                        .frame(width: 150)
                    }
                    Text("会话键用于直达第 \(s + 1) 个活跃会话并显示状态点；动作键用于执行快捷动作并自定义丝印图标。")
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }
                .padding(10)
                .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
            }

        case .bottomWideKey:
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Text("拆分为两个按键")
                        .font(UITheme.Typography.bodyMedium)
                        .foregroundStyle(UITheme.textPrimary)
                    Spacer()
                    Toggle("拆分为两个按键", isOn: Binding(
                        get: { store.isBottomRowSplit },
                        set: { split in Task { await store.setBottomRowSplit(split) } }
                    ))
                    .labelsHidden()
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .disabled(store.busy)
                }

                if store.isBottomRowSplit {
                    Picker("底行按键选择", selection: Binding(
                        get: { store.slot == 11 ? 11 : 10 },
                        set: { store.slot = $0 }
                    )) {
                        Text("左键").tag(10)
                        Text("右键").tag(11)
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }

                if agentKind == .antigravity {
                    HStack(spacing: 8) {
                        Button("左键输入 + 右键听写") {
                            Task { await applyAntigravityBottomPreset(useLiveOnRight: false) }
                        }
                        .controlSize(.small)
                        Button("右键设为实时语音") {
                            Task { await applyAntigravityBottomPreset(useLiveOnRight: true) }
                        }
                        .controlSize(.small)
                    }
                }
            }
            .padding(10)
            .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))

        case .rotaryEncoder:
            VStack(alignment: .leading, spacing: 10) {
                Picker("旋钮方向", selection: Binding(
                    get: { (13..<16).contains(store.slot) ? store.slot : 15 },
                    set: { store.slot = $0 }
                )) {
                    Text("左旋").tag(13)
                    Text("右旋").tag(14)
                    Text("按下").tag(15)
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                Text("一键设左旋钮用途")
                    .font(UITheme.Typography.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)

                VStack(alignment: .leading, spacing: 6) {
                    if agentKind == .antigravity {
                        HStack(spacing: 6) {
                            Button("滚轮切 Working + 蓝点") {
                                Task { await applyEncoderPreset(mode: "ag-attention-cycle") }
                            }
                            .controlSize(.small)
                            Button("滚轮切 Working 会话") {
                                Task { await applyEncoderPreset(mode: "ag-working-cycle") }
                            }
                            .controlSize(.small)
                            Button("滚轮切蓝点已完成") {
                                Task { await applyEncoderPreset(mode: "ag-completed-cycle") }
                            }
                            .controlSize(.small)
                        }
                        HStack(spacing: 6) {
                            Button("在项目中滚动") {
                                Task { await applyEncoderPreset(mode: "ag-project-scroll") }
                            }
                            .controlSize(.small)
                            Button("在打开的会话中滚动") {
                                Task { await applyEncoderPreset(mode: "scroll") }
                            }
                            .controlSize(.small)
                            Button("切换上下会话") {
                                Task { await applyEncoderPreset(mode: "ag-sidebar") }
                            }
                            .controlSize(.small)
                        }
                    } else if agentKind != .herdr {
                        Button("在打开的会话中滚动") {
                            Task { await applyEncoderPreset(mode: "scroll") }
                        }
                        .controlSize(.small)
                    } else {
                        Button("切换标签") {
                            Task { await applyEncoderPreset(mode: "herdr-sidebar") }
                        }
                        .controlSize(.small)
                    }
                }
            }
            .padding(10)
            .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))

        case .joystickSector:
            VStack(alignment: .leading, spacing: 10) {
                Picker("摇杆方向", selection: Binding(
                    get: { (16..<20).contains(store.slot) ? store.slot : 16 },
                    set: { store.slot = $0 }
                )) {
                    Text("上").tag(16)
                    Text("左").tag(17)
                    Text("下").tag(18)
                    Text("右").tag(19)
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                if agentKind == .antigravity {
                    Text("一键设右摇杆用途")
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.textSecondary)

                    HStack(spacing: 6) {
                        Button("上下滚项目 + 左右切会话") {
                            Task { await applyJoystickPreset(mode: "ag-project-scroll") }
                        }
                        .controlSize(.small)

                        Button("上下切项目 + 左右切会话") {
                            Task { await applyJoystickPreset(mode: "ag-project-step") }
                        }
                        .controlSize(.small)

                        Button("上下滚对话") {
                            Task { await applyJoystickPreset(mode: "ag-chat-scroll") }
                        }
                        .controlSize(.small)
                    }
                }
            }
            .padding(10)
            .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))

        case .actionRowKey:
            EmptyView()
        }
    }

    private var sessionSlotExplanationCard: some View {
        let entry = store.fourAppSlots.slots.indices.contains(slot) ? store.fourAppSlots.slots[slot] : nil
        let isGlowing = entry.map { store.fourAppLighting.isSlotAttentionNeeded($0, in: store) } ?? false
        return VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("会话 \(slot + 1)")
                    .font(UITheme.Typography.bodySemibold)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                if isGlowing {
                    Text(entry?.status == "completed" ? "已完成，待你回应" : "等待确认")
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(Color(red: 0.08, green: 0.78, blue: 0.42))
                }
            }

            if let entry, entry.place != nil {
                Text("当前关联：\(entry.title.isEmpty ? "未命名会话" : entry.title)")
                    .font(UITheme.Typography.bodyMedium)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(2)

                HStack(spacing: 8) {
                    Button("立即跳转到此会话") {
                        Task { await store.jumpToFourAppSlot(slot) }
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)

                    if isGlowing, let place = entry.place {
                        Button("标为已读熄灯") {
                            Task {
                                store.fourAppLighting.acknowledgeSlot(place: place, in: store)
                                await store.fourAppLighting.forceSyncNow(in: store)
                            }
                        }
                        .controlSize(.small)
                    }
                }
            }

            Text("点按此键直接切换到第 \(slot + 1) 个活跃会话，当该会话完成或需要确认时此键会亮起呼吸灯。如需绑定其他功能，请将上方「按键用途」切换为「动作键」。")
                .font(UITheme.Typography.caption)
                .foregroundStyle(UITheme.textSecondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
    }

    private var inlineShortcutBinding: Binding<CatalogAction> {
        Binding(
            get: {
                if let currentAction, currentAction.payload["type"]?.text == "shortcut" {
                    return currentAction
                }
                return CodexKeycapCatalog.makeShortcutAction(
                    id: "custom-shortcut-slot-\(slot)",
                    name: "快捷键",
                    detail: "向当前前台软件发送自定义快捷键",
                    key: "",
                    modifiers: [],
                    target: "frontmost-editor"
                )
            },
            set: { updated in
                let rawKey = (updated.payload["key"]?.text ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
                if rawKey.isEmpty {
                    Task { await store.clearSlot() }
                } else {
                    Task { _ = await store.saveAction(updated) }
                }
            }
        )
    }

    private var customShortcutInspectorCard: some View {
        let isAppleFn = currentAction?.id == "apple-fn-key"
            || (currentAction?.payload["type"]?.text == "shortcut"
                && ["fn", "globe", "apple_fn", "fn_globe"].contains((currentAction?.payload["key"]?.text ?? "").lowercased()))
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("自定义快捷键映射")
                    .font(UITheme.Typography.bodySemibold)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button(isAppleFn ? "已设为 🌐 苹果 Fn" : "设为 🌐 苹果 Fn") {
                    prefs.setRole(.actionKey, for: profileId, slot: slot)
                    prefs.setGlyph(.mic, for: profileId, slot: slot)
                    Task { _ = await store.saveAction(CodexKeycapCatalog.makeAppleFnAction()) }
                }
                .controlSize(.small)
                .disabled(store.busy || !store.canEditDraft)

                if currentAction != nil {
                    Button("设为空键") {
                        prefs.setGlyph(.blank, for: profileId, slot: slot)
                        Task { await store.clearSlot() }
                    }
                    .controlSize(.small)
                    .disabled(store.busy || !store.canEditDraft)
                }
            }

            ShortcutRecorder(
                action: inlineShortcutBinding,
                isDisabled: store.busy || !store.canEditDraft,
                onCommit: { recorded in
                    let keyLower = (recorded.payload["key"]?.text ?? "").lowercased()
                    if ["fn", "globe", "apple_fn", "fn_globe"].contains(keyLower) {
                        prefs.setGlyph(.mic, for: profileId, slot: slot)
                        Task { _ = await store.saveAction(CodexKeycapCatalog.makeAppleFnAction()) }
                    } else {
                        Task { _ = await store.saveAction(recorded) }
                    }
                }
            )

            Text("点击上方「录制快捷键」后直接按键盘组合键即可绑定到当前键位；或点击「设为 🌐 苹果 Fn」触发系统/语音输入法专属 Fn（支持按住说话与单按切换）。")
                .font(UITheme.Typography.caption)
                .foregroundStyle(UITheme.textSecondary)
        }
        .padding(10)
        .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
    }

    private var glyphGridSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("键帽丝印图标")
                    .font(UITheme.Typography.bodySemibold)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Text(currentGlyph.chineseHint)
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(UITheme.textSecondary)
            }

            TextField("搜索图标（如：批准、听写、终端、分栏）", text: $glyphSearchQuery)
                .textFieldStyle(.roundedBorder)
                .font(UITheme.Typography.caption)

            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 6), count: 6), spacing: 6) {
                ForEach(filteredKeycaps) { cap in
                    let isChosen = currentGlyph == cap
                    Button {
                        prefs.setRole(.actionKey, for: profileId, slot: slot)
                        prefs.setGlyph(cap, for: profileId, slot: slot)
                        if cap == .blank && agentKind == .custom {
                            Task { await store.clearSlot() }
                        } else if currentAction == nil || agentKind != .custom {
                            let defaultAct = CodexKeycapCatalog.defaultAction(for: cap, agent: agentKind, slot: slot)
                            Task { _ = await store.saveAction(defaultAct) }
                        }
                    } label: {
                        VStack(spacing: 4) {
                            CodexKeycapGlyphView(
                                keycap: cap,
                                size: 18,
                                color: UITheme.textPrimary
                            )
                            Text(cap.shortLabel)
                                .font(UITheme.Typography.captionMedium)
                                .foregroundStyle(UITheme.textPrimary)
                                .lineLimit(1)
                        }
                        .frame(maxWidth: .infinity)
                        .frame(height: 52)
                        .background(
                            isChosen ? UITheme.accentSoft : UITheme.subtleSurface,
                            in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                                .strokeBorder(isChosen ? UITheme.accent : UITheme.border, lineWidth: isChosen ? 1.8 : 1)
                        )
                    }
                    .buttonStyle(.plain)
                    .help(cap.chineseHint)
                    .disabled(store.busy || !store.canEditDraft)
                }
            }
        }
    }

    private var agentActionCatalogSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("\(DesktopAgent.displayName(store.profile?.name ?? "")) 常用动作")
                .font(UITheme.Typography.bodySemibold)
                .foregroundStyle(UITheme.textPrimary)

            ForEach(groups) { group in
                VStack(alignment: .leading, spacing: 6) {
                    Text(group.title)
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.textSecondary)

                    ForEach(group.items) { item in
                        let isAssigned = currentAction?.name == item.action.name && currentAction?.id == item.action.id
                        Button {
                            prefs.setRole(.actionKey, for: profileId, slot: slot)
                            prefs.setGlyph(item.keycap, for: profileId, slot: slot)
                            Task { _ = await store.saveAction(item.action) }
                        } label: {
                            HStack(spacing: 10) {
                                CodexKeycapGlyphView(
                                    keycap: item.keycap,
                                    size: 18,
                                    color: isAssigned ? UITheme.accent : UITheme.textPrimary
                                )
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(item.title)
                                        .font(UITheme.Typography.bodyMedium)
                                        .foregroundStyle(UITheme.textPrimary)
                                        .lineLimit(1)
                                    Text(item.subtitle)
                                        .font(UITheme.Typography.caption)
                                        .foregroundStyle(UITheme.textSecondary)
                                        .lineLimit(1)
                                }
                                Spacer(minLength: 4)
                                if isAssigned {
                                    Text("已启用")
                                        .font(UITheme.Typography.captionMedium)
                                        .foregroundStyle(UITheme.accent)
                                        .padding(.horizontal, 6)
                                        .padding(.vertical, 2)
                                        .background(UITheme.accentSoft, in: RoundedRectangle(cornerRadius: 4))
                                }
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(
                                isAssigned ? UITheme.accentSoft : UITheme.subtleSurface,
                                in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: UITheme.Radius.itemInner)
                                    .strokeBorder(isAssigned ? UITheme.accent : UITheme.border, lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)
                        .disabled(store.busy || !store.canEditDraft)
                    }
                }
            }
        }
    }

    private var inspectorFooter: some View {
        HStack(spacing: 10) {
            Button("更多动作…") {
                onOpenFullActionLibrary()
            }
            .controlSize(.small)
            .disabled(store.busy || !store.canEditDraft || store.isProtectedLayerSelected)

            Button("绑定宏…") {
                onOpenMacroLibrary()
            }
            .controlSize(.small)
            .disabled(store.busy || !store.canEditDraft || store.isProtectedLayerSelected || store.editingDevice)

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(UITheme.subtleSurface)
    }

    private func applyEncoderPreset(mode: String) async {
        let adapter = agentKind.adapterId
        let bundle = agentKind.bundleId
        switch mode {
        case "scroll":
            for (slotIdx, id, label, cmd) in [
                (13, "desktop-scroll-up", "向上滚动对话", "scroll-up"),
                (14, "desktop-scroll-down", "向下滚动对话", "scroll-down"),
                (15, "desktop-scroll-bottom", "跳到底部并聚焦输入", "scroll-to-bottom")
            ] {
                store.slot = slotIdx
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: adapter, command: cmd
                ))
            }
        case "ag-sidebar":
            for (slotIdx, id, label, cmd) in [
                (13, "desktop-prev-chat", "上一会话", "prev-conversation"),
                (14, "desktop-next-chat", "下一会话", "next-conversation"),
                (15, "desktop-search", "搜索会话", "search")
            ] {
                store.slot = slotIdx
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: "antigravity", command: cmd
                ))
            }
        case "ag-project-scroll":
            for (slotIdx, id, label, cmd) in [
                (13, "ag-sidebar-scroll-up", "在项目中向上滚动", "sidebar-scroll-up"),
                (14, "ag-sidebar-scroll-down", "在项目中向下滚动", "sidebar-scroll-down"),
                (15, "desktop-project", "选项目 / 新建项目", "project")
            ] {
                store.slot = slotIdx
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: "antigravity", command: cmd
                ))
            }
        case "ag-working-cycle":
            for (slotIdx, id, label, cmd) in [
                (13, "ag-cycle-working-prev", "上一个 Working 会话", "cycle-working-prev"),
                (14, "ag-cycle-working-next", "下一个 Working 会话", "cycle-working-next"),
                (15, "cockpit-new-or-stop", "新建或停止会话", "new-or-stop")
            ] {
                store.slot = slotIdx
                let targetAdapter = cmd == "new-or-stop" ? "host" : "antigravity"
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: targetAdapter, command: cmd
                ))
            }
        case "ag-completed-cycle":
            for (slotIdx, id, label, cmd) in [
                (13, "ag-cycle-completed-prev", "上一个蓝点已完成会话", "cycle-completed-prev"),
                (14, "ag-cycle-completed-next", "下一个蓝点已完成会话", "cycle-completed-next"),
                (15, "cockpit-attention", "四应用全局待办跳转", "four-app-attention")
            ] {
                store.slot = slotIdx
                let targetAdapter = cmd == "four-app-attention" ? "four-app" : "antigravity"
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: targetAdapter, command: cmd
                ))
            }
        case "ag-attention-cycle":
            for (slotIdx, id, label, cmd) in [
                (13, "ag-cycle-attention-prev", "上一个 Working 或蓝点会话", "cycle-attention-prev"),
                (14, "ag-cycle-attention-next", "下一个 Working 或蓝点会话", "cycle-attention-next"),
                (15, "cockpit-attention", "四应用全局待办跳转", "four-app-attention")
            ] {
                store.slot = slotIdx
                let targetAdapter = cmd == "four-app-attention" ? "four-app" : "antigravity"
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: targetAdapter, command: cmd
                ))
            }
        case "herdr-sidebar":
            store.slot = 15
            _ = await store.saveAction(CodexKeycapCatalog.makeShortcutAction(
                id: "herdr-desktop", name: "显示或隐藏侧栏", detail: "dev.herdr.app", key: "b", modifiers: ["command"], target: "dev.herdr.app"
            ))
        default:
            break
        }
    }

    private func applyJoystickPreset(mode: String) async {
        let savedSlot = store.slot
        let bundle = agentKind.bundleId
        switch mode {
        case "ag-project-scroll":
            for (slotIdx, id, label, cmd) in [
                (16, "ag-sidebar-scroll-up", "在项目中向上滚动", "sidebar-scroll-up"),
                (17, "desktop-prev-chat", "上一会话", "prev-conversation"),
                (18, "ag-sidebar-scroll-down", "在项目中向下滚动", "sidebar-scroll-down"),
                (19, "desktop-next-chat", "下一会话", "next-conversation")
            ] {
                store.slot = slotIdx
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: "antigravity", command: cmd
                ))
            }
        case "ag-project-step":
            for (slotIdx, id, label, cmd) in [
                (16, "ag-prev-project", "切换到上一项目", "prev-project"),
                (17, "desktop-prev-chat", "上一会话", "prev-conversation"),
                (18, "ag-next-project", "切换到下一项目", "next-project"),
                (19, "desktop-next-chat", "下一会话", "next-conversation")
            ] {
                store.slot = slotIdx
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: "antigravity", command: cmd
                ))
            }
        case "ag-chat-scroll":
            for (slotIdx, id, label, cmd) in [
                (16, "desktop-scroll-up", "向上滚动对话", "scroll-up"),
                (17, "desktop-prev-chat", "上一会话", "prev-conversation"),
                (18, "desktop-scroll-down", "向下滚动对话", "scroll-down"),
                (19, "desktop-next-chat", "下一会话", "next-conversation")
            ] {
                store.slot = slotIdx
                _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                    id: id, name: label, detail: bundle, adapter: "antigravity", command: cmd
                ))
            }
        default:
            break
        }
        store.slot = savedSlot
    }

    private func applyAntigravityBottomPreset(useLiveOnRight: Bool) async {
        if !store.isBottomRowSplit {
            await store.setBottomRowSplit(true)
        }
        store.slot = 10
        _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
            id: "desktop-input", name: "聚焦当前输入框", detail: "com.google.antigravity", adapter: "antigravity", command: "focus-input"
        ))
        prefs.setGlyph(.codex, for: profileId, slot: 10)

        store.slot = 11
        if useLiveOnRight {
            _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                id: "desktop-live-voice", name: "Gemini Live 实时语音", detail: "启动或结束 Antigravity Live 实时语音对话", adapter: "antigravity", command: "live-voice"
            ))
            prefs.setGlyph(.party, for: profileId, slot: 11)
        } else {
            _ = await store.saveAction(CodexKeycapCatalog.makeAgentAction(
                id: "desktop-dictation", name: "按住说话，松开停止", detail: "按住启动听写，松开自动停止", adapter: "antigravity", command: "push-to-talk"
            ))
            prefs.setGlyph(.mic, for: profileId, slot: 11)
        }
    }
}

struct SimpleLightingControlCard: View {
    @ObservedObject var store: Store
    @ObservedObject var lighting: FourAppLightingController
    @ObservedObject var prefs = KeycapVisualPreferences.shared
    var showAdvancedButton: Bool = false
    var onOpenAdvancedLighting: (() -> Void)? = nil

    struct ColorPreset: Identifiable {
        let id: String
        let name: String
        let hex: String
        let color: Color
    }

    static let presets: [ColorPreset] = [
        ColorPreset(id: "warm-white", name: "暖白", hex: "#FFFFFF", color: Color(red: 0.96, green: 0.94, blue: 0.89)),
        ColorPreset(id: "emerald", name: "翠绿", hex: "#00FF66", color: Color(red: 0.08, green: 0.78, blue: 0.42)),
        ColorPreset(id: "ocean", name: "海蓝", hex: "#0088FF", color: UITheme.accent),
        ColorPreset(id: "cyan", name: "青碧", hex: "#00CCAA", color: Color(red: 0.06, green: 0.72, blue: 0.65)),
        ColorPreset(id: "amber", name: "琥珀", hex: "#FFAA00", color: UITheme.caution)
    ]

    static func color(forHex hex: String) -> Color {
        presets.first(where: { $0.hex.caseInsensitiveCompare(hex) == .orderedSame })?.color
            ?? Color(red: 0.08, green: 0.78, blue: 0.42)
    }

    static func presetName(forHex hex: String) -> String {
        presets.first(where: { $0.hex.caseInsensitiveCompare(hex) == .orderedSame })?.name ?? "暖白"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("键盘灯光")
                    .font(UITheme.Typography.bodySemibold)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Toggle("开启灯光", isOn: Binding(
                    get: { lighting.lightingEnabled },
                    set: { val in Task { await lighting.setLightingEnabled(val, in: store) } }
                ))
                .labelsHidden()
                .toggleStyle(.switch)
                .controlSize(.small)
            }

            Toggle("随会话状态自动变色（完成亮绿光，生成亮蓝光）", isOn: Binding(
                get: { lighting.sessionLightingLinked },
                set: { val in Task { await lighting.setSessionLightingLinked(val, in: store) } }
            ))
            .toggleStyle(.checkbox)
            .font(UITheme.Typography.caption)
            .disabled(!lighting.lightingEnabled)

            Toggle("仅点亮已完成或待回应的会话键（平时熄灯不打扰）", isOn: Binding(
                get: { lighting.onlyGlowOnAttentionOrCompleted },
                set: { val in Task { await lighting.setOnlyGlowOnAttentionOrCompleted(val, in: store) } }
            ))
            .toggleStyle(.checkbox)
            .font(UITheme.Typography.caption)
            .disabled(!lighting.lightingEnabled || !lighting.sessionLightingLinked)

            Toggle("同时点亮 Working 运行中的会话（亮蓝光 ::）", isOn: Binding(
                get: { lighting.glowOnWorkingSessions },
                set: { val in Task { await lighting.setGlowOnWorkingSessions(val, in: store) } }
            ))
            .toggleStyle(.checkbox)
            .font(UITheme.Typography.caption)
            .disabled(!lighting.lightingEnabled || !lighting.sessionLightingLinked)

            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("一键试灯（立即点亮键盘）")
                        .font(UITheme.Typography.captionMedium)
                        .foregroundStyle(UITheme.textSecondary)
                    Spacer()
                    if lighting.manualPreviewPhase != nil {
                        Button("恢复自动联动") {
                            Task { await lighting.triggerManualPreview(nil, in: store) }
                        }
                        .font(UITheme.Typography.captionMedium)
                        .controlSize(.mini)
                    }
                }

                HStack(spacing: 6) {
                    demoButton(title: "完成绿光", phase: .completed, dotColor: Color(red: 0.08, green: 0.78, blue: 0.42))
                    demoButton(title: "思考蓝光", phase: .thinking, dotColor: UITheme.accent)
                    demoButton(title: "待确认黄光", phase: .awaitingInput, dotColor: UITheme.caution)
                    demoButton(title: "平时常亮", phase: .idle, dotColor: Self.color(forHex: lighting.idleColorHex))
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                Text("平时常亮颜色")
                    .font(UITheme.Typography.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)

                HStack(spacing: 8) {
                    ForEach(Self.presets) { preset in
                        let selected = lighting.idleColorHex.caseInsensitiveCompare(preset.hex) == .orderedSame
                        Button {
                            Task {
                                await lighting.setIdleColor(preset.hex, in: store)
                                if lighting.manualPreviewPhase != nil {
                                    await lighting.triggerManualPreview(.idle, in: store)
                                }
                            }
                        } label: {
                            HStack(spacing: 5) {
                                Circle()
                                    .fill(preset.color)
                                    .frame(width: 10, height: 10)
                                    .overlay(Circle().strokeBorder(Color.black.opacity(0.15), lineWidth: 0.5))
                                Text(preset.name)
                                    .font(UITheme.Typography.caption)
                                    .foregroundStyle(UITheme.textPrimary)
                            }
                            .padding(.horizontal, 8)
                            .padding(.vertical, 5)
                            .background(
                                selected ? UITheme.accentSoft : UITheme.subtleSurface,
                                in: RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                                    .strokeBorder(selected ? UITheme.accent : UITheme.border, lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            HStack {
                Text("灯光亮度")
                    .font(UITheme.Typography.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Spacer()
                Picker("灯光亮度", selection: Binding(
                    get: {
                        if lighting.brightnessScale < 0.5 { return 0.35 }
                        if lighting.brightnessScale < 0.8 { return 0.65 }
                        return 0.95
                    },
                    set: { val in Task { await lighting.setBrightnessScale(val, in: store) } }
                )) {
                    Text("柔和").tag(0.35)
                    Text("标准").tag(0.65)
                    Text("明亮").tag(0.95)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 180)
            }

            HStack {
                Text("自动熄灯")
                    .font(UITheme.Typography.captionMedium)
                    .foregroundStyle(UITheme.textSecondary)
                Spacer()
                Picker("自动熄灯", selection: $prefs.autoDimMinutes) {
                    Text("1 分钟").tag(1)
                    Text("3 分钟").tag(3)
                    Text("5 分钟").tag(5)
                    Text("常亮不熄").tag(0)
                }
                .labelsHidden()
                .frame(width: 120)
            }

            if showAdvancedButton, let onOpenAdvancedLighting {
                HStack {
                    Spacer()
                    Button("高级图层灯效编辑器…") {
                        onOpenAdvancedLighting()
                    }
                    .font(UITheme.Typography.caption)
                    .controlSize(.small)
                }
            }
        }
    }

    private func demoButton(title: String, phase: FourAppAgentPhase, dotColor: Color) -> some View {
        let isSelected = lighting.manualPreviewPhase == phase
        return Button {
            Task {
                if isSelected {
                    await lighting.triggerManualPreview(nil, in: store)
                } else {
                    await lighting.triggerManualPreview(phase, in: store)
                }
            }
        } label: {
            HStack(spacing: 4) {
                Circle()
                    .fill(dotColor)
                    .frame(width: 7, height: 7)
                Text(title)
                    .font(UITheme.Typography.caption)
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(1)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(
                isSelected ? UITheme.accentSoft : UITheme.subtleSurface,
                in: RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
            )
            .overlay(
                RoundedRectangle(cornerRadius: UITheme.Radius.segmentedInner)
                    .strokeBorder(isSelected ? UITheme.accent : UITheme.border, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}
