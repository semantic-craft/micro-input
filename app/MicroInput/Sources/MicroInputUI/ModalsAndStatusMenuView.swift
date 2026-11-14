import AppKit
import SwiftUI

public struct StatusMenuPopoverView: View {
    let store: CoreStore
    var updater: SoftwareUpdateController?
    var onOpenMainWindow: () -> Void
    var onOpenSettings: (() -> Void)?
    var onCheckForUpdates: (() -> Void)?
    var canCheckForUpdates: Bool

    @MainActor
    public init(
        store: CoreStore,
        updater: SoftwareUpdateController? = nil,
        onOpenMainWindow: @escaping () -> Void,
        onOpenSettings: (() -> Void)? = nil,
        onCheckForUpdates: (() -> Void)? = nil,
        canCheckForUpdates: Bool = false
    ) {
        self.store = store
        self.updater = updater
        self.onOpenMainWindow = onOpenMainWindow
        self.onOpenSettings = onOpenSettings
        self.onCheckForUpdates = onCheckForUpdates
        self.canCheckForUpdates = canCheckForUpdates
    }

    public var body: some View {
        let _ = ViewRenderTelemetry.record("StatusMenuPopoverView")
        let sm = store.statusMenu

        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(sm.activeProfileName)
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(UITheme.textPrimary)
                        if sm.isProfileLocked {
                            Image(systemName: "lock.fill")
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(UITheme.textPrimary)
                        }
                    }
                    Text(sm.routingModeSummary)
                        .font(.system(size: 12.5, weight: .regular))
                        .foregroundStyle(UITheme.textSecondary)
                }
                Spacer(minLength: 8)
                Toggle(
                    "",
                    isOn: Binding(
                        get: { sm.hostExecutionEnabled },
                        set: { store.dispatch("status_menu", ["SetHostExecutionEnabled": $0]) }
                    )
                )
                .labelsHidden()
                .toggleStyle(NeutralSwitchToggleStyle())
            }

            StatusMenuHardwareStrip(
                powerStatus: sm.powerStatus,
                candidates: sm.candidateDevices
            ) {
                store.selectStatusMenuCandidateDevice($0)
            }

            if !sm.accessibilityTrusted {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(UITheme.textSecondary)
                    Text("缺少 macOS 辅助功能权限，按键动作已被系统拦截")
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    Button("打开辅助功能设置") { store.dispatch("status_menu", "OpenAccessibilitySettings") }
                        .surfaceButton(.primary)
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(UITheme.subtleSurface))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
            }

            Rectangle().fill(UITheme.rowDivider).frame(height: 1)

            VStack(alignment: .leading, spacing: 6) {
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 8) {
                        Text("锁定当前方案")
                            .font(UITheme.captionMedium)
                            .foregroundStyle(UITheme.textPrimary)
                        Spacer()
                        Toggle("", isOn: Binding(
                            get: { sm.isProfileLocked },
                            set: { store.dispatch("status_menu", ["SetProfileLocked": $0]) }
                        ))
                        .labelsHidden()
                        .toggleStyle(NeutralSwitchToggleStyle())
                    }
                    if let notice = sm.competingHostNotice {
                        Text(notice).font(UITheme.micro).foregroundStyle(UITheme.textSecondary)
                    } else if sm.isProfileLocked {
                        Text("固定响应当前方案，屏蔽前台切换与层漂移").font(UITheme.micro).foregroundStyle(UITheme.textSecondary)
                    }
                }

                HStack(spacing: 8) {
                    Text("自动跟随前台软件")
                        .font(UITheme.captionMedium)
                        .foregroundStyle(UITheme.textPrimary)
                    if sm.manualOverride || sm.isProfileLocked {
                        Button("恢复自动") { store.dispatch("status_menu", "ResumeAutoFollow") }
                            .surfaceButton(.ghost)
                    }
                    Spacer()
                    Toggle("", isOn: Binding(
                        get: { sm.autoFollowEnabled },
                        set: { store.dispatch("status_menu", ["SetAutoFollowEnabled": $0]) }
                    ))
                    .labelsHidden()
                    .toggleStyle(NeutralSwitchToggleStyle())
                    .disabled(sm.isProfileLocked)
                    .opacity(sm.isProfileLocked ? 0.45 : 1.0)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                Text("常用软件")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)

                LazyVGrid(columns: [GridItem(.flexible(), spacing: 6), GridItem(.flexible(), spacing: 6)], spacing: 6) {
                    ForEach(sm.favorites2x2) { fav in
                        Button {
                            store.dispatch("status_menu", ["ActivateFavoriteSlot": fav.slotIndex])
                        } label: {
                            HStack(spacing: 8) {
                                AgentBadge(name: fav.title, size: 18)
                                Text(DesktopAgent.displayName(fav.title))
                                    .font(.system(size: 12.5, weight: fav.isActive ? .semibold : .medium))
                                    .foregroundStyle(UITheme.textPrimary)
                                    .lineLimit(1)
                                Spacer(minLength: 2)
                                if fav.isActive {
                                    Circle()
                                        .fill(UITheme.statusSuccess)
                                        .frame(width: 6, height: 6)
                                }
                            }
                            .padding(.horizontal, 10)
                            .padding(.vertical, 8)
                            .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(UITheme.surface))
                            .overlay(
                                RoundedRectangle(cornerRadius: 10, style: .continuous)
                                    .stroke(fav.isActive ? UITheme.textPrimary : UITheme.hairlineBorder, lineWidth: fav.isActive ? 1.5 : 1)
                            )
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .focusEffectDisabled()
                    }
                }
            }

            if let sheet = sm.cheatSheetPreview {
                PopoverMiniCheatSheetView(sheet: sheet)
            } else {
                PopoverCheatSheetEmptyCard()
            }

            Rectangle().fill(UITheme.rowDivider).frame(height: 1)

            VStack(spacing: 6) {
                HStack {
                    Text(WorkbenchDestination.radial.rawValue)
                        .font(UITheme.captionMedium)
                        .foregroundStyle(UITheme.textPrimary)
                    Spacer()
                    Toggle(
                        "",
                        isOn: Binding(
                            get: { sm.radialMenuEnabled },
                            set: { store.dispatch("status_menu", ["SetRadialMenuEnabled": $0]) }
                        )
                    )
                    .labelsHidden()
                    .toggleStyle(NeutralSwitchToggleStyle())
                }

                HStack {
                    Text("状态灯效")
                        .font(UITheme.captionMedium)
                        .foregroundStyle(UITheme.textPrimary)
                    Spacer()
                    Toggle(
                        "",
                        isOn: Binding(
                            get: { sm.lightingEnabled },
                            set: { store.dispatch("session_strip", ["SetLightingEnabled": $0]) }
                        )
                    )
                    .labelsHidden()
                    .toggleStyle(NeutralSwitchToggleStyle())
                }
            }

            Rectangle().fill(UITheme.rowDivider).frame(height: 1)

            VStack(spacing: 2) {
                PopoverActionRow(
                    icon: "macwindow",
                    title: "打开主配置台"
                ) {
                    store.dispatch("status_menu", "OpenMainWorkbenchWindow")
                    onOpenMainWindow()
                }

                PopoverActionRow(
                    icon: "gearshape",
                    title: "通用设置…",
                    shortcut: "⌘,"
                ) {
                    if let onOpenSettings {
                        onOpenSettings()
                    } else {
                        store.dispatch("status_menu", "OpenMainWorkbenchWindow")
                        onOpenMainWindow()
                    }
                }

                let effectiveCanCheck = updater?.canCheckForUpdates ?? canCheckForUpdates
                PopoverActionRow(
                    icon: "arrow.triangle.2.circlepath",
                    title: "检查更新…"
                ) {
                    if let onCheckForUpdates {
                        onCheckForUpdates()
                    } else {
                        updater?.checkForUpdates()
                    }
                }
                .disabled(!effectiveCanCheck)
                .opacity(effectiveCanCheck ? 1.0 : 0.45)

                PopoverActionRow(
                    icon: "power",
                    title: "退出 Micro Input",
                    shortcut: "⌘Q"
                ) {
                    NSApplication.shared.terminate(nil)
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .frame(width: 348)
        .fixedSize(horizontal: false, vertical: true)
        .background(UITheme.pageBackground)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

private struct PopoverCheatSheetEmptyCard: View {
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("按键速查")
                .font(UITheme.micro)
                .foregroundStyle(UITheme.textSecondary)
            Text("当前未选择可展示的应用方案")
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textMuted)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 12)
                .padding(.vertical, 14)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(UITheme.surface))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
        }
    }
}

