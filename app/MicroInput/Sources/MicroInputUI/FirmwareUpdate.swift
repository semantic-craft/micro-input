import AppKit
import SwiftUI

public struct FirmwarePortModel: Equatable, Identifiable {
    public var id: String { path }
    public let path: String
    public let product: String
    public let label: String

    public init(path: String, product: String = "USB 设备", label: String? = nil) {
        self.path = path
        self.product = product
        self.label = label ?? "\(product) · \(path)"
    }
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
            return FirmwarePortModel(path: path, product: product, label: "\(product) · \(path)")
        }
    }

    public var canCheck: Bool { !busy && !deviceReserved }
    public var canDownload: Bool { canCheck && releaseTag != nil }
    public var canPrepare: Bool { !busy && !deviceReserved && downloaded && digestVerified }
    public var canInstall: Bool {
        !busy && deviceReserved && downloaded && digestVerified && selectedPort != nil
            && ports.contains { $0.path == selectedPort }
    }
    public var preventsTermination: Bool { busy && deviceReserved }
    public var currentVersionLabel: String { currentVersionLabel(.zhHans) }
    public func currentVersionLabel(_ locale: ResolvedLocale) -> String {
        currentVersion ?? L10n.tr("未知（尚未从设备读取）", locale)
    }
}

public struct FirmwareUpdateSettingsSection: View {
    let model: FirmwareSliceModel
    let locale: ResolvedLocale
    let onConnectKeyboard: (() -> Void)?
    let dispatch: (Any) -> Void
    @State private var confirmation: Confirmation?
    private enum Confirmation: String, Identifiable {
        case prepare
        case install

        var id: String { rawValue }

        func title(_ locale: ResolvedLocale) -> String {
            L10n.tr(self == .prepare ? "准备更新设备固件？" : "确认安装到所选 Creator Micro 2？", locale)
        }

        func actionTitle(_ locale: ResolvedLocale) -> String {
            L10n.tr(self == .prepare ? "暂停控制并准备" : "确认设备并安装", locale)
        }
    }

    public init(
        model: FirmwareSliceModel,
        locale: ResolvedLocale = .zhHans,
        onConnectKeyboard: (() -> Void)? = nil,
        dispatch: @escaping (Any) -> Void
    ) {
        self.model = model
        self.locale = locale
        self.onConnectKeyboard = onConnectKeyboard
        self.dispatch = dispatch
    }

