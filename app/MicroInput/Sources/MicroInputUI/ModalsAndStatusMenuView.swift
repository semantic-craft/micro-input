import AppKit
import SwiftUI

public struct StatusMenuPopoverView: View {
    let store: CoreStore
    var onOpenMainWindow: () -> Void

    public init(store: CoreStore, onOpenMainWindow: @escaping () -> Void) {
        self.store = store
        self.onOpenMainWindow = onOpenMainWindow
    }

    public var body: some View {
        let _ = ViewRenderTelemetry.record("StatusMenuPopoverView")
        let sm = store.statusMenu
        let ss = store.sessionStrip
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(sm.activeProfileName)
                        .font(.headline)
                    Text(sm.routingModeSummary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Toggle(
                    "键盘控制",
                    isOn: Binding(
                        get: { sm.hostExecutionEnabled },
                        set: { store.dispatch("status_menu", ["SetHostExecutionEnabled": $0]) }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
            }

            Divider()

            HStack {
                Toggle(
                    "自动跟随前台软件",
                    isOn: Binding(
                        get: { sm.autoFollowEnabled },
                        set: { store.dispatch("status_menu", ["SetAutoFollowEnabled": $0]) }
                    )
                )
                .font(.caption)
                Spacer()
                if sm.manualOverride {
                    Button("恢复自动") {
                        store.dispatch("status_menu", "ResumeAutoFollow")
                    }
                    .controlSize(.mini)
                }
            }

            Text("常用软件磁贴 (2×2)")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

            LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 6) {
                ForEach(sm.favorites2x2) { fav in
                    Button {
                        store.dispatch("status_menu", ["ActivateFavoriteSlot": fav.slotIndex])
                    } label: {
                        HStack(spacing: 8) {
                            AgentBadge(name: fav.title, size: 20)
                            Text(DesktopAgent.displayName(fav.title))
                                .font(.caption.weight(.semibold))
                                .lineLimit(1)
                            Spacer(minLength: 2)
                            if fav.isActive {
                                Circle().fill(UITheme.accent).frame(width: 6, height: 6)
                            }
                        }
                        .padding(8)
                        .background(
                            RoundedRectangle(cornerRadius: 8)
                                .fill(fav.isActive ? UITheme.accentSoft : UITheme.subtleSurface)
                        )
                        .overlay(
                            RoundedRectangle(cornerRadius: 8)
                                .stroke(fav.isActive ? UITheme.accent : UITheme.border, lineWidth: 1)
                        )
                    }
                    .buttonStyle(.plain)
                }
            }

            Text("活跃会话槽位 (AG00–AG05)")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

            VStack(spacing: 4) {
                ForEach(ss.sessionSlots.prefix(6)) { slot in
                    Button {
                        store.dispatch("session_strip", ["ActivateSlot": slot.slotIndex])
                    } label: {
                        HStack(spacing: 6) {
                            Circle()
                                .fill(Color(hex: slot.ledColorHex))
                                .frame(width: 7, height: 7)
                            Text(slot.keyBindingId)
                                .font(.system(size: 10, weight: .semibold, design: .monospaced))
                                .foregroundStyle(.secondary)
                            Text(slot.title.isEmpty ? "空闲槽位" : slot.title)
                                .font(.caption)
                                .lineLimit(1)
                            Spacer()
                            if slot.isUnreadBlueDot {
                                Text("未读")
                                    .font(.caption2.weight(.bold))
                                    .foregroundStyle(.green)
                            }
                        }
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: .controlBackgroundColor)))
                    }
                    .buttonStyle(.plain)
                }
            }

            Divider()

            HStack(spacing: 10) {
                Toggle(
                    "轮盘菜单",
                    isOn: Binding(
                        get: { sm.radialMenuEnabled },
                        set: { store.dispatch("status_menu", ["SetRadialMenuEnabled": $0]) }
                    )
                )
                .font(.caption)

                Toggle(
                    "状态灯效",
                    isOn: Binding(
                        get: { ss.simpleLighting.lightingEnabled },
                        set: { store.dispatch("session_strip", ["SetLightingEnabled": $0]) }
                    )
                )
                .font(.caption)

                Spacer()

                Button(sm.cheatSheetVisible ? "隐藏速查" : "按键速查") {
                    store.dispatch("status_menu", "ToggleCheatSheet")
                }
                .controlSize(.mini)
            }

            HStack {
                Button("打开主配置台") {
                    store.dispatch("status_menu", "OpenMainWorkbenchWindow")
                    onOpenMainWindow()
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)

                Spacer()

                Button("退出") {
                    NSApplication.shared.terminate(nil)
                }
                .controlSize(.small)
            }
        }
        .padding(14)
        .frame(width: 340)
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
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(model.macroId == nil ? "新建键盘宏序列" : "编辑键盘宏序列")
                    .font(.headline)
                Spacer()
                Button("关闭") {
                    store.dispatch("editor", "CloseModal")
                }
            }

            HStack {
                Text("宏名称：")
                TextField(
                    "输入宏名称…",
                    text: Binding(
                        get: { model.name },
                        set: { store.dispatch("editor", ["MacroSetName": $0]) }
                    )
                )
                .textFieldStyle(.roundedBorder)
            }

            HStack(spacing: 8) {
                Text("步骤键名：")
                TextField(
                    "如 a, return, space",
                    text: Binding(
                        get: { model.draftKeyInput },
                        set: { store.dispatch("editor", ["MacroSetDraftKeyInput": $0]) }
                    )
                )
                .textFieldStyle(.roundedBorder)
                .frame(width: 120)

                Text("间隔 (ms)：")
                TextField(
                    "30",
                    value: Binding(
                        get: { model.draftDelayMs },
                        set: { store.dispatch("editor", ["MacroSetDraftDelayMs": max(1, $0)]) }
                    ),
                    format: .number
                )
                .textFieldStyle(.roundedBorder)
                .frame(width: 70)

                Button("+ 添加单击步骤") {
                    store.dispatch("editor", [
                        "MacroAddTapKey": [
                            "key": model.draftKeyInput,
                            "delay_ms": model.draftDelayMs
                        ]
                    ])
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)

                Button("清空全部") {
                    store.dispatch("editor", "MacroClearSteps")
                }
                .controlSize(.small)
            }

            List {
                ForEach(model.steps) { step in
                    HStack {
                        Text("#\(step.index + 1)")
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                        Text(step.summaryLabel)
                            .font(.subheadline)
                        Spacer()
                        if step.index > 0 {
                            Button("↑") {
                                store.dispatch("editor", ["MacroMoveStep": ["from": step.index, "to": step.index - 1]])
                            }
                            .controlSize(.mini)
                        }
                        if step.index + 1 < model.steps.count {
                            Button("↓") {
                                store.dispatch("editor", ["MacroMoveStep": ["from": step.index, "to": step.index + 1]])
                            }
                            .controlSize(.mini)
                        }
                        Button("删除", role: .destructive) {
                            store.dispatch("editor", ["MacroRemoveStep": step.index])
                        }
                        .controlSize(.mini)
                    }
                }
            }
            .frame(height: 200)

            if let err = model.validationError {
                Text(err).font(.caption).foregroundStyle(.red)
            }

            HStack {
                Toggle("保存后立即绑定到当前选中按键", isOn: $bindOnSave)
                Spacer()
                Button("取消") {
                    store.dispatch("editor", "CloseModal")
                }
                Button("保存宏序列") {
                    store.dispatch("editor", ["MacroSave": ["bind_to_selected_slot": bindOnSave]])
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(18)
        .frame(width: 560)
    }
}

