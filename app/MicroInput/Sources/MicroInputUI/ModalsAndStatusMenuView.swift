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
        let locale = store.locale

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
                    Text(sm.hostExecutionEnabled ? sm.routingModeSummary : store.tr(sm.isProfileLocked ? "已锁定 · 已暂停软件按键映射" : "已暂停软件按键映射"))
                        .font(.system(size: 12.5, weight: .regular))
                        .foregroundStyle(UITheme.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 8)
                Toggle(
                    store.tr("按键映射"),
                    isOn: Binding(
                        get: { sm.hostExecutionEnabled },
                        set: { store.dispatch("status_menu", ["SetHostExecutionEnabled": $0]) }
                    )
                )
                .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
                .fixedSize()
            }

            StatusMenuHardwareStrip(
                powerStatus: sm.powerStatus,
                candidates: sm.candidateDevices,
                locale: locale
            ) {
                store.selectStatusMenuCandidateDevice($0)
            }

            if !sm.accessibilityTrusted {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(UITheme.textSecondary)
                    Text(store.tr("缺少 macOS 辅助功能权限，按键动作已被系统拦截"))
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    Button(store.tr("打开辅助功能设置")) { store.dispatch("status_menu", "OpenAccessibilitySettings") }
                        .surfaceButton(.primary)
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(UITheme.subtleSurface))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
            }

            if let notice = sm.hostCautionNotice {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.circle")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(UITheme.textSecondary)
                    Text(notice)
                        .font(UITheme.micro)
                        .foregroundStyle(UITheme.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 4)
                    if sm.canRetryHostCleanup {
                        Button(store.tr("重试清理")) { store.dispatch("status_menu", "RetryHostCleanup") }
                            .surfaceButton(.secondary)
                    }
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(UITheme.subtleSurface))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
            }

            Rectangle().fill(UITheme.rowDivider).frame(height: 1)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 8) {
                    Text(store.tr("锁定当前方案"))
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
                    Text(store.tr("固定响应当前方案，屏蔽前台切换与层漂移")).font(UITheme.micro).foregroundStyle(UITheme.textSecondary)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                Text(store.tr("常用软件"))
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.textSecondary)

                LazyVGrid(columns: [GridItem(.flexible(), spacing: 6), GridItem(.flexible(), spacing: 6)], spacing: 6) {
                    ForEach(sm.favorites2x2) { fav in
                        Button {
                            store.dispatch("status_menu", ["ActivateFavoriteSlot": fav.slotIndex])
                        } label: {
                            HStack(spacing: 8) {
                                AgentBadge(name: fav.title, size: 18)
                                Text(DesktopAgent.displayName(fav.title, locale: locale))
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
                PopoverMiniCheatSheetView(sheet: sheet, locale: locale)
            } else {
                PopoverCheatSheetEmptyCard(locale: locale)
            }

            Rectangle().fill(UITheme.rowDivider).frame(height: 1)

            VStack(spacing: 6) {
                HStack {
                    Text(L10n.destinationTitle(WorkbenchDestination.radial.rawValue, locale))
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
                    Text(store.tr("状态灯效"))
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
                    title: store.tr("打开主配置台")
                ) {
                    store.dispatch("status_menu", "OpenMainWorkbenchWindow")
                    onOpenMainWindow()
                }

                PopoverActionRow(
                    icon: "gearshape",
                    title: store.tr("通用设置…"),
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
                    title: store.tr("检查更新…")
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
                    title: store.tr("退出 Micro Input"),
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
    var locale: ResolvedLocale = .zhHans

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(L10n.tr("按键速查", locale))
                .font(UITheme.micro)
                .foregroundStyle(UITheme.textSecondary)
            Text(L10n.tr("当前未选择可展示的应用方案", locale))
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
                Text(store.tr(model.macroId == nil ? "新建键盘宏序列" : "编辑键盘宏序列"))
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
                SettingsRow(store.tr("宏名称"), showsDivider: true) {
                    TextField(
                        store.tr("输入宏名称…"),
                        text: Binding(
                            get: { model.name },
                            set: { store.dispatch("editor", ["MacroSetName": $0]) }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 240)
                }
                SettingsRow(store.tr("添加单击步骤"), subtitle: store.tr("输入键名（如 a, return, space）与间隔毫秒数")) {
                    HStack(spacing: 8) {
                        TextField(
                            store.tr("键名"),
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

                        Button(store.tr("+ 添加")) {
                            store.dispatch("editor", [
                                "MacroAddTapKey": ["key": model.draftKeyInput, "delay_ms": model.draftDelayMs]
                            ])
                        }
                        .surfaceButton(.secondary)

                        Button(store.tr("清空")) {
                            store.dispatch("editor", "MacroClearSteps")
                        }
                        .surfaceButton(.ghost)
                    }
                }
            }

            ScrollView {
                SettingsCard {
                    if model.steps.isEmpty {
                        SettingsRow(store.tr("尚未添加步骤"), subtitle: store.tr("在上方输入键名后点击「添加」生成敲击与等待序列"))
                    } else {
                        ForEach(Array(model.steps.enumerated()), id: \.element.id) { idx, step in
                            SettingsRow(
                                step.summaryLabel,
                                subtitle: store.locale == .en ? "Step #\(step.index + 1)" : "步骤 #\(step.index + 1)",
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
                                    Button(store.tr("删除")) {
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
                Toggle(store.tr("保存后立即绑定到当前选中按键"), isOn: $bindOnSave)
                    .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
                Spacer()
                Button(store.tr("取消")) {
                    store.dispatch("editor", "CloseModal")
                }
                .surfaceButton(.ghost)
                Button(store.tr("保存宏序列")) {
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
            .init(value: "__none__", title: store.tr("不设置"), subtitle: store.tr("清空此触发分支动作"))
        ]
        for option in model.availableActions.prefix(24) {
            opts.append(.init(value: "action:\(option.id)", title: option.label))
        }
        for option in model.availableMacros {
            let prefix = store.locale == .en ? "Macro: " : "宏："
            opts.append(.init(value: "macro:\(option.id)", title: "\(prefix)\(option.label)"))
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
                Text(store.tr(model.multiId == nil ? "新建四分支复用动作" : "编辑四分支复用动作"))
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
                SettingsRow(store.tr("动作名称"), showsDivider: true) {
                    TextField(
                        store.tr("四分支动作名称…"),
                        text: Binding(
                            get: { model.name },
                            set: { store.dispatch("editor", ["MultiSetName": $0]) }
                        )
                    )
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 220)
                }
                SettingsRow(store.tr("双击判定窗口 (ms)"), subtitle: store.tr("区分单击与连续双击的时间窗口")) {
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
                Toggle(store.tr("保存后立即绑定到当前按键"), isOn: $bindOnSave)
                    .toggleStyle(NeutralSwitchToggleStyle(showsLabel: true))
                Spacer()
                Button(store.tr("取消")) {
                    store.dispatch("editor", "CloseModal")
                }
                .surfaceButton(.ghost)
                Button(store.tr("保存四分支动作")) {
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

    private func effectOptions(_ locale: ResolvedLocale) -> [SelectOption<String>] {
        [
            .init(value: "off", title: L10n.tr("关闭 (Off)", locale), subtitle: L10n.tr("完全熄灭当前分区的硬件 LED", locale)),
            .init(value: "solid", title: L10n.tr("常亮 (Solid)", locale), subtitle: L10n.tr("保持均匀稳定的静态单色背光", locale)),
            .init(value: "gradient", title: L10n.tr("渐变 (Gradient)", locale), subtitle: L10n.tr("柔和过渡的环境双色渐变光效", locale)),
            .init(value: "snake", title: L10n.tr("追光 (Snake)", locale), subtitle: L10n.tr("沿键位轨迹依次流动的动态光效", locale)),
            .init(value: "rainbow", title: L10n.tr("光谱 (Rainbow)", locale), subtitle: L10n.tr("连续循环的全彩柔和色相过渡", locale)),
            .init(value: "breath", title: L10n.tr("呼吸 (Breath)", locale), subtitle: L10n.tr("随节奏明暗起伏的低干扰呼吸光", locale)),
        ]
    }

    private static let presetColors = ["#FFFFFF", "#3B82F6", "#14C76B", "#FFAA00", "#EF4444", "#A855F7"]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                Text("\(store.tr("按键层双区 RGB 灯效")) · \(model.layerName)")
                    .font(UITheme.heading)
                    .foregroundStyle(UITheme.textPrimary)
                Spacer()
                Toggle(
                    store.tr("同步应用到所有可编辑层"),
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
                        store.tr("尚未启用自定义硬件 RGB 配置"),
                        subtitle: store.tr("初始化后可独立配置当前层的键帽背光与底盘环绕光效。")
                    ) {
                        Button(store.tr("初始化默认双区灯效")) {
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
                Button(store.tr("重置当前层灯效")) {
                    store.dispatch("editor", "LightingResetLayer")
                }
                .surfaceButton(.ghost)
                Spacer()
                if model.hasLighting {
                    Button(store.tr("完成")) {
                        store.dispatch("editor", "CloseModal")
                    }
                    .surfaceButton(.primary)
                } else {
                    Button(store.tr("关闭")) {
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
                SettingsRow(store.tr("灯效模式"), subtitle: store.tr("选择此分区的发光方式"), showsDivider: true) {
                    SelectDropdown(
                        options: effectOptions(store.locale),
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

                SettingsRow(store.tr("颜色"), subtitle: store.tr("选择静态背光或动态呼吸主色"), showsDivider: true) {
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

                SettingsRow(store.tr("亮度"), subtitle: store.tr("调节此分区的整体发光亮度"), showsDivider: true) {
                    NeutralTrackSlider(value: Binding(
                        get: { Double(zone.brightnessPercent) },
                        set: { val in
                            store.dispatch("editor", ["LightingEditBrightness": ["zone": zone.zone, "brightness_percent": Int(val.rounded()), "all_layers": model.applyToAllLayers]])
                        }
                    ), in: 0...100).disabled(!zone.canEditBrightness)
                }

                SettingsRow(store.tr("速度"), subtitle: store.tr("调节呼吸或追光灯效的循环速率"), showsDivider: true) {
                    NeutralTrackSlider(value: Binding(
                        get: { Double(zone.speedPercent) },
                        set: { val in
                            store.dispatch("editor", ["LightingEditSpeed": ["zone": zone.zone, "speed_percent": Int(val.rounded()), "all_layers": model.applyToAllLayers]])
                        }
                    ), in: 0...100).disabled(!zone.canEditSpeed)
                }

                SettingsRow(store.tr("渐变跨度"), subtitle: store.tr("调节渐变跨度与动态光效细节")) {
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
                Text(store.tr("设备重连三方比较"))
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
                    SettingsRow(store.tr("检测到 USB 接口路径变化"), subtitle: store.tr("请先确认当前连接设备与原基线为同一台 Creator Micro。")) {
                        Button(store.tr("确认是同一台设备")) {
                            store.dispatch("device", "ConfirmComparisonIdentity")
                        }
                        .surfaceButton(.primary)
                    }
                }
            }

            ScrollView {
                SettingsCard {
                    if model.rows.isEmpty {
                        SettingsRow(store.tr("配置完全一致"), subtitle: store.tr("基线、本地草稿与设备端配置无差异"))
                    } else {
                        ForEach(Array(model.rows.enumerated()), id: \.element.id) { idx, row in
                            SettingsRow(
                                row.label,
                                subtitle: store.locale == .en
                                    ? "Base: \(row.baseSummary) → Local: \(row.localSummary) | Device: \(row.deviceSummary)"
                                    : "基线：\(row.baseSummary) → 本地：\(row.localSummary) ｜ 设备：\(row.deviceSummary)",
                                showsDivider: idx + 1 < model.rows.count
                            ) {
                                Text(store.tr(row.resultBadge))
                                    .font(UITheme.micro)
                                    .foregroundStyle(row.isConflict ? Color.red : UITheme.statusSuccess)
                            }
                        }
                    }
                }
            }
            .frame(height: 230)

            HStack(spacing: 8) {
                Button(store.tr("保留本地草稿")) {
                    store.dispatch("device", ["ResolveComparison": "KeepLocal"])
                }
                .surfaceButton(.secondary)
                Button(store.tr("采用设备配置")) {
                    store.dispatch("device", ["ResolveComparison": "UseDevice"])
                }
                .surfaceButton(.secondary)
                Spacer()
                Button(store.tr("自动合并")) {
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
                Text(store.tr("配置应用预检与备份"))
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

            Text(
                store.locale == .en
                    ? "Before: \(model.beforeSummary) → After: \(model.afterSummary)"
                    : "变更前：\(model.beforeSummary) → 变更后：\(model.afterSummary)"
            )
            .font(UITheme.caption)
            .foregroundStyle(UITheme.textSecondary)

            ScrollView {
                SettingsCard {
                    if model.changes.isEmpty {
                        SettingsRow(store.tr("暂无待应用变更"), subtitle: store.tr("当前本地草稿与基线一致"))
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
                Text(store.locale == .en ? "Rollback backup created: \(path)" : "已生成本地回滚备份：\(path)")
                    .font(UITheme.micro)
                    .foregroundStyle(UITheme.statusSuccess)
            }
            if let opId = model.candidateOperationId {
                Text(
                    store.locale == .en
                        ? "Candidate operation confirmed: \(opId) (\(model.unavailableReason))"
                        : "已确认写入候选记录：\(opId)（\(model.unavailableReason)）"
                )
                .font(UITheme.micro)
                .foregroundStyle(UITheme.textSecondary)
            }

            HStack(spacing: 8) {
                Button(store.tr("1. 生成本地回滚备份")) {
                    store.dispatch("device", "PrepareApplyBackup")
                }
                .surfaceButton(.secondary)
                .disabled(model.backupPrepared)

                Button(store.tr("2. 确认候选配置")) {
                    store.dispatch("device", "TakePreparedCandidate")
                }
                .surfaceButton(.primary)
                .disabled(!model.backupPrepared || model.candidateTaken)

                Spacer()

                Button(store.tr("完成")) {
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
