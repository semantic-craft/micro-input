import AppKit
import SwiftUI

public struct FirmwarePortModel: Equatable, Identifiable {
    public var id: String { path }
    public let path: String
    public let label: String
}

public struct FirmwareSliceModel: Equatable {
    public var channel = "stable"
    public var currentVersion: String?
    public var releaseTag: String?
    public var releaseNotes = ""
    public var versionStatus = ""
    public var prerelease = false
    public var phase = "idle"
    public var message = "检查 Creator Micro 2 官方稳定版固件"
    public var busy = false
    public var downloaded = false
    public var digestVerified = false
    public var deviceReserved = false
    public var ports: [FirmwarePortModel] = []
    public var selectedPort: String?
    public var progress: Double?
    public var backupPath: String?
    public var v070 = V070HardwareSliceModel()

    public init() {}

    public init(json: [String: Any]) {
        channel = json["channel"] as? String ?? "stable"
        currentVersion = json["current_version"] as? String
        if let release = json["release"] as? [String: Any] {
            releaseTag = release["tag"] as? String
            releaseNotes = release["notes"] as? String ?? ""
            prerelease = release["prerelease"] as? Bool ?? false
        }
        versionStatus = json["version_status"] as? String ?? ""
        phase = json["phase"] as? String ?? "idle"
        message = json["message"] as? String ?? message
        busy = json["busy"] as? Bool ?? false
        downloaded = json["downloaded"] as? Bool ?? false
        digestVerified = json["digest_verified"] as? Bool ?? false
        deviceReserved = json["device_reserved"] as? Bool ?? false
        selectedPort = json["selected_port"] as? String
        progress = json["progress"] as? Double
        backupPath = json["backup_path"] as? String
        ports = (json["ports"] as? [[String: Any]] ?? []).compactMap {
            guard let path = $0["path"] as? String else { return nil }
            let product = $0["product"] as? String ?? $0["manufacturer"] as? String ?? "USB 设备"
            return FirmwarePortModel(path: path, label: "\(product) · \(path)")
        }
        v070 = V070HardwareSliceModel(json: json["v070"] as? [String: Any] ?? [:])
    }

    public var canCheck: Bool { !busy && !deviceReserved }
    public var canDownload: Bool { canCheck && releaseTag != nil }
    public var canPrepare: Bool { !busy && !deviceReserved && downloaded && digestVerified }
    public var canInstall: Bool {
        !busy && deviceReserved && downloaded && digestVerified && selectedPort != nil
            && ports.contains { $0.path == selectedPort }
    }
    public var preventsTermination: Bool { busy && deviceReserved }
    public var currentVersionLabel: String { currentVersion ?? "未知（尚未从设备读取）" }
}

public struct FirmwareUpdateSettingsSection: View {
    let model: FirmwareSliceModel
    let onConnectKeyboard: (() -> Void)?
    let dispatch: (Any) -> Void
    @State private var confirmation: Confirmation?
    private enum Confirmation: String, Identifiable {
        case prepare
        case install

        var id: String { rawValue }

        var title: String {
            self == .prepare ? "准备更新设备固件？" : "确认安装到所选 Creator Micro 2？"
        }

        var actionTitle: String {
            self == .prepare ? "暂停控制并准备" : "确认设备并安装"
        }
    }