public struct ActiveModalSheetView: View {
    let store: CoreStore
    let modal: ActiveModalModel

    public init(store: CoreStore, modal: ActiveModalModel) {
        self.store = store
        self.modal = modal
    }

    public var body: some View {
        switch modal {
        case .macroEditor(let mac):
            MacroEditorSheet(store: store, model: mac)
        case .multiEditor(let mul):
            MultiEditorSheet(store: store, model: mul)
        case .lightingEditor(let lit):
            LightingEditorSheet(store: store, model: lit)
        case .reconnectComparison(let cmp):
            ReconnectComparisonSheet(store: store, model: cmp)
        case .applyPreparation(let app):
            ApplyPreparationSheet(store: store, model: app)
        }
    }
}

struct MacroEditorSheet: View {
    let store: CoreStore
    let model: MacroEditorModel
    @State private var bindOnSave: Bool = true

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(model.macroId == nil ? "新建键盘宏序列" : "编辑键盘宏序列")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button {
                    store.dispatch("editor", "CloseModal")
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            SettingsCard {
                SettingsRow("宏名称", showsDivider: true) {
                    TextField(
                        "输入宏名称…",
                        text: Binding(
                            get: { model.name },
                            set: { store.dispatch("editor", ["MacroSetName": $0]) }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 240)
                }
                SettingsRow("添加单击步骤", subtitle: "输入键名（如 a, return, space）与间隔毫秒数") {
                    HStack(spacing: 8) {
                        TextField(
                            "键名",
                            text: Binding(
                                get: { model.draftKeyInput },
                                set: { store.dispatch("editor", ["MacroSetDraftKeyInput": $0]) }
                            )
                        )
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 96)

                        TextField(
                            "30",
                            value: Binding(
                                get: { model.draftDelayMs },
                                set: { store.dispatch("editor", ["MacroSetDraftDelayMs": max(1, $0)]) }
                            ),
                            format: .number
                        )
                        .textFieldStyle(.roundedBorder)
                        .frame(width: 60)

                        Button("+ 添加") {
                            store.dispatch("editor", [
                                "MacroAddTapKey": ["key": model.draftKeyInput, "delay_ms": model.draftDelayMs]
                            ])
                        }
                        .surfaceButton(.secondary)

                        Button("清空") {
                            store.dispatch("editor", "MacroClearSteps")
                        }
                        .surfaceButton(.ghost)
                    }
                }
            }

            ScrollView {
                SettingsCard {
                    if model.steps.isEmpty {
                        SettingsRow("尚未添加步骤", subtitle: "在上方输入键名后点击「添加」生成敲击与等待序列")
                    } else {
                        ForEach(Array(model.steps.enumerated()), id: \.element.id) { idx, step in
                            SettingsRow(
                                step.summaryLabel,
                                subtitle: "步骤 #\(step.index + 1)",
                                showsDivider: idx + 1 < model.steps.count
                            ) {
                                HStack(spacing: 6) {
                                    if step.index > 0 {
                                        Button {
                                            store.dispatch("editor", ["MacroMoveStep": ["from": step.index, "to": step.index - 1]])
                                        } label: {
                                            Image(systemName: "arrow.up")
                                        }
                                        .surfaceButton(.icon)
                                    }
                                    if step.index + 1 < model.steps.count {
                                        Button {
                                            store.dispatch("editor", ["MacroMoveStep": ["from": step.index, "to": step.index + 1]])
                                        } label: {
                                            Image(systemName: "arrow.down")
                                        }
                                        .surfaceButton(.icon)
                                    }
                                    Button("删除") {
                                        store.dispatch("editor", ["MacroRemoveStep": step.index])
                                    }
                                    .surfaceButton(.ghost)
                                }
                            }
                        }
                    }
                }
            }
            .frame(height: 210)

            if let err = model.validationError {
                Text(err)
                    .font(UITheme.caption)
                    .foregroundStyle(.red)
            }

            HStack {
                Toggle("保存后立即绑定到当前选中按键", isOn: $bindOnSave)
                    .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
                Spacer()
                Button("取消") {
                    store.dispatch("editor", "CloseModal")
                }
                .surfaceButton(.ghost)
                Button("保存宏序列") {
                    store.dispatch("editor", ["MacroSave": ["bind_to_selected_slot": bindOnSave]])
                }
                .surfaceButton(.primary)
            }
        }
        .padding(20)
        .frame(width: 600)
        .background(UITheme.pageBackground)
    }
}