struct MultiEditorSheet: View {
    let store: CoreStore
    let model: MultiEditorModel
    @State private var bindOnSave: Bool = true

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(model.multiId == nil ? "新建四分支复用动作 (Multi)" : "编辑四分支复用动作 (Multi)")
                    .font(.headline)
                Spacer()
                Button("关闭") {
                    store.dispatch("editor", "CloseModal")
                }
            }

            HStack(spacing: 12) {
                Text("名称：")
                TextField(
                    "四分支动作名称…",
                    text: Binding(
                        get: { model.name },
                        set: { store.dispatch("editor", ["MultiSetName": $0]) }
                    )
                )
                .textFieldStyle(.roundedBorder)

                Text("双击判定窗口 (ms)：")
                TextField(
                    "200",
                    value: Binding(
                        get: { model.termMs },
                        set: { store.dispatch("editor", ["MultiSetTermMs": max(50, $0)]) }
                    ),
                    format: .number
                )
                .textFieldStyle(.roundedBorder)
                .frame(width: 80)
            }

            ForEach(model.branches) { br in
                HStack {
                    Text(br.title)
                        .font(.subheadline.weight(.semibold))
                        .frame(width: 170, alignment: .leading)
                    Text(br.targetDisplayName)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Menu("选择动作…") {
                        Button("不设置（清空此分支）") {
                            store.dispatch("editor", [
                                "MultiSetBranch": [
                                    "branch": br.branch,
                                    "target": NSNull()
                                ]
                            ])
                        }
                        Divider()
                        ForEach(model.availableActions.prefix(24), id: \.0) { actId, actName in
                            Button(actName) {
                                store.dispatch("editor", [
                                    "MultiSetBranch": [
                                        "branch": br.branch,
                                        "target": ["Action": ["action_id": actId]]
                                    ]
                                ])
                            }
                        }
                        if !model.availableMacros.isEmpty {
                            Divider()
                            ForEach(model.availableMacros, id: \.0) { macId, macName in
                                Button("宏：\(macName)") {
                                    store.dispatch("editor", [
                                        "MultiSetBranch": [
                                            "branch": br.branch,
                                            "target": ["Macro": ["action_id": macId]]
                                        ]
                                    ])
                                }
                            }
                        }
                    }
                    .frame(width: 160)
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: .controlBackgroundColor)))
            }

            if let err = model.validationError {
                Text(err).font(.caption).foregroundStyle(.red)
            }

            HStack {
                Toggle("保存后立即绑定到当前按键", isOn: $bindOnSave)
                Spacer()
                Button("取消") {
                    store.dispatch("editor", "CloseModal")
                }
                Button("保存四分支动作") {
                    store.dispatch("editor", ["MultiSave": ["bind_to_selected_slot": bindOnSave]])
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(18)
        .frame(width: 580)
    }
}