    public init(
        model: FirmwareSliceModel,
        onConnectKeyboard: (() -> Void)? = nil,
        dispatch: @escaping (Any) -> Void
    ) {
        self.model = model
        self.onConnectKeyboard = onConnectKeyboard
        self.dispatch = dispatch
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("设备固件")
            SettingsCard {
                SettingsRow("Creator Micro 2", subtitle: "当前固件：\(model.currentVersionLabel)", showsDivider: true) {
                    HStack(spacing: 8) {
                        if !model.deviceReserved && model.currentVersion == nil, let onConnectKeyboard {
                            Button("连接有线键盘") { onConnectKeyboard() }
                                .surfaceButton(.secondary)
                                .accessibilityIdentifier("firmware-connect")
                        }
                        Button("检查固件更新") { dispatch(["Check": ["channel": model.channel]]) }
                            .surfaceButton(.secondary)
                            .disabled(!model.canCheck)
                            .accessibilityIdentifier("firmware-check")
                    }
                }
                SettingsRow("更新通道", subtitle: model.channel == "prerelease" ? "预发布固件与现有键位和配置协议的兼容性尚未验证。" : "只检查官方稳定版。", showsDivider: true) {
                    SelectDropdown(
                        options: [SelectOption(value: "stable", title: "稳定版"), SelectOption(value: "prerelease", title: "预发布版")],
                        selection: model.channel
                    ) { dispatch(["Check": ["channel": $0]]) }
                    .disabled(!model.canCheck)
                    .accessibilityIdentifier("firmware-channel")
                }
                if !model.versionStatus.isEmpty {
                    SettingsRow("版本比较", subtitle: model.versionStatus, showsDivider: true)
                }
                if let tag = model.releaseTag {
                    SettingsRow("可下载版本：\(tag)", subtitle: model.downloaded ? (model.digestVerified ? "下载完成，官方 SHA-256 摘要核对通过。" : "官方未提供摘要；可下载查看，不能自动安装。") : "从 Work Louder 官方 GitHub 发布下载。", showsDivider: true) {
                        Button(model.downloaded ? "重新下载" : "下载并校验") { dispatch("Download") }
                            .surfaceButton(.secondary)
                            .disabled(!model.canDownload)
                            .accessibilityIdentifier("firmware-download")
                    }
                    if !model.releaseNotes.isEmpty {
                        DisclosureGroup("发行说明") {
                            Text(model.releaseNotes)
                                .font(UITheme.caption)
                                .foregroundStyle(UITheme.textSecondary)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .tint(UITheme.textPrimary)
                        .padding(20)
                        .accessibilityIdentifier("firmware-release-notes")
                    }
                }
                SettingsRow("准备设备", subtitle: model.deviceReserved ? "按键控制已暂停。选择这把键盘对应的引导模式串口。" : "会暂停按键控制并请求进入引导模式，键盘暂时不可用。", showsDivider: true) {
                    Button("准备更新") { confirmation = .prepare }
                        .surfaceButton(.secondary)
                        .disabled(!model.canPrepare)
                        .accessibilityIdentifier("firmware-prepare")
                }
                if model.deviceReserved {
                    SettingsRow("引导模式设备", subtitle: "Espressif 名称不能确认键盘型号。请确认所选串口属于这把 Creator Micro 2。", showsDivider: true) {
                        VStack(alignment: .trailing, spacing: 8) {
                            SelectDropdown(
                                options: model.ports.map { SelectOption(value: $0.path, title: $0.label) },
                                selection: model.selectedPort ?? "",
                                overrideLabel: model.selectedPort ?? "请选择设备串口"
                            ) { dispatch(["SelectPort": ["path": $0]]) }
                            .disabled(model.busy || model.ports.isEmpty)
                            .accessibilityIdentifier("firmware-port")
                            Button("查找串口") { dispatch("ScanPorts") }
                                .surfaceButton(.secondary)
                                .disabled(model.busy)
                                .accessibilityIdentifier("firmware-scan")
                        }
                    }
                    SettingsRow("安装固件", subtitle: "先保存完整 Flash 备份，再写入并校验。期间请勿断开 USB 或退出应用。", showsDivider: true) {
                        Button("安装固件…") { confirmation = .install }
                            .surfaceButton(.secondary)
                            .disabled(!model.canInstall)
                            .accessibilityIdentifier("firmware-install")
                    }
                }
                SettingsRow(model.phase == "flash_verified" ? "写入校验通过" : "更新状态", subtitle: model.message) {
                    if model.busy {
                        if let progress = model.progress {
                            ProgressView(value: progress).frame(width: 100)
                        } else {
                            ProgressView().controlSize(.small)
                        }
                    }
                }
                if model.backupPath != nil || model.deviceReserved {
                    SettingsRow("结束与备份", subtitle: "完成后需重新连接并核对运行版本与原键位；写入校验不代表运行验收。") {
                        HStack(spacing: 8) {
                            if let path = model.backupPath {
                                Button("显示备份") {
                                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                                }
                                .surfaceButton(.secondary)
                                .accessibilityIdentifier("firmware-reveal-backup")
                            }
                            if model.deviceReserved {
                                Button("结束更新") { dispatch("Finish") }
                                    .surfaceButton(.ghost)
                                    .disabled(model.busy)
                                    .accessibilityIdentifier("firmware-finish")
                            }
                        }
                    }
                }
            }
        }
        .accessibilityIdentifier("firmware-settings")
        .alert(
            confirmation?.title ?? "",
            isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }),
            presenting: confirmation
        ) { kind in
            Button("取消", role: .cancel) { confirmation = nil }
            Button(kind.actionTitle, role: kind == .install ? .destructive : nil) {
                if kind == .prepare, model.canPrepare { dispatch("PrepareDevice") }
                if kind == .install, model.canInstall { dispatch("Install") }
                confirmation = nil
            }
        } message: { kind in
            Text(kind == .prepare
                 ? "Micro Input 将暂停宿主按键控制并请求键盘进入引导模式，键盘会暂时停止工作。下载的固件不会在此步骤写入。"
                 : "请确认串口 \(model.selectedPort ?? "未选择") 属于你的 Creator Micro 2。将先保存完整 Flash 备份，再安装 \(model.releaseTag ?? "固件")。写入中断可能需要恢复，备份不代表已验证恢复成功。\(model.prerelease ? "预发布固件与当前配置协议的兼容性尚未验证。" : "更新后仍需核对运行版本和原键位。")")
        }
    }
}

private func v070Int(_ value: Any?) -> Int? {
    guard let value, !(value is Bool) else { return nil }
    return (value as? NSNumber)?.intValue
}

private func v070Double(_ value: Any?) -> Double? {
    guard let value, !(value is Bool), let d = (value as? NSNumber)?.doubleValue, d.isFinite else { return nil }
    return d
}

public struct V070MouseConfigModel: Equatable {
    public var preset = "standard"
    public var delayMs = 100, intervalMs = 20, moveDelta = 8, maxSpeed = 10, timeToMax = 30
    public var wheelDelayMs = 100, wheelIntervalMs = 80, wheelDelta = 1, wheelMaxSpeed = 8, wheelTimeToMax = 40
    public var stickSpeed = 2, stickRegulator = 20, stickScroll = 40
    public var deadzone = 0.15

    public init() {}
    public init(json: [String: Any]) {
        preset = json["preset"] as? String ?? "standard"
        delayMs = v070Int(json["delay_ms"]) ?? delayMs
        intervalMs = v070Int(json["interval_ms"]) ?? intervalMs
        moveDelta = v070Int(json["move_delta"]) ?? moveDelta
        maxSpeed = v070Int(json["max_speed"]) ?? maxSpeed
        timeToMax = v070Int(json["time_to_max"]) ?? timeToMax
        wheelDelayMs = v070Int(json["wheel_delay_ms"]) ?? wheelDelayMs
        wheelIntervalMs = v070Int(json["wheel_interval_ms"]) ?? wheelIntervalMs
        wheelDelta = v070Int(json["wheel_delta"]) ?? wheelDelta
        wheelMaxSpeed = v070Int(json["wheel_max_speed"]) ?? wheelMaxSpeed
        wheelTimeToMax = v070Int(json["wheel_time_to_max"]) ?? wheelTimeToMax
        stickSpeed = v070Int(json["stick_speed"]) ?? stickSpeed
        stickRegulator = v070Int(json["stick_regulator"]) ?? stickRegulator
        stickScroll = v070Int(json["stick_scroll"]) ?? stickScroll
        deadzone = v070Double(json["deadzone"]) ?? deadzone
    }
}