struct MultiEditorSheet: View {
    let store: CoreStore
    let model: MultiEditorModel
    @State private var bindOnSave: Bool = true

    private var branchOptions: [SelectOption<String>] {
        var opts: [SelectOption<String>] = [
            .init(value: "__none__", title: "不设置", subtitle: "清空此触发分支动作")
        ]
        for (actId, actName) in model.availableActions.prefix(24) {
            opts.append(.init(value: "action:\(actId)", title: actName))
        }
        for (macId, macName) in model.availableMacros {
            opts.append(.init(value: "macro:\(macId)", title: "宏：\(macName)"))
        }
        return opts
    }

    private func selectedValue(for branch: MultiBranchModel) -> String {
        guard let kind = branch.targetKind, let id = branch.targetId else { return "__none__" }
        if kind == "Action" { return "action:\(id)" }
        if kind == "Macro" { return "macro:\(id)" }
        return "__none__"
    }

    var body: some View {
        let options = branchOptions
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(model.multiId == nil ? "新建四分支复用动作" : "编辑四分支复用动作")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button {
                    store.dispatch("editor", "CloseModal")
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            SettingsCard {
                SettingsRow("动作名称", showsDivider: true) {
                    TextField(
                        "四分支动作名称…",
                        text: Binding(
                            get: { model.name },
                            set: { store.dispatch("editor", ["MultiSetName": $0]) }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 220)
                }
                SettingsRow("双击判定窗口 (ms)", subtitle: "区分单击与连续双击的时间窗口") {
                    TextField(
                        "200",
                        value: Binding(
                            get: { model.termMs },
                            set: { store.dispatch("editor", ["MultiSetTermMs": max(50, $0)]) }
                        ),
                        format: .number
                    )
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 84)
                }
            }

            SettingsCard {
                ForEach(Array(model.branches.enumerated()), id: \.element.id) { idx, br in
                    SettingsRow(br.title, subtitle: br.targetDisplayName, showsDivider: idx + 1 < model.branches.count) {
                        SelectDropdown(
                            options: options,
                            selection: selectedValue(for: br),
                            overrideLabel: br.targetDisplayName,
                            minTriggerWidth: 164
                        ) { chosen in
                            if chosen == "__none__" {
                                store.dispatch("editor", [
                                    "MultiSetBranch": ["branch": br.branch, "target": NSNull()]
                                ])
                            } else if chosen.hasPrefix("action:") {
                                let actId = String(chosen.dropFirst("action:".count))
                                store.dispatch("editor", [
                                    "MultiSetBranch": ["branch": br.branch, "target": ["Action": ["action_id": actId]]]
                                ])
                            } else if chosen.hasPrefix("macro:") {
                                let macId = String(chosen.dropFirst("macro:".count))
                                store.dispatch("editor", [
                                    "MultiSetBranch": ["branch": br.branch, "target": ["Macro": ["action_id": macId]]]
                                ])
                            }
                        }
                    }
                }
            }

            if let err = model.validationError {
                Text(err)
                    .font(UITheme.caption)
                    .foregroundStyle(.red)
            }

            HStack {
                Toggle("保存后立即绑定到当前按键", isOn: $bindOnSave)
                    .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
                Spacer()
                Button("取消") {
                    store.dispatch("editor", "CloseModal")
                }
                .surfaceButton(.ghost)
                Button("保存四分支动作") {
                    store.dispatch("editor", ["MultiSave": ["bind_to_selected_slot": bindOnSave]])
                }
                .surfaceButton(.primary)
            }
        }
        .padding(20)
        .frame(width: 600)
        .background(UITheme.pageBackground)
    }
}