    private func tr(_ zh: String) -> String { L10n.tr(zh, locale) }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(tr("设备固件"))
            SettingsCard {
                SettingsRow(
                    "Creator Micro 2",
                    subtitle: locale == .en
                        ? "Current firmware: \(model.currentVersionLabel(locale))"
                        : "当前固件：\(model.currentVersionLabel(locale))",
                    showsDivider: true
                ) {
                    HStack(spacing: 8) {
                        if !model.deviceReserved && model.currentVersion == nil, let onConnectKeyboard {
                            Button(tr("连接有线键盘")) { onConnectKeyboard() }
                                .surfaceButton(.secondary)
                                .accessibilityIdentifier("firmware-connect")
                        }
                        Button(tr("检查固件更新")) { dispatch(["Check": ["channel": model.channel]]) }
                            .surfaceButton(.secondary)
                            .disabled(!model.canCheck)
                            .accessibilityIdentifier("firmware-check")
                    }
                }
                SettingsRow(
                    tr("更新通道"),
                    subtitle: tr(model.channel == "prerelease" ? "预发布固件与现有键位和配置协议的兼容性尚未验证。" : "只检查官方稳定版。"),
                    showsDivider: true
                ) {
                    SelectDropdown(
                        options: [SelectOption(value: "stable", title: tr("稳定版")), SelectOption(value: "prerelease", title: tr("预发布版"))],
                        selection: model.channel
                    ) { dispatch(["Check": ["channel": $0]]) }
                    .disabled(!model.canCheck)
                    .accessibilityIdentifier("firmware-channel")
                }
                if !model.versionStatus.isEmpty {
                    SettingsRow(tr("版本比较"), subtitle: model.versionStatus, showsDivider: true)
                }
                if let tag = model.releaseTag {
                    SettingsRow(
                        locale == .en ? "Available release: \(tag)" : "可下载版本：\(tag)",
                        subtitle: tr(model.downloaded ? (model.digestVerified ? "下载完成，官方 SHA-256 摘要核对通过。" : "官方未提供摘要；可下载查看，不能自动安装。") : "从 Work Louder 官方 GitHub 发布下载。"),
                        showsDivider: true
                    ) {
                        Button(tr(model.downloaded ? "重新下载" : "下载并校验")) { dispatch("Download") }
                            .surfaceButton(.secondary)
                            .disabled(!model.canDownload)
                            .accessibilityIdentifier("firmware-download")
                    }
                    if !model.releaseNotes.isEmpty {
                        DisclosureGroup(tr("发行说明")) {
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
                SettingsRow(
                    tr("准备设备"),
                    subtitle: tr(model.deviceReserved ? "按键映射已暂停。选择这把键盘对应的引导模式串口。" : "会暂停按键映射并请求进入引导模式，键盘暂时不可用。"),
                    showsDivider: true
                ) {
                    Button(tr("准备更新")) { confirmation = .prepare }
                        .surfaceButton(.secondary)
                        .disabled(!model.canPrepare)
                        .accessibilityIdentifier("firmware-prepare")
                }
                if model.deviceReserved {
                    SettingsRow(
                        tr("引导模式设备"),
                        subtitle: tr("Espressif 名称不能确认键盘型号。请确认所选串口属于这把 Creator Micro 2。"),
                        showsDivider: true
                    ) {
                        VStack(alignment: .trailing, spacing: 8) {
                            SelectDropdown(
                                options: model.ports.map {
                                    SelectOption(value: $0.path, title: "\(tr($0.product)) · \($0.path)")
                                },
                                selection: model.selectedPort ?? "",
                                overrideLabel: model.selectedPort ?? tr("请选择设备串口")
                            ) { dispatch(["SelectPort": ["path": $0]]) }
                            .disabled(model.busy || model.ports.isEmpty)
                            .accessibilityIdentifier("firmware-port")
                            Button(tr("查找串口")) { dispatch("ScanPorts") }
                                .surfaceButton(.secondary)
                                .disabled(model.busy)
                                .accessibilityIdentifier("firmware-scan")
                        }
                    }
                    SettingsRow(
                        tr("安装固件"),
                        subtitle: tr("先保存完整 Flash 备份，再写入并校验。期间请勿断开 USB 或退出应用。"),
                        showsDivider: true
                    ) {
                        Button(tr("安装固件…")) { confirmation = .install }
                            .surfaceButton(.secondary)
                            .disabled(!model.canInstall)
                            .accessibilityIdentifier("firmware-install")
                    }
                }
                SettingsRow(tr(model.phase == "flash_verified" ? "写入校验通过" : "更新状态"), subtitle: tr(model.message)) {
                    if model.busy {
                        if let progress = model.progress {
                            ProgressView(value: progress).frame(width: 100)
                        } else {
                            ProgressView().controlSize(.small)
                        }
                    }
                }
                if model.backupPath != nil || model.deviceReserved {
                    SettingsRow(tr("结束与备份"), subtitle: tr("完成后需重新连接并核对运行版本与原键位；写入校验不代表运行验收。")) {
                        HStack(spacing: 8) {
                            if let path = model.backupPath {
                                Button(tr("显示备份")) {
                                    NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
                                }
                                .surfaceButton(.secondary)
                                .accessibilityIdentifier("firmware-reveal-backup")
                            }
                            if model.deviceReserved {
                                Button(tr("结束更新")) { dispatch("Finish") }
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
            confirmation?.title(locale) ?? "",
            isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }),
            presenting: confirmation
        ) { kind in
            Button(tr("取消"), role: .cancel) { confirmation = nil }
            Button(kind.actionTitle(locale), role: kind == .install ? .destructive : nil) {
                if kind == .prepare, model.canPrepare { dispatch("PrepareDevice") }
                if kind == .install, model.canInstall { dispatch("Install") }
                confirmation = nil
            }
        } message: { kind in
            if kind == .prepare {
                Text(tr("Micro Input 将暂停宿主按键映射并请求键盘进入引导模式，键盘会暂时停止工作。下载的固件不会在此步骤写入。"))
            } else if locale == .en {
                let port = model.selectedPort ?? "none"
                let tag = model.releaseTag ?? "firmware"
                let tail = model.prerelease
                    ? "Prerelease compatibility with the current config protocol has not been verified."
                    : "Verify the running firmware version and keymap after reboot."
                Text("Confirm serial port \(port) belongs to your Creator Micro 2. A full Flash backup will be saved before installing \(tag). Interrupted writes may require recovery. \(tail)")
            } else {
                Text("请确认串口 \(model.selectedPort ?? "未选择") 属于你的 Creator Micro 2。将先保存完整 Flash 备份，再安装 \(model.releaseTag ?? "固件")。写入中断可能需要恢复，备份不代表已验证恢复成功。\(model.prerelease ? "预发布固件与当前配置协议的兼容性尚未验证。" : "更新后仍需核对运行版本和原键位。")")
            }
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
    public var activeProfile: Int?, activeLayer: Int?, activeJoystickMode: String?, batteryPercent: Int?, isCharging: Bool?
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
        activeJoystickMode = json["active_joystick_mode"] as? String
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
    public static func refresh() -> String { "Refresh" }
    public static func setAutoLayerSync(_ enabled: Bool) -> [String: Any] { ["SetAutoLayerSync": ["enabled": enabled]] }
    public static func setSessionSaRgbSync(_ enabled: Bool) -> [String: Any] { ["SetSessionSaRgbSync": ["enabled": enabled]] }
    public static func selectLayer(_ layer: Int) -> [String: Any] { ["SelectLayer": ["layer": layer]] }
    public static func applyJoystickHardwareMode(_ mode: String) -> [String: Any] { ["ApplyJoystickHardwareMode": ["mode": mode]] }
    public static func applyMousePreset(_ preset: String) -> [String: Any] { ["ApplyMousePreset": ["preset": preset]] }
    public static func renameDevice(name: String) -> [String: Any] { ["RenameDevice": ["name": name]] }
    public static func resetSaRgb(sa: Int) -> [String: Any] { ["ResetSaRgb": ["sa": sa]] }

    public static func applyMouseConfig(
        moveDelta: Int, maxSpeed: Int, wheelDelta: Int, wheelMaxSpeed: Int,
        stickSpeed: Int, stickScroll: Int
    ) -> [String: Any] {
        ["ApplyMouseConfig": [
            "move_delta": moveDelta, "max_speed": maxSpeed, "wheel_delta": wheelDelta,
            "wheel_max_speed": wheelMaxSpeed, "stick_speed": stickSpeed, "stick_scroll": stickScroll,
        ]]
    }

    public static func applyJoystickDeadzone(_ deadzone: Double) -> [String: Any] {
        ["ApplyJoystickDeadzone": ["deadzone": deadzone]]
    }

    public static func applyPmConfig(autoSleep: Bool, hostSleep: Bool, sleepAfterMin: Int) -> [String: Any] {
        ["ApplyPmConfig": [
            "auto_sleep": autoSleep, "host_sleep": hostSleep, "sleep_after_min": sleepAfterMin,
        ]]
    }

    public static func applySaRgb(
        sa: Int, colorHex: String, brightness: Double, speed: Double, effect: Int
    ) -> [String: Any] {
        ["ApplySaRgb": [
            "sa": sa, "color_hex": colorHex, "brightness": brightness, "speed": speed, "effect": effect,
        ]]
    }
}

struct V070MouseDraft: Equatable {
    var moveDelta: String, maxSpeed: String, wheelDelta: String, wheelMaxSpeed: String
    var stickSpeed: String, stickScroll: String

    init(from mouse: V070MouseConfigModel) {
        moveDelta = String(mouse.moveDelta)
        maxSpeed = String(mouse.maxSpeed)
        wheelDelta = String(mouse.wheelDelta)
        wheelMaxSpeed = String(mouse.wheelMaxSpeed)
        stickSpeed = String(mouse.stickSpeed)
        stickScroll = String(mouse.stickScroll)
    }

    static func isValidDeadzone(_ value: Double) -> Bool {
        value.isFinite && (0.0025...0.95).contains(value)
    }

    var validated: (Int, Int, Int, Int, Int, Int)? {
        guard let md = Self.parseInt(moveDelta, in: 1...127),
              let ms = Self.parseInt(maxSpeed, in: 1...100),
              let wd = Self.parseInt(wheelDelta, in: 1...127),
              let ws = Self.parseInt(wheelMaxSpeed, in: 1...100),
              let ss = Self.parseInt(stickSpeed, in: 1...255),
              let sc = Self.parseInt(stickScroll, in: 1...255)
        else { return nil }
        return (md, ms, wd, ws, ss, sc)
    }

    var validationHint: String? { validationHint(.zhHans) }

    func validationHint(_ locale: ResolvedLocale) -> String? {
        validated == nil ? L10n.tr("请确认步进在 1–127、加速上限在 1–100、摇杆参数在 1–255", locale) : nil
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
    let locale: ResolvedLocale
    let dispatch: (Any) -> Void

    @State private var deviceNameDraft: String
    @State private var deadzoneDraft: Double
    @State private var mouseCurveDraft: V070MouseDraft
    @State private var showAdvancedMouseParams: Bool
    @State private var pmAutoSleepDraft: Bool
    @State private var pmHostSleepDraft: Bool
    @State private var pmSleepAfterMinDraft: Int

    private static let mousePresets: [(id: String, title: String)] = [
        ("slow", "慢速"), ("standard", "标准"), ("fast", "快速"), ("precise", "精准"),
    ]
    private static let joystickModes: [(id: String, title: String)] = [
        ("mouse", "硬件 MOUSE"), ("scroll", "硬件 SCROLL"), ("logical_sectors", "宿主扇区"),
    ]
    private static let baseSleepMinutes = [1, 5, 10, 15, 30, 60, 120, 240, 480, 1440]

    public init(
        model: V070HardwareSliceModel,
        locale: ResolvedLocale = .zhHans,
        dispatch: @escaping (Any) -> Void
    ) {
        self.model = model
        self.locale = locale
        self.dispatch = dispatch
        _deviceNameDraft = State(initialValue: model.deviceName.name)
        _deadzoneDraft = State(initialValue: model.mouse.deadzone)
        _mouseCurveDraft = State(initialValue: V070MouseDraft(from: model.mouse))
        _showAdvancedMouseParams = State(initialValue: model.mouse.preset == "custom")
        _pmAutoSleepDraft = State(initialValue: model.pmConfig.autoSleep)
        _pmHostSleepDraft = State(initialValue: model.pmConfig.hostSleep)
        _pmSleepAfterMinDraft = State(initialValue: model.pmConfig.sleepAfterMin)
    }

    private func tr(_ zh: String) -> String { L10n.tr(zh, locale) }

    public var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            overviewSection
            if model.supported {
                mouseSection
                pmSection
            }
        }
        .accessibilityIdentifier("v070-hardware-settings")
        .onChange(of: model.synced) { _, synced in
            guard synced else { return }
            deviceNameDraft = model.deviceName.name
            deadzoneDraft = model.mouse.deadzone
            mouseCurveDraft = V070MouseDraft(from: model.mouse)
            pmAutoSleepDraft = model.pmConfig.autoSleep
            pmHostSleepDraft = model.pmConfig.hostSleep
            pmSleepAfterMinDraft = model.pmConfig.sleepAfterMin
        }
        .onChange(of: model.deviceName.name) { _, next in deviceNameDraft = next }
        .onChange(of: model.mouse.deadzone) { _, next in deadzoneDraft = next }
        .onChange(of: V070MouseDraft(from: model.mouse)) { _, next in mouseCurveDraft = next }
        .onChange(of: model.pmConfig) { _, next in
            pmAutoSleepDraft = next.autoSleep
            pmHostSleepDraft = next.hostSleep
            pmSleepAfterMinDraft = next.sleepAfterMin
        }
    }

    private var overviewSection: some View {
        let hasSerial = !(model.preferredDeviceSerial?.isEmpty ?? true)
        return VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(tr("设备识别与状态同步")) {
                HStack(spacing: 8) {
                    if model.busy { ProgressView().controlSize(.small) }
                    Button(tr("刷新状态")) { dispatch(V070HardwareCommand.refresh()) }
                        .surfaceButton(.secondary)
                        .disabled(!model.supported || model.busy)
                        .accessibilityIdentifier("v070-refresh")
                }
            }
            SettingsCard {
                if !model.supported {
                    SettingsRow(unavailableTitle, subtitle: unavailableSubtitle, showsDivider: hasSerial) { V070Badge(text: hostSleepBadgeText) }
                } else {
                    SettingsRow(tr("设备名称"), subtitle: deviceNameSubtitle, showsDivider: true) {
                        HStack(spacing: 8) {
                            TextField(tr("设备名称"), text: $deviceNameDraft)
                                .textFieldStyle(.roundedBorder).controlSize(.small).frame(width: 136).disabled(model.busy)
                                .accessibilityIdentifier("v070-device-name-field")
                            Button(tr("应用名称")) { dispatch(V070HardwareCommand.renameDevice(name: trimmedDeviceName)) }
                                .surfaceButton(.secondary).disabled(!canApplyDeviceName).accessibilityIdentifier("v070-device-rename")
                        }
                    }
                    SettingsRow(tr("状态同步"), subtitle: tr(model.lastError ?? model.statusMessage), showsDivider: hasSerial) {
                        if let err = model.lastError, !err.isEmpty {
                            Text(tr("异常")).font(UITheme.micro).foregroundStyle(.red)
                        } else {
                            V070Badge(text: tr(model.synced ? "已同步" : "待同步"), accentDot: model.synced)
                        }
                    }
                }
                if let serial = model.preferredDeviceSerial, !serial.isEmpty {
                    SettingsRow(tr("已记忆设备序列号"), subtitle: tr("多台 Creator Micro 2 同时接入时将优先匹配此设备"), showsDivider: false) {
                        V070Badge(text: serial)
                    }
                }
            }
        }
    }

    private var mouseSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(tr("鼠标速度与摇杆死区"))
            Text(tr("速度参数仅对绑定了硬件鼠标或滚轮的按键层生效；摇杆死区同时影响轮盘菜单离中阈值。"))
                .font(UITheme.caption)
                .foregroundStyle(UITheme.textSecondary)
            SettingsCard {
                SettingsRow(
                    tr("摇杆固件模式"),
                    subtitle: locale == .en
                        ? "Active firmware mode: \(joystickModeDisplayName(model.activeJoystickMode))"
                        : "当前键盘固件：\(joystickModeDisplayName(model.activeJoystickMode))",
                    showsDivider: true
                ) {
                    HStack(spacing: 6) {
                        ForEach(Self.joystickModes, id: \.id) { item in
                            Button(tr(item.title)) { dispatch(V070HardwareCommand.applyJoystickHardwareMode(item.id)) }
                                .surfaceButton(model.activeJoystickMode == item.id ? .primary : .secondary)
                                .disabled(model.busy)
                                .accessibilityIdentifier("v070-joystick-mode-\(item.id)")
                        }
                    }
                }
                SettingsRow(
                    tr("速度预设"),
                    subtitle: locale == .en
                        ? "Current preset: \(presetDisplayName(model.mouse.preset)) (applies immediately)"
                        : "当前档位：\(presetDisplayName(model.mouse.preset))（立即生效）",
                    showsDivider: true
                ) {
                    HStack(spacing: 6) {
                        ForEach(Self.mousePresets, id: \.id) { item in
                            Button(tr(item.title)) { dispatch(V070HardwareCommand.applyMousePreset(item.id)) }
                                .surfaceButton(model.mouse.preset == item.id ? .primary : .secondary)
                                .disabled(model.busy)
                                .accessibilityIdentifier("v070-mouse-preset-\(item.id)")
                        }
                    }
                }
                SettingsRow(tr("摇杆死区"), subtitle: tr("防漂移盲区（出厂默认 0.15，UI 范围 0.02–0.45）"), showsDivider: true) {
                    HStack(spacing: 10) {
                        NeutralTrackSlider(
                            value: Binding(get: { deadzoneDraft }, set: { deadzoneDraft = (($0 * 100.0).rounded() / 100.0) }),
                            in: 0.02...0.45, trackWidth: 132, showsPercentage: false
                        ).disabled(model.busy).accessibilityIdentifier("v070-deadzone-slider")
                        Text(String(format: "%.2f", deadzoneDraft)).font(UITheme.monoBadge).foregroundStyle(UITheme.textSecondary).frame(width: 42, alignment: .trailing)
                        if abs(deadzoneDraft - model.mouse.deadzone) > 0.001 {
                            Button(tr("保存死区")) { dispatch(V070HardwareCommand.applyJoystickDeadzone(deadzoneDraft)) }
                                .surfaceButton(.secondary)
                                .disabled(model.busy || !V070MouseDraft.isValidDeadzone(deadzoneDraft))
                                .accessibilityIdentifier("v070-deadzone-save")
                        }
                    }
                }
                DisclosureGroup(tr("高级曲线参数"), isExpanded: $showAdvancedMouseParams) {
                    VStack(spacing: 0) {
                        Rectangle().fill(UITheme.rowDivider).frame(height: 1).padding(.top, 10)
                        mouseSliderRow(tr("指针初速"), subtitle: tr("按键鼠标单步位移（推荐 4–32，固件上限 127）"), text: $mouseCurveDraft.moveDelta, defaultRange: 1...32, unit: "px/tick", id: "v070-mouse-move-delta-slider")
                        mouseSliderRow(tr("指针上限"), subtitle: tr("按键鼠标加速上限（推荐 4–40，固件上限 100）"), text: $mouseCurveDraft.maxSpeed, defaultRange: 1...40, unit: "px/tick", id: "v070-mouse-max-speed-slider")
                        mouseSliderRow(tr("滚轮步进"), subtitle: tr("旋钮或按键单步滚动（推荐 1–16，固件上限 127）"), text: $mouseCurveDraft.wheelDelta, defaultRange: 1...16, unit: "x/tick", id: "v070-mouse-wheel-delta-slider")
                        mouseSliderRow(tr("滚轮上限"), subtitle: tr("旋钮或按键滚轮上限（推荐 1–32，固件上限 100）"), text: $mouseCurveDraft.wheelMaxSpeed, defaultRange: 1...32, unit: "x/tick", id: "v070-mouse-wheel-max-speed-slider")
                        mouseSliderRow(tr("摇杆指针"), subtitle: tr("模拟摇杆满偏转步长（推荐 1–16，固件上限 255）"), text: $mouseCurveDraft.stickSpeed, defaultRange: 1...16, unit: "px/tick", id: "v070-mouse-stick-speed-slider")
                        mouseSliderRow(tr("摇杆滚轮"), subtitle: tr("模拟摇杆滚轮分频间隔（10–120，越小越快；上限 255）"), text: $mouseCurveDraft.stickScroll, defaultRange: 10...120, unit: "ticks", id: "v070-mouse-stick-scroll-slider")
                        SettingsRow(tr("自定义参数"), subtitle: mouseCurveDraft.validationHint(locale) ?? tr("仅将上方 6 项自定义曲线参数写入设备"), showsDivider: false) {
                            Button(tr("应用鼠标参数")) { applyMouseCurveDraft() }
                                .surfaceButton(.secondary).disabled(model.busy || mouseCurveDraft.validated == nil).accessibilityIdentifier("v070-mouse-apply")
                        }
                    }
                }
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(UITheme.textPrimary)
                .tint(UITheme.textPrimary)
                .padding(.horizontal, 20)
                .padding(.vertical, 14)
            }
        }
    }

    private var pmSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(tr("休眠跟随与电源策略"))
            SettingsCard {
                toggleRow(
                    tr("自动休眠"),
                    subtitle: tr("空闲达到设定时长后自动进入低功耗待机"),
                    isOn: Binding(
                        get: { pmAutoSleepDraft },
                        set: { next in
                            pmAutoSleepDraft = next
                            dispatchPmConfig(autoSleep: next, hostSleep: pmHostSleepDraft, sleepAfterMin: pmSleepAfterMinDraft)
                        }
                    ),
                    id: "v070-pm-auto-sleep"
                )
                toggleRow(
                    tr("跟随宿主休眠"),
                    subtitle: tr("随 Mac USB 挂起与唤醒动态调整设备电源地板"),
                    isOn: Binding(
                        get: { pmHostSleepDraft },
                        set: { next in
                            pmHostSleepDraft = next
                            dispatchPmConfig(autoSleep: pmAutoSleepDraft, hostSleep: next, sleepAfterMin: pmSleepAfterMinDraft)
                        }
                    ),
                    id: "v070-pm-host-sleep"
                )
                SettingsRow(
                    tr("休眠等待时长"),
                    subtitle: locale == .en
                        ? "Valid range 1–1440 min (currently \(pmSleepAfterMinDraft) min)"
                        : "有效范围 1–1440 分钟（当前选中 \(pmSleepAfterMinDraft) 分钟）",
                    showsDivider: false
                ) {
                    SelectDropdown(
                        options: sleepDurationOptions,
                        selection: Binding(
                            get: { pmSleepAfterMinDraft },
                            set: { next in
                                pmSleepAfterMinDraft = next
                                dispatchPmConfig(autoSleep: pmAutoSleepDraft, hostSleep: pmHostSleepDraft, sleepAfterMin: next)
                            }
                        ),
                        minTriggerWidth: 120
                    )
                    .disabled(model.busy).accessibilityIdentifier("v070-pm-sleep-after")
                }
            }
        }
    }