struct LightingEditorSheet: View {
    let store: CoreStore
    let model: LightingEditorModel

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text("按键层双区 RGB 灯效编辑 · \(model.layerName)")
                    .font(.headline)
                Spacer()
                Toggle(
                    "同步应用到当前方案所有可编辑层",
                    isOn: Binding(
                        get: { model.applyToAllLayers },
                        set: { store.dispatch("editor", ["LightingSetApplyToAllLayers": $0]) }
                    )
                )
                .font(.caption)
                Button("关闭") {
                    store.dispatch("editor", "CloseModal")
                }
            }

            if !model.hasLighting {
                HStack {
                    Text("当前按键层尚未启用自定义硬件 RGB 配置。")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("初始化默认双区灯效") {
                        store.dispatch("editor", "LightingAddDefault")
                    }
                    .buttonStyle(.borderedProminent)
                }
                .padding(12)
                .background(RoundedRectangle(cornerRadius: 10).fill(Color(nsColor: .controlBackgroundColor)))
            } else {
                HStack(alignment: .top, spacing: 14) {
                    if let bl = model.backlight {
                        zoneCard(zone: bl)
                    }
                    if let ug = model.underglow {
                        zoneCard(zone: ug)
                    }
                }
            }

            if let err = model.validationError {
                Text(err).font(.caption).foregroundStyle(.red)
            }

            HStack {
                Button("重置当前层灯效") {
                    store.dispatch("editor", "LightingResetLayer")
                }
                if model.previewActive {
                    Button("结束 30s 硬件预览 (\(model.previewRemainingSecs)s)") {
                        store.dispatch("editor", "LightingEndPreview")
                    }
                } else {
                    Button("开始 30s 硬件预览") {
                        store.dispatch("editor", "LightingBeginPreview")
                    }
                }
                Spacer()
                Button("完成") {
                    store.dispatch("editor", "CloseModal")
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(18)
        .frame(width: 660)
    }

    @ViewBuilder
    private func zoneCard(zone: LightingZoneModel) -> some View {
        let effects: [(String, String)] = [
            ("off", "关闭 (Off)"),
            ("solid", "常亮 (Solid)"),
            ("gradient", "渐变 (Gradient)"),
            ("snake", "贪吃蛇 (Snake)"),
            ("rainbow", "彩虹 (Rainbow)"),
            ("breath", "呼吸 (Breath)"),
        ]
        let presetColors = ["#FFFFFF", "#3B82F6", "#14C76B", "#FFAA00", "#EF4444", "#A855F7"]

        GroupBox(zone.zoneTitle) {
            VStack(alignment: .leading, spacing: 10) {
                Picker(
                    "灯效模式",
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
                    )
                ) {
                    ForEach(effects, id: \.0) { code, label in
                        Text(label).tag(code)
                    }
                }

                HStack(spacing: 6) {
                    Text("颜色：").font(.caption)
                    ForEach(presetColors, id: \.self) { hex in
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
                                .frame(width: 18, height: 18)
                                .overlay(
                                    Circle().strokeBorder(zone.colorHex.uppercased() == hex ? Color.primary : Color.clear, lineWidth: 2)
                                )
                        }
                        .buttonStyle(.plain)
                        .disabled(!zone.canEditColor)
                    }
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("亮度：\(zone.brightnessPercent)%").font(.caption)
                    Slider(
                        value: Binding(
                            get: { Double(zone.brightnessPercent) },
                            set: { val in
                                store.dispatch("editor", [
                                    "LightingEditBrightness": [
                                        "zone": zone.zone,
                                        "brightness_percent": Int(val.rounded()),
                                        "all_layers": model.applyToAllLayers
                                    ]
                                ])
                            }
                        ),
                        in: 0...100
                    )
                    .disabled(!zone.canEditBrightness)
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("速度：\(zone.speedPercent)%").font(.caption)
                    Slider(
                        value: Binding(
                            get: { Double(zone.speedPercent) },
                            set: { val in
                                store.dispatch("editor", [
                                    "LightingEditSpeed": [
                                        "zone": zone.zone,
                                        "speed_percent": Int(val.rounded()),
                                        "all_layers": model.applyToAllLayers
                                    ]
                                ])
                            }
                        ),
                        in: 0...100
                    )
                    .disabled(!zone.canEditSpeed)
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("动态参数 (Magic)：\(zone.magicPercent)%").font(.caption)
                    Slider(
                        value: Binding(
                            get: { Double(zone.magicPercent) },
                            set: { val in
                                store.dispatch("editor", [
                                    "LightingEditMagic": [
                                        "zone": zone.zone,
                                        "magic_percent": Int(val.rounded()),
                                        "all_layers": model.applyToAllLayers
                                    ]
                                ])
                            }
                        ),
                        in: 0...100
                    )
                    .disabled(!zone.canEditMagic)
                }
            }
            .padding(8)
        }
    }
}