public struct V070PmConfigModel: Equatable {
    public var autoSleep = true, hostSleep = true
    public var sleepAfterMin = 15

    public init() {}
    public init(json: [String: Any]) {
        autoSleep = json["auto_sleep"] as? Bool ?? autoSleep
        hostSleep = json["host_sleep"] as? Bool ?? hostSleep
        sleepAfterMin = v070Int(json["sleep_after_min"]) ?? sleepAfterMin
    }
}

public struct V070PmHoldModel: Equatable, Identifiable {
    public var id: String { "\(owner):\(floor):\(ageMs)" }
    public var owner = "", floor = ""
    public var ageMs = 0

    public init() {}
    public init(json: [String: Any]) {
        owner = json["owner"] as? String ?? ""
        floor = json["floor"] as? String ?? ""
        ageMs = v070Int(json["age_ms"]) ?? 0
    }
}

public struct V070PmStateModel: Equatable {
    public var state = "", floor = "", baseFloor = ""
    public var linePowered = false, autoSleep = false, usbHostHold = false
    public var sleepAfterS = 0, idleS = 0, standbyS = 0, creditS = 0
    public var holds: [V070PmHoldModel] = []

    public init() {}
    public init(json: [String: Any]) {
        state = json["state"] as? String ?? ""
        floor = json["floor"] as? String ?? ""
        baseFloor = json["base_floor"] as? String ?? ""
        linePowered = json["line_powered"] as? Bool ?? false
        autoSleep = json["auto_sleep"] as? Bool ?? false
        sleepAfterS = v070Int(json["sleep_after_s"]) ?? 0
        idleS = v070Int(json["idle_s"]) ?? 0
        standbyS = v070Int(json["standby_s"]) ?? 0
        creditS = v070Int(json["credit_s"]) ?? 0
        usbHostHold = json["usb_host_hold"] as? Bool ?? false
        holds = (json["holds"] as? [[String: Any]] ?? []).map(V070PmHoldModel.init(json:))
    }
}

public struct V070DeviceNameModel: Equatable {
    public var name = "", factoryName = ""
    public var maxLength = 15

    public init() {}
    public init(json: [String: Any]) {
        name = json["name"] as? String ?? ""
        factoryName = json["factory_name"] as? String ?? ""
        maxLength = v070Int(json["max_length"]) ?? 15
    }
}

public struct V070SaRgbModel: Equatable {
    public var sa: Int?
    public var colorHex = "#FFFFFF"
    public var brightness = 1.0, speed = 0.5
    public var effect = 1

    public init() {}
    public init(json: [String: Any]) {
        sa = v070Int(json["sa"])
        colorHex = json["color_hex"] as? String ?? "#FFFFFF"
        brightness = v070Double(json["brightness"]) ?? 1.0
        speed = v070Double(json["speed"]) ?? 0.5
        effect = v070Int(json["effect"]) ?? 1
    }
}

public struct V070HardwareSliceModel: Equatable {
    public var supported = false, busy = false, synced = false
    public var activeProfile: Int?, activeLayer: Int?, batteryPercent: Int?, isCharging: Bool?
    public var hostSleepStatus = "awake"
    public var statusMessage = "需连接 0.7.0-rc.16+ 独占固件设备"
    public var lastError: String?
    public var autoLayerSyncEnabled = false
    public var sessionSaRgbSyncEnabled = false
    public var preferredDeviceSerial: String?
    public var mouse = V070MouseConfigModel()
    public var pmConfig = V070PmConfigModel()
    public var pmState = V070PmStateModel()
    public var deviceName = V070DeviceNameModel()
    public var saRgb = V070SaRgbModel()

    public init() {}
    public init(json: [String: Any]) {
        supported = json["supported"] as? Bool ?? false
        busy = json["busy"] as? Bool ?? false
        synced = json["synced"] as? Bool ?? false
        activeProfile = v070Int(json["active_profile"])
        activeLayer = v070Int(json["active_layer"])
        batteryPercent = v070Int(json["battery_percent"])
        isCharging = json["is_charging"] as? Bool
        hostSleepStatus = json["host_sleep_status"] as? String ?? hostSleepStatus
        statusMessage = json["status_message"] as? String ?? statusMessage
        lastError = json["last_error"] as? String
        autoLayerSyncEnabled = json["auto_layer_sync_enabled"] as? Bool ?? false
        sessionSaRgbSyncEnabled = json["session_sa_rgb_sync_enabled"] as? Bool ?? false
        preferredDeviceSerial = json["preferred_device_serial"] as? String
        mouse = V070MouseConfigModel(json: json["mouse"] as? [String: Any] ?? [:])
        pmConfig = V070PmConfigModel(json: json["pm_config"] as? [String: Any] ?? [:])
        pmState = V070PmStateModel(json: json["pm_state"] as? [String: Any] ?? [:])
        deviceName = V070DeviceNameModel(json: json["device_name"] as? [String: Any] ?? [:])
        saRgb = V070SaRgbModel(json: json["sa_rgb"] as? [String: Any] ?? [:])
    }
}