struct LightingEditorSheet: View {
    let store: CoreStore
    let model: LightingEditorModel

    private static let effectOptions: [SelectOption<String>] = [
        .init(value: "off", title: "关闭 (Off)", subtitle: "完全熄灭当前分区的硬件 LED"),
        .init(value: "solid", title: "常亮 (Solid)", subtitle: "保持均匀稳定的静态单色背光"),
        .init(value: "gradient", title: "渐变 (Gradient)", subtitle: "柔和过渡的环境双色渐变光效"),
        .init(value: "snake", title: "追光 (Snake)", subtitle: "沿键位轨迹依次流动的动态光效"),
        .init(value: "rainbow", title: "光谱 (Rainbow)", subtitle: "连续循环的全彩柔和色相过渡"),
        .init(value: "breath", title: "呼吸 (Breath)", subtitle: "随节奏明暗起伏的低干扰呼吸光"),
    ]

    private static let presetColors = ["#FFFFFF", "#3B82F6", "#14C76B", "#FFAA00", "#EF4444", "#A855F7"]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                Text("按键层双区 RGB 灯效 · \(model.layerName)")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Toggle(
                    "同步应用到所有可编辑层",
                    isOn: Binding(
                        get: { model.applyToAllLayers },
                        set: { store.dispatch("editor", ["LightingSetApplyToAllLayers": $0]) }
                    )
                )
                .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
                Button {
                    store.dispatch("editor", "CloseModal")
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            if !model.hasLighting {
                SettingsCard {
                    SettingsRow(
                        "尚未启用自定义硬件 RGB 配置",
                        subtitle: "初始化后可独立配置当前层的键帽背光与底盘环绕光效。"
                    ) {
                        Button("初始化默认双区灯效") {
                            store.dispatch("editor", "LightingAddDefault")
                        }
                        .surfaceButton(.primary)
                    }
                }
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 20) {
                        if let bl = model.backlight {
                            zoneCard(zone: bl)
                        }
                        if let ug = model.underglow {
                            zoneCard(zone: ug)
                        }
                    }
                    .padding(.vertical, 2)
                }
                .frame(maxHeight: 480)
            }

            if let err = model.validationError {
                Text(err)
                    .font(UITheme.caption)
                    .foregroundStyle(.red)
            }

            HStack(spacing: 8) {
                Button("重置当前层灯效") {
                    store.dispatch("editor", "LightingResetLayer")
                }
                .surfaceButton(.ghost)

                if model.previewActive {
                    Button("结束 30s 硬件预览 (\(model.previewRemainingSecs)s)") {
                        store.dispatch("editor", "LightingEndPreview")
                    }
                    .surfaceButton(.secondary)
                } else {
                    Button("开始 30s 硬件预览") {
                        store.dispatch("editor", "LightingBeginPreview")
                    }
                    .surfaceButton(.secondary)
                }
                Spacer()
                if model.hasLighting {
                    Button("完成") {
                        store.dispatch("editor", "CloseModal")
                    }
                    .surfaceButton(.primary)
                } else {
                    Button("关闭") {
                        store.dispatch("editor", "CloseModal")
                    }
                    .surfaceButton(.secondary)
                }
            }
        }
        .padding(20)
        .frame(width: 640)
        .background(UITheme.pageBackground)
    }

    @ViewBuilder
    private func zoneCard(zone: LightingZoneModel) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(zone.zoneTitle)
            SettingsCard {
                SettingsRow("灯效模式", subtitle: "选择此分区的发光方式", showsDivider: true) {
                    SelectDropdown(
                        options: Self.effectOptions,
                        selection: Binding(
                            get: { zone.effect },
                            set: { newEff in
                                store.dispatch("editor", [
                                    "LightingEditEffect": [
                                        "zone": zone.zone,
                                        "effect": newEff,
                                        "all_layers": model.applyToAllLayers
                                    ]
                                ])
                            }
                        ),
                        minTriggerWidth: 148
                    )
                }

                SettingsRow("颜色", subtitle: "选择静态背光或动态呼吸主色", showsDivider: true) {
                    HStack(spacing: 8) {
                        ForEach(Self.presetColors, id: \.self) { hex in
                            let isSelected = zone.colorHex.uppercased() == hex
                            Button {
                                store.dispatch("editor", [
                                    "LightingEditColorHex": [
                                        "zone": zone.zone,
                                        "color_hex": hex,
                                        "all_layers": model.applyToAllLayers
                                    ]
                                ])
                            } label: {
                                Circle()
                                    .fill(Color(hex: hex))
                                    .frame(width: 20, height: 20)
                                    .overlay(
                                        Circle()
                                            .strokeBorder(
                                                isSelected ? UITheme.textPrimary : UITheme.hairlineBorder,
                                                lineWidth: isSelected ? 2 : 1
                                            )
                                    )
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .focusEffectDisabled()
                            .disabled(!zone.canEditColor)
                            .opacity(zone.canEditColor ? 1.0 : 0.38)
                        }
                    }
                }

                SettingsRow("亮度", subtitle: "调节此分区的整体发光亮度", showsDivider: true) {
                    NeutralTrackSlider(value: Binding(
                        get: { Double(zone.brightnessPercent) },
                        set: { val in
                            store.dispatch("editor", ["LightingEditBrightness": ["zone": zone.zone, "brightness_percent": Int(val.rounded()), "all_layers": model.applyToAllLayers]])
                        }
                    ), in: 0...100).disabled(!zone.canEditBrightness)
                }

                SettingsRow("速度", subtitle: "调节呼吸或追光灯效的循环速率", showsDivider: true) {
                    NeutralTrackSlider(value: Binding(
                        get: { Double(zone.speedPercent) },
                        set: { val in
                            store.dispatch("editor", ["LightingEditSpeed": ["zone": zone.zone, "speed_percent": Int(val.rounded()), "all_layers": model.applyToAllLayers]])
                        }
                    ), in: 0...100).disabled(!zone.canEditSpeed)
                }

                SettingsRow("动态参数", subtitle: "调节渐变跨度与动态光效细节") {
                    NeutralTrackSlider(value: Binding(
                        get: { Double(zone.magicPercent) },
                        set: { val in
                            store.dispatch("editor", ["LightingEditMagic": ["zone": zone.zone, "magic_percent": Int(val.rounded()), "all_layers": model.applyToAllLayers]])
                        }
                    ), in: 0...100).disabled(!zone.canEditMagic)
                }
            }
        }
    }
}