struct ReconnectComparisonSheet: View {
    let store: CoreStore
    let model: ReconnectComparisonModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("设备重连三方比较（基线 vs 本地草稿 vs 设备配置）")
                    .font(.headline)
                Spacer()
                Button("关闭") {
                    store.dispatch("editor", "CloseModal")
                }
            }

            if model.identityConfirmationRequired && !model.identityUserConfirmed {
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                    Text("检测到 USB 接口路径变化，请先确认该设备与原基线为同一台小键盘。")
                        .font(.caption)
                    Spacer()
                    Button("确认是同一台设备") {
                        store.dispatch("device", "ConfirmComparisonIdentity")
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                }
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.orange.opacity(0.14)))
            }

            List(model.rows) { row in
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(row.label).font(.subheadline.weight(.semibold))
                        Text("基线：\(row.baseSummary) → 本地：\(row.localSummary) ｜ 设备：\(row.deviceSummary)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Text(row.resultBadge)
                        .font(.caption.weight(.bold))
                        .foregroundStyle(row.isConflict ? .red : .green)
                }
            }
            .frame(height: 220)

            HStack {
                Button("保留本地草稿 (KeepLocal)") {
                    store.dispatch("device", ["ResolveComparison": "KeepLocal"])
                }
                Button("采用设备配置 (UseDevice)") {
                    store.dispatch("device", ["ResolveComparison": "UseDevice"])
                }
                Button("三方无冲突自动合并 (Merge)") {
                    store.dispatch("device", ["ResolveComparison": "Merge"])
                }
                .buttonStyle(.borderedProminent)
                .disabled(!model.canMerge)
            }
        }
        .padding(18)
        .frame(width: 620)
    }
}

struct ApplyPreparationSheet: View {
    let store: CoreStore
    let model: ApplyPreparationModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("配置应用预检与备份（只读门禁核验）")
                    .font(.headline)
                Spacer()
                Button("取消并关闭") {
                    store.dispatch("device", "CancelApplyPreview")
                }
            }

            Text("变更前：\(model.beforeSummary) → 变更后：\(model.afterSummary)")
                .font(.caption)
                .foregroundStyle(.secondary)

            List(model.changes) { ch in
                HStack {
                    Text(ch.locationLabel).font(.subheadline.weight(.semibold))
                    Spacer()
                    Text("\(ch.before) → \(ch.after)")
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                }
            }
            .frame(height: 180)

            if let path = model.backupPath {
                Text("已生成本地回滚备份：\(path)")
                    .font(.caption2)
                    .foregroundStyle(.green)
            }
            if let opId = model.candidateOperationId {
                Text("已确认写入候选记录：\(opId)（\(model.unavailableReason)）")
                    .font(.caption2)
                    .foregroundStyle(.blue)
            }

            HStack {
                Button("1. 生成本地回滚备份") {
                    store.dispatch("device", "PrepareApplyBackup")
                }
                .disabled(model.backupPrepared)

                Button("2. 确认候选配置") {
                    store.dispatch("device", "TakePreparedCandidate")
                }
                .buttonStyle(.borderedProminent)
                .disabled(!model.backupPrepared || model.candidateTaken)

                Spacer()

                Button("完成") {
                    store.dispatch("device", "CancelApplyPreview")
                }
            }
        }
        .padding(18)
        .frame(width: 620)
    }
}