    private func applyMouseCurveDraft() {
        if let v = mouseCurveDraft.validated {
            dispatch(V070HardwareCommand.applyMouseConfig(
                moveDelta: v.0, maxSpeed: v.1, wheelDelta: v.2,
                wheelMaxSpeed: v.3, stickSpeed: v.4, stickScroll: v.5
            ))
        }
    }

    private func dispatchPmConfig(autoSleep: Bool, hostSleep: Bool, sleepAfterMin: Int) {
        guard (1...1440).contains(sleepAfterMin) else { return }
        dispatch(V070HardwareCommand.applyPmConfig(autoSleep: autoSleep, hostSleep: hostSleep, sleepAfterMin: sleepAfterMin))
    }

    private func toggleRow(_ title: String, subtitle: String, isOn: Binding<Bool>, id: String) -> some View {
        SettingsRow(title, subtitle: subtitle, showsDivider: true) {
            Toggle("", isOn: isOn).labelsHidden().toggleStyle(NeutralSwitchToggleStyle()).disabled(model.busy).accessibilityIdentifier(id)
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
        case "sleeping": return tr("Mac 正在休眠，扩展硬件配置已暂停")
        case "waking": return tr("Mac 已唤醒，正在重连设备")
        default: return tr("扩展硬件配置暂不可用")
        }
    }