struct ReconnectComparisonSheet: View {
    let store: CoreStore
    let model: ReconnectComparisonModel

    var body: some View {
        let needsIdentityConfirm = model.identityConfirmationRequired && !model.identityUserConfirmed
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("设备重连三方比较")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button {
                    store.dispatch("editor", "CloseModal")
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            if needsIdentityConfirm {
                SettingsCard {
                    SettingsRow("检测到 USB 接口路径变化", subtitle: "请先确认当前连接设备与原基线为同一台 Creator Micro。") {
                        Button("确认是同一台设备") {
                            store.dispatch("device", "ConfirmComparisonIdentity")
                        }
                        .surfaceButton(.primary)
                    }
                }
            }

            ScrollView {
                SettingsCard {
                    if model.rows.isEmpty {
                        SettingsRow("配置完全一致", subtitle: "基线、本地草稿与设备端配置无差异")
                    } else {
                        ForEach(Array(model.rows.enumerated()), id: \.element.id) { idx, row in
                            SettingsRow(
                                row.label,
                                subtitle: "基线：\(row.baseSummary) → 本地：\(row.localSummary) ｜ 设备：\(row.deviceSummary)",
                                showsDivider: idx + 1 < model.rows.count
                            ) {
                                Text(row.resultBadge)
                                    .font(UITheme.micro)
                                    .foregroundStyle(row.isConflict ? Color.red : UITheme.statusSuccess)
                            }
                        }
                    }
                }
            }
            .frame(height: 230)

            HStack(spacing: 8) {
                Button("保留本地草稿") {
                    store.dispatch("device", ["ResolveComparison": "KeepLocal"])
                }
                .surfaceButton(.secondary)
                Button("采用设备配置") {
                    store.dispatch("device", ["ResolveComparison": "UseDevice"])
                }
                .surfaceButton(.secondary)
                Spacer()
                Button("自动合并") {
                    store.dispatch("device", ["ResolveComparison": "Merge"])
                }
                .surfaceButton(needsIdentityConfirm ? .secondary : .primary)
                .disabled(!model.canMerge)
            }
        }
        .padding(20)
        .frame(width: 620)
        .background(UITheme.pageBackground)
    }
}