public enum V070HardwareCommand {
    public static func refresh() -> [String: Any] { ["V070Hardware": "Refresh"] }
    public static func setAutoLayerSync(_ enabled: Bool) -> [String: Any] { ["V070Hardware": ["SetAutoLayerSync": ["enabled": enabled]]] }
    public static func setSessionSaRgbSync(_ enabled: Bool) -> [String: Any] { ["V070Hardware": ["SetSessionSaRgbSync": ["enabled": enabled]]] }
    public static func selectLayer(_ layer: Int) -> [String: Any] { ["V070Hardware": ["SelectLayer": ["layer": layer]]] }
    public static func applyMousePreset(_ preset: String) -> [String: Any] { ["V070Hardware": ["ApplyMousePreset": ["preset": preset]]] }
    public static func renameDevice(name: String) -> [String: Any] { ["V070Hardware": ["RenameDevice": ["name": name]]] }
    public static func resetSaRgb(sa: Int) -> [String: Any] { ["V070Hardware": ["ResetSaRgb": ["sa": sa]]] }

    public static func applyMouseConfig(
        moveDelta: Int, maxSpeed: Int, wheelDelta: Int, wheelMaxSpeed: Int,
        stickSpeed: Int, stickScroll: Int, deadzone: Double
    ) -> [String: Any] {
        ["V070Hardware": ["ApplyMouseConfig": [
            "move_delta": moveDelta, "max_speed": maxSpeed, "wheel_delta": wheelDelta,
            "wheel_max_speed": wheelMaxSpeed, "stick_speed": stickSpeed, "stick_scroll": stickScroll,
            "deadzone": deadzone,
        ]]]
    }

    public static func applyPmConfig(autoSleep: Bool, hostSleep: Bool, sleepAfterMin: Int) -> [String: Any] {
        ["V070Hardware": ["ApplyPmConfig": [
            "auto_sleep": autoSleep, "host_sleep": hostSleep, "sleep_after_min": sleepAfterMin,
        ]]]
    }

    public static func applySaRgb(
        sa: Int, colorHex: String, brightness: Double, speed: Double, effect: Int
    ) -> [String: Any] {
        ["V070Hardware": ["ApplySaRgb": [
            "sa": sa, "color_hex": colorHex, "brightness": brightness, "speed": speed, "effect": effect,
        ]]]
    }
}

private struct V070MouseDraft: Equatable {
    var moveDelta: String, maxSpeed: String, wheelDelta: String, wheelMaxSpeed: String
    var stickSpeed: String, stickScroll: String
    var deadzone: Double

    init(from mouse: V070MouseConfigModel) {
        moveDelta = String(mouse.moveDelta)
        maxSpeed = String(mouse.maxSpeed)
        wheelDelta = String(mouse.wheelDelta)
        wheelMaxSpeed = String(mouse.wheelMaxSpeed)
        stickSpeed = String(mouse.stickSpeed)
        stickScroll = String(mouse.stickScroll)
        deadzone = mouse.deadzone
    }

    var validated: (Int, Int, Int, Int, Int, Int, Double)? {
        guard let md = Self.parseInt(moveDelta, in: 1...255),
              let ms = Self.parseInt(maxSpeed, in: 1...255),
              let wd = Self.parseInt(wheelDelta, in: 1...255),
              let ws = Self.parseInt(wheelMaxSpeed, in: 1...255),
              let ss = Self.parseInt(stickSpeed, in: 1...255),
              let sc = Self.parseInt(stickScroll, in: 1...2000),
              deadzone.isFinite, (0.0025...0.95).contains(deadzone)
        else { return nil }
        return (md, ms, wd, ws, ss, sc, deadzone)
    }

    var validationHint: String? {
        validated == nil ? "请确认步进/速度在 1–255、摇杆滚轮在 1–2000，死区在 0.0025–0.95" : nil
    }

    private static func parseInt(_ raw: String, in range: ClosedRange<Int>) -> Int? {
        guard let val = Int(raw.trimmingCharacters(in: .whitespacesAndNewlines)), range.contains(val) else { return nil }
        return val
    }
}

private struct V070Badge: View {
    let text: String
    var accentDot = false
    var body: some View {
        HStack(spacing: 5) {
            if accentDot { Circle().fill(UITheme.statusSuccess).frame(width: 6, height: 6) }
            Text(text).font(UITheme.monoBadge).foregroundStyle(UITheme.textSecondary).lineLimit(1)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 4)
        .background(Capsule().fill(UITheme.subtleSurface))
        .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
    }
}

public struct V070HardwareSettingsSection: View {
    let model: V070HardwareSliceModel
    let dispatch: (Any) -> Void

    @State private var deviceNameDraft: String
    @State private var mouseDraft: V070MouseDraft
    @State private var pmAutoSleepDraft: Bool
    @State private var pmHostSleepDraft: Bool
    @State private var pmSleepAfterMinDraft: Int
    @State private var saIdText: String
    @State private var saColorHexDraft: String
    @State private var saBrightnessPercentDraft: Double
    @State private var saSpeedPercentDraft: Double
    @State private var saEffectDraft: Int

    private static let mousePresets: [(id: String, title: String)] = [("slow", "慢速"), ("standard", "标准"), ("fast", "快速")]
    private static let baseSleepMinutes = [1, 5, 10, 15, 30, 60, 120, 240, 480, 1440]
    private static let saPresetColors = ["#FFFFFF", "#33AAFF", "#14C76B", "#FFB040", "#EF4444"]
    private static let saEffectOptions: [SelectOption<Int>] = [
        .init(value: 1, title: "常亮 (1)", subtitle: "稳定静态单色覆盖"),
        .init(value: 3, title: "彩虹 (3)", subtitle: "连续色相循环过渡"),
        .init(value: 4, title: "呼吸 (4)", subtitle: "标准节奏明暗起伏"),
        .init(value: 6, title: "浅呼吸 (6)", subtitle: "低幅度柔和微光呼吸"),
    ]