    private var unavailableSubtitle: String {
        var parts = [tr("需通过 USB 连接支持扩展控制的 Creator Micro 2 固件。")]
        if !model.statusMessage.isEmpty { parts.append(tr(model.statusMessage)) }
        if let err = model.lastError, !err.isEmpty, err != model.statusMessage { parts.append(tr(err)) }
        return parts.joined(separator: " ")
    }

    private var hostSleepBadgeText: String {
        tr(model.hostSleepStatus == "sleeping" ? "宿主休眠" : (model.hostSleepStatus == "waking" ? "唤醒同步中" : "宿主清醒"))
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
        let factory: String
        if model.deviceName.factoryName.isEmpty {
            factory = tr("出厂名称未知")
        } else {
            factory = locale == .en ? "Factory: \(model.deviceName.factoryName)" : "出厂：\(model.deviceName.factoryName)"
        }
        let bytes = trimmedDeviceName.utf8.count
        let hint: String
        if locale == .en {
            hint = bytes > maxDeviceNameBytes
                ? "Exceeds limit (\(bytes)/\(maxDeviceNameBytes) bytes)"
                : "\(bytes)/\(maxDeviceNameBytes) bytes"
        } else {
            hint = bytes > maxDeviceNameBytes
                ? "超出上限（\(bytes)/\(maxDeviceNameBytes) 字节）"
                : "\(bytes)/\(maxDeviceNameBytes) 字节"
        }
        return "\(factory) · \(hint)"
    }

    private var sleepDurationOptions: [SelectOption<Int>] {
        let values = Set(Self.baseSleepMinutes + ((1...1440).contains(pmSleepAfterMinDraft) ? [pmSleepAfterMinDraft] : []))
        return values.sorted().map { mins in
            let title: String
            if locale == .en {
                title = mins >= 60 && mins % 60 == 0 ? "\(mins / 60) hr (\(mins)m)" : "\(mins) min"
            } else {
                title = mins >= 60 && mins % 60 == 0 ? "\(mins / 60) 小时 (\(mins)m)" : "\(mins) 分钟"
            }
            return SelectOption(value: mins, title: title)
        }
    }

    private func presetDisplayName(_ raw: String) -> String {
        switch raw {
        case "slow": return tr("慢速")
        case "standard": return tr("标准")
        case "fast": return tr("快速")
        case "precise": return tr("精准")
        default: return tr("自定义")
        }
    }

    private func joystickModeDisplayName(_ raw: String?) -> String {
        switch raw {
        case "mouse": return tr("硬件指针 (MOUSE)")
        case "scroll": return tr("硬件滚轮 (SCROLL)")
        case "logical_sectors": return tr("宿主逻辑扇区")
        default: return tr("待同步")
        }
    }
}