struct ApplyPreparationSheet: View {
    let store: CoreStore
    let model: ApplyPreparationModel

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("配置应用预检与备份")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Button {
                    store.dispatch("device", "CancelApplyPreview")
                } label: {
                    Image(systemName: "xmark")
                }
                .surfaceButton(.icon)
            }

            Text("变更前：\(model.beforeSummary) → 变更后：\(model.afterSummary)")
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)

            ScrollView {
                SettingsCard {
                    if model.changes.isEmpty {
                        SettingsRow("暂无待应用变更", subtitle: "当前本地草稿与基线一致")
                    } else {
                        ForEach(Array(model.changes.enumerated()), id: \.element.id) { idx, ch in
                            SettingsRow(ch.locationLabel, showsDivider: idx + 1 < model.changes.count) {
                                Text("\(ch.before) → \(ch.after)")
                                    .font(UITheme.monoBadge)
                                    .foregroundStyle(UITheme.textSecondary)
                            }
                        }
                    }
                }
            }
            .frame(height: 200)

            if let path = model.backupPath {
                Text("已生成本地回滚备份：\(path)")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.statusSuccess)
            }
            if let opId = model.candidateOperationId {
                Text("已确认写入候选记录：\(opId)（\(model.unavailableReason)）")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)
            }

            HStack(spacing: 8) {
                Button("1. 生成本地回滚备份") {
                    store.dispatch("device", "PrepareApplyBackup")
                }
                .surfaceButton(.secondary)
                .disabled(model.backupPrepared)

                Button("2. 确认候选配置") {
                    store.dispatch("device", "TakePreparedCandidate")
                }
                .surfaceButton(.primary)
                .disabled(!model.backupPrepared || model.candidateTaken)

                Spacer()

                Button("完成") {
                    store.dispatch("device", "CancelApplyPreview")
                }
                .surfaceButton(.ghost)
            }
        }
        .padding(20)
        .frame(width: 620)
        .background(UITheme.pageBackground)
    }
}