    public init(model: V070HardwareSliceModel, dispatch: @escaping (Any) -> Void) {
        self.model = model
        self.dispatch = dispatch
        _deviceNameDraft = State(initialValue: model.deviceName.name)
        _mouseDraft = State(initialValue: V070MouseDraft(from: model.mouse))
        _pmAutoSleepDraft = State(initialValue: model.pmConfig.autoSleep)
        _pmHostSleepDraft = State(initialValue: model.pmConfig.hostSleep)
        _pmSleepAfterMinDraft = State(initialValue: model.pmConfig.sleepAfterMin)
        _saIdText = State(initialValue: model.saRgb.sa.map(String.init) ?? "")
        _saColorHexDraft = State(initialValue: model.saRgb.colorHex)
        _saBrightnessPercentDraft = State(initialValue: Self.unitToPercent(model.saRgb.brightness))
        _saSpeedPercentDraft = State(initialValue: Self.unitToPercent(model.saRgb.speed))
        _saEffectDraft = State(initialValue: Self.validApplyEffect(model.saRgb.effect))
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            overviewSection
            if model.supported {
                layerSection
                mouseSection
                pmSection
                saRgbSection
            }
        }
        .accessibilityIdentifier("v070-hardware-settings")
        .onChange(of: model.synced) { _, synced in
            guard synced else { return }
            deviceNameDraft = model.deviceName.name
            mouseDraft = V070MouseDraft(from: model.mouse)
            pmAutoSleepDraft = model.pmConfig.autoSleep
            pmHostSleepDraft = model.pmConfig.hostSleep
            pmSleepAfterMinDraft = model.pmConfig.sleepAfterMin
        }
        .onChange(of: model.deviceName.name) { _, next in deviceNameDraft = next }
        .onChange(of: model.mouse) { _, next in mouseDraft = V070MouseDraft(from: next) }
        .onChange(of: model.pmConfig) { _, next in
            pmAutoSleepDraft = next.autoSleep
            pmHostSleepDraft = next.hostSleep
            pmSleepAfterMinDraft = next.sleepAfterMin
        }
        .onChange(of: model.saRgb) { _, next in
            if let sa = next.sa { saIdText = String(sa) }
            saColorHexDraft = next.colorHex
            saBrightnessPercentDraft = Self.unitToPercent(next.brightness)
            saSpeedPercentDraft = Self.unitToPercent(next.speed)
            saEffectDraft = Self.validApplyEffect(next.effect)
        }
    }

    private var overviewSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("0.7.0 硬件能力") {
                HStack(spacing: 8) {
                    if model.busy { ProgressView().controlSize(.small) }
                    Button("刷新状态") { dispatch(V070HardwareCommand.refresh()) }
                        .surfaceButton(.secondary)
                        .disabled(!model.supported || model.busy)
                        .accessibilityIdentifier("v070-refresh")
                }
            }
            SettingsCard {
                if !model.supported {
                    SettingsRow(unavailableTitle, subtitle: unavailableSubtitle, showsDivider: true) { V070Badge(text: hostSleepBadgeText) }
                } else {
                    SettingsRow("电量与当前层", subtitle: batterySummaryText, showsDivider: true) {
                        HStack(spacing: 6) {
                            if let pct = model.batteryPercent { V070Badge(text: "\(pct)%", accentDot: model.isCharging == true) }
                            V070Badge(text: profileLayerBadgeText)
                        }
                    }
                    SettingsRow("电源状态", subtitle: pmStateSummaryText, showsDivider: true) {
                        HStack(spacing: 6) {
                            V070Badge(text: hostSleepBadgeText)
                            V070Badge(text: model.pmState.usbHostHold ? "usb.host 保持中" : "无 usb.host 锁", accentDot: model.pmState.usbHostHold)
                        }
                    }
                    SettingsRow("设备名称", subtitle: deviceNameSubtitle, showsDivider: true) {
                        HStack(spacing: 8) {
                            TextField("设备名称", text: $deviceNameDraft)
                                .textFieldStyle(.roundedBorder).controlSize(.small).frame(width: 136).disabled(model.busy)
                                .accessibilityIdentifier("v070-device-name-field")
                            Button("应用名称") { dispatch(V070HardwareCommand.renameDevice(name: trimmedDeviceName)) }
                                .surfaceButton(.secondary).disabled(!canApplyDeviceName).accessibilityIdentifier("v070-device-rename")
                        }
                    }
                    SettingsRow("同步回执", subtitle: model.lastError ?? model.statusMessage, showsDivider: true) {
                        if let err = model.lastError, !err.isEmpty {
                            Text("异常").font(UITheme.micro).foregroundStyle(.red)
                        } else {
                            V070Badge(text: model.synced ? "已同步" : "待同步", accentDot: model.synced)
                        }
                    }
                }
                if let serial = model.preferredDeviceSerial, !serial.isEmpty {
                    SettingsRow("已记忆设备序列号", subtitle: "多台 Creator Micro 2 同时接入时将优先匹配此设备", showsDivider: true) {
                        V070Badge(text: serial)
                    }
                }
                SettingsRow(
                    "前台切层实验联动 (kb.pl.set)",
                    subtitle: "随前台编程工具自动切换板载第 1–6 层（仅写 RAM 零擦写；OAI 原生层自动跳过）",
                    showsDivider: true
                ) {
                    Toggle(
                        "",
                        isOn: Binding(
                            get: { model.autoLayerSyncEnabled },
                            set: { dispatch(V070HardwareCommand.setAutoLayerSync($0)) }
                        )
                    )
                    .labelsHidden()
                    .toggleStyle(NeutralSwitchToggleStyle())
                    .disabled(model.busy)
                    .accessibilityIdentifier("v070-auto-layer-sync")
                }
                SettingsRow(
                    "AI 会话状态自动映灯 (kb.sa.rgb)",
                    subtitle: "按会话状态自动映射自定义 SA 键灯色（≥100ms 节流；退出或休眠自动恢复板载默认，OAI 原生层跳过）",
                    showsDivider: false
                ) {
                    Toggle(
                        "",
                        isOn: Binding(
                            get: { model.sessionSaRgbSyncEnabled },
                            set: { dispatch(V070HardwareCommand.setSessionSaRgbSync($0)) }
                        )
                    )
                    .labelsHidden()
                    .toggleStyle(NeutralSwitchToggleStyle())
                    .disabled(model.busy)
                    .accessibilityIdentifier("v070-session-sa-rgb-sync")
                }
            }
        }
    }

    private var layerSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("无写 Flash 实时切层")
            SettingsCard {
                SettingsRow("当前板载层", subtitle: "第 0 层为系统基底层（不可直切）；切换第 1–6 层仅修改 RAM，不写 Flash。", showsDivider: true) {
                    V070Badge(text: profileLayerBadgeText)
                }
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 68), spacing: 8)], spacing: 8) {
                    ForEach(1...6, id: \.self) { layer in
                        Button("第 \(layer) 层") { dispatch(V070HardwareCommand.selectLayer(layer)) }
                            .surfaceButton(model.activeLayer == layer ? .primary : .secondary)
                            .disabled(model.busy)
                            .accessibilityIdentifier("v070-layer-\(layer)")
                    }
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 14)
            }
        }
    }

    private var mouseSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("鼠标速度与摇杆死区")
            SettingsCard {
                SettingsRow("速度预设", subtitle: "当前档位：\(presetDisplayName(model.mouse.preset))（立即生效）", showsDivider: true) {
                    HStack(spacing: 6) {
                        ForEach(Self.mousePresets, id: \.id) { item in
                            Button(item.title) { dispatch(V070HardwareCommand.applyMousePreset(item.id)) }
                                .surfaceButton(model.mouse.preset == item.id ? .primary : .secondary)
                                .disabled(model.busy)
                                .accessibilityIdentifier("v070-mouse-preset-\(item.id)")
                        }
                    }
                }
                SettingsRow("摇杆死区", subtitle: "防漂移盲区（UI 范围 0.02–0.45，固件允许 0.0025–0.95）", showsDivider: true) {
                    HStack(spacing: 10) {
                        NeutralTrackSlider(
                            value: Binding(get: { mouseDraft.deadzone }, set: { mouseDraft.deadzone = (($0 * 100.0).rounded() / 100.0) }),
                            in: 0.02...0.45, trackWidth: 132, showsPercentage: false
                        ).disabled(model.busy).accessibilityIdentifier("v070-deadzone-slider")
                        Text(String(format: "%.2f", mouseDraft.deadzone)).font(UITheme.monoBadge).foregroundStyle(UITheme.textSecondary).frame(width: 42, alignment: .trailing)
                    }
                }
                mouseSliderRow("指针初速", subtitle: "move_delta · 单步位移（1–255）", text: $mouseDraft.moveDelta, defaultRange: 1...255, unit: "px/tick", id: "v070-mouse-move-delta-slider")
                mouseSliderRow("指针上限", subtitle: "max_speed · 加速上限（1–255）", text: $mouseDraft.maxSpeed, defaultRange: 1...255, unit: "px/tick", id: "v070-mouse-max-speed-slider")
                mouseSliderRow("滚轮步进", subtitle: "wheel_delta · 单步滚动（1–255）", text: $mouseDraft.wheelDelta, defaultRange: 1...255, unit: "x/tick", id: "v070-mouse-wheel-delta-slider")
                mouseSliderRow("滚轮上限", subtitle: "wheel_max_speed · 加速上限（1–255）", text: $mouseDraft.wheelMaxSpeed, defaultRange: 1...255, unit: "x/tick", id: "v070-mouse-wheel-max-speed-slider")
                mouseSliderRow("摇杆指针", subtitle: "stick_speed · 硬件指针速率（1–255）", text: $mouseDraft.stickSpeed, defaultRange: 1...255, unit: "x/tick", id: "v070-mouse-stick-speed-slider")
                mouseSliderRow("摇杆滚轮", subtitle: "stick_scroll · 硬件滚轮速率（1–2000）", text: $mouseDraft.stickScroll, defaultRange: 1...2000, unit: "x/tick", id: "v070-mouse-stick-scroll-slider")
                SettingsRow("自定义参数", subtitle: mouseDraft.validationHint ?? "连同上方摇杆死区一起写入设备 NVS") {
                    Button("应用鼠标参数") {
                        if let v = mouseDraft.validated {
                            dispatch(V070HardwareCommand.applyMouseConfig(
                                moveDelta: v.0, maxSpeed: v.1, wheelDelta: v.2,
                                wheelMaxSpeed: v.3, stickSpeed: v.4, stickScroll: v.5, deadzone: v.6
                            ))
                        }
                    }
                    .surfaceButton(.secondary).disabled(model.busy || mouseDraft.validated == nil).accessibilityIdentifier("v070-mouse-apply")
                }
            }
        }
    }

    private var pmSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("休眠跟随与电源策略")
            SettingsCard {
                toggleRow("自动休眠", subtitle: "空闲达到设定时长后自动进入低功耗待机", isOn: $pmAutoSleepDraft, id: "v070-pm-auto-sleep")
                toggleRow("跟随宿主休眠", subtitle: "随 Mac USB 挂起与唤醒动态调整设备电源地板", isOn: $pmHostSleepDraft, id: "v070-pm-host-sleep")
                SettingsRow("休眠等待时长", subtitle: "有效范围 1–1440 分钟（当前选中 \(pmSleepAfterMinDraft) 分钟）", showsDivider: true) {
                    SelectDropdown(options: sleepDurationOptions, selection: $pmSleepAfterMinDraft, minTriggerWidth: 120)
                        .disabled(model.busy).accessibilityIdentifier("v070-pm-sleep-after")
                }
                SettingsRow("写入电源策略", subtitle: "保存自动休眠、宿主跟随与超时阈值至 NVS") {
                    Button("应用休眠策略") {
                        dispatch(V070HardwareCommand.applyPmConfig(autoSleep: pmAutoSleepDraft, hostSleep: pmHostSleepDraft, sleepAfterMin: pmSleepAfterMinDraft))
                    }
                    .surfaceButton(.secondary).disabled(model.busy || !(1...1440).contains(pmSleepAfterMinDraft)).accessibilityIdentifier("v070-pm-apply")
                }
            }
        }
    }

    private var saRgbSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("单键动态背光")
            SettingsCard {
                SettingsRow("Smart Action 编号", subtitle: "输入非零整数 SA ID（1–16777215）", showsDivider: true) {
                    TextField("如 7057", text: $saIdText)
                        .textFieldStyle(.roundedBorder).controlSize(.small).frame(width: 120).multilineTextAlignment(.trailing)
                        .disabled(model.busy).accessibilityIdentifier("v070-sa-id-field")
                }
                SettingsRow("动态灯效", subtitle: model.saRgb.effect == 0 ? "最近状态：已恢复默认背景灯效" : "仅发送固件支持的 1/3/4/6 动态灯效", showsDivider: true) {
                    SelectDropdown(options: Self.saEffectOptions, selection: $saEffectDraft, minTriggerWidth: 140)
                        .disabled(model.busy).accessibilityIdentifier("v070-sa-effect")
                }
                SettingsRow("颜色", subtitle: "6 位十六进制 RGB（如 #FFFFFF）", showsDivider: true) {
                    HStack(spacing: 8) {
                        ForEach(Self.saPresetColors, id: \.self) { hex in
                            let isSelected = normalizedSaHex == hex
                            Button { saColorHexDraft = hex } label: {
                                Circle().fill(Color(hex: hex)).frame(width: 18, height: 18)
                                    .overlay(Circle().strokeBorder(isSelected ? UITheme.textPrimary : UITheme.hairlineBorder, lineWidth: isSelected ? 2 : 1))
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain).focusEffectDisabled().disabled(model.busy).accessibilityIdentifier("v070-sa-color-\(hex.dropFirst())")
                        }
                        TextField("#FFFFFF", text: $saColorHexDraft)
                            .textFieldStyle(.roundedBorder).controlSize(.small).frame(width: 84).disabled(model.busy).accessibilityIdentifier("v070-sa-color-field")
                    }
                }
                percentSliderRow("动态亮度", value: $saBrightnessPercentDraft, id: "v070-sa-brightness")
                percentSliderRow("动态速率", value: $saSpeedPercentDraft, id: "v070-sa-speed")
                SettingsRow("下发或重置", subtitle: saValidationHint ?? "运行时覆盖单键灯光；重置恢复默认背景灯效") {
                    HStack(spacing: 8) {
                        Button("重置默认") {
                            if let sa = parsedSaId { dispatch(V070HardwareCommand.resetSaRgb(sa: sa)) }
                        }
                        .surfaceButton(.ghost).disabled(model.busy || parsedSaId == nil).accessibilityIdentifier("v070-sa-reset")
                        Button("应用灯光") {
                            if let sa = parsedSaId, let hex = normalizedSaHex {
                                dispatch(V070HardwareCommand.applySaRgb(
                                    sa: sa, colorHex: hex,
                                    brightness: min(max(saBrightnessPercentDraft / 100.0, 0.0), 1.0),
                                    speed: min(max(saSpeedPercentDraft / 100.0, 0.0), 1.0),
                                    effect: saEffectDraft
                                ))
                            }
                        }
                        .surfaceButton(.secondary).disabled(model.busy || parsedSaId == nil || normalizedSaHex == nil).accessibilityIdentifier("v070-sa-apply")
                    }
                }
            }
        }
    }

    private func toggleRow(_ title: String, subtitle: String, isOn: Binding<Bool>, id: String) -> some View {
        SettingsRow(title, subtitle: subtitle, showsDivider: true) {
            Toggle("", isOn: isOn).labelsHidden().toggleStyle(NeutralSwitchToggleStyle()).disabled(model.busy).accessibilityIdentifier(id)
        }
    }

    private func percentSliderRow(_ title: String, value: Binding<Double>, id: String) -> some View {
        SettingsRow(title, subtitle: "映射为固件 0.0–1.0 单位浮点", showsDivider: true) {
            NeutralTrackSlider(value: value, in: 0...100, trackWidth: 132).disabled(model.busy).accessibilityIdentifier(id)
        }
    }

    private func intStringBinding(_ text: Binding<String>, fallback: Int) -> Binding<Double> {
        Binding(
            get: { Double(Int(text.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines)) ?? fallback) },
            set: { text.wrappedValue = String(Int($0.rounded())) }
        )
    }

    private func mouseSliderRow(
        _ title: String,
        subtitle: String,
        text: Binding<String>,
        defaultRange: ClosedRange<Int>,
        unit: String,
        id: String
    ) -> some View {
        let current = Int(text.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines)) ?? defaultRange.lowerBound
        let bounds = Double(min(defaultRange.lowerBound, current))...Double(max(defaultRange.upperBound, current))
        return SettingsRow(title, subtitle: subtitle, showsDivider: true) {
            HStack(spacing: 10) {
                NeutralTrackSlider(
                    value: intStringBinding(text, fallback: defaultRange.lowerBound),
                    in: bounds,
                    trackWidth: 132,
                    showsPercentage: false
                )
                .disabled(model.busy)
                .accessibilityIdentifier(id)
                Text("\(current) \(unit)")
                    .font(UITheme.monoBadge)
                    .foregroundStyle(UITheme.textSecondary)
                    .frame(minWidth: 78, alignment: .trailing)
            }
        }
    }

    private var unavailableTitle: String {
        switch model.hostSleepStatus {
        case "sleeping": return "Mac 正在休眠，0.7.0 能力已暂停"
        case "waking": return "Mac 已唤醒，正在重连 0.7.0 设备"
        default: return "0.7.0 硬件能力仅在连接 0.7.0+ 固件时可用"
        }
    }

    private var unavailableSubtitle: String {
        var parts = ["需通过 USB 专用独占接口连接 0.7.0-rc.16 及以上版本固件。"]
        if !model.statusMessage.isEmpty { parts.append(model.statusMessage) }
        if let err = model.lastError, !err.isEmpty, err != model.statusMessage { parts.append(err) }
        return parts.joined(separator: " ")
    }

    private var hostSleepBadgeText: String {
        model.hostSleepStatus == "sleeping" ? "宿主休眠" : (model.hostSleepStatus == "waking" ? "唤醒同步中" : "宿主清醒")
    }

    private var profileLayerBadgeText: String {
        "\(model.activeProfile.map { "P\($0)" } ?? "P-") · \(model.activeLayer.map { "第 \($0) 层" } ?? "层未知")"
    }

    private var batterySummaryText: String {
        let pct = model.batteryPercent.map { "\($0)%" } ?? "电量未知"
        let chg = model.isCharging.map { $0 ? "充电中" : "未充电" } ?? "充电状态未知"
        let power = model.synced ? (model.pmState.linePowered ? "USB 外接供电" : "电池供电") : "供电状态同步中"
        return "\(pct) · \(chg) · \(power)"
    }

    private var pmStateSummaryText: String {
        guard model.synced else { return "等待读取 pm.state 电源状态快照…" }
        let st = model.pmState.state.isEmpty ? "-" : model.pmState.state
        let fl = model.pmState.floor.isEmpty ? "-" : model.pmState.floor
        let bfl = model.pmState.baseFloor.isEmpty ? "-" : model.pmState.baseFloor
        return "state=\(st) · floor=\(fl)（base=\(bfl)）· idle \(model.pmState.idleS)s / sleep \(model.pmState.sleepAfterS)s"
    }

    private var trimmedDeviceName: String { deviceNameDraft.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var maxDeviceNameBytes: Int {
        model.deviceName.maxLength > 0 ? min(model.deviceName.maxLength, 15) : 15
    }

    private var canApplyDeviceName: Bool {
        let bytes = trimmedDeviceName.utf8.count
        return !model.busy && bytes > 0 && bytes <= maxDeviceNameBytes
    }

    private var deviceNameSubtitle: String {
        let factory = model.deviceName.factoryName.isEmpty ? "出厂名称未知" : "出厂：\(model.deviceName.factoryName)"
        let bytes = trimmedDeviceName.utf8.count
        let hint = bytes > maxDeviceNameBytes ? "超出上限（\(bytes)/\(maxDeviceNameBytes) 字节）" : "\(bytes)/\(maxDeviceNameBytes) 字节"
        return "\(factory) · \(hint)"
    }

    private var sleepDurationOptions: [SelectOption<Int>] {
        let values = Set(Self.baseSleepMinutes + ((1...1440).contains(pmSleepAfterMinDraft) ? [pmSleepAfterMinDraft] : []))
        return values.sorted().map { mins in
            SelectOption(value: mins, title: mins >= 60 && mins % 60 == 0 ? "\(mins / 60) 小时 (\(mins)m)" : "\(mins) 分钟")
        }
    }

    private var parsedSaId: Int? {
        guard let val = Int(saIdText.trimmingCharacters(in: .whitespacesAndNewlines)), (1...0xFF_FFFF).contains(val) else { return nil }
        return val
    }

    private var normalizedSaHex: String? {
        let trimmed = saColorHexDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        let stripped = trimmed.hasPrefix("#") ? String(trimmed.dropFirst())
            : (trimmed.hasPrefix("0x") || trimmed.hasPrefix("0X") ? String(trimmed.dropFirst(2)) : trimmed)
        guard stripped.count == 6,
              stripped.unicodeScalars.allSatisfy({ ("0"..."9").contains($0) || ("a"..."f").contains($0) || ("A"..."F").contains($0) })
        else { return nil }
        return "#" + stripped.uppercased()
    }

    private var saValidationHint: String? {
        if parsedSaId == nil { return "请输入有效的非零 Smart Action 编号（1–16777215）" }
        if normalizedSaHex == nil { return "请输入 6 位十六进制 RGB 颜色（例如 #FFFFFF）" }
        return nil
    }

    private func presetDisplayName(_ raw: String) -> String {
        switch raw {
        case "slow": return "慢速"
        case "standard": return "标准"
        case "fast": return "快速"
        default: return "自定义"
        }
    }

    private static func unitToPercent(_ unit: Double) -> Double {
        unit.isFinite ? min(max((unit * 100.0).rounded(), 0.0), 100.0) : 100.0
    }

    private static func validApplyEffect(_ raw: Int) -> Int { [1, 3, 4, 6].contains(raw) ? raw : 1 }
}
