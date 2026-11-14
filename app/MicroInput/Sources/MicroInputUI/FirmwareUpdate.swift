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

    public init(model: FirmwareSliceModel, dispatch: @escaping (Any) -> Void) {
        self.model = model
        self.dispatch = dispatch
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader("设备固件")
            SettingsCard {
                SettingsRow("Creator Micro 2", subtitle: "当前固件：\(model.currentVersionLabel)", showsDivider: true) {
                    Button("检查固件更新") { dispatch(["Check": ["channel": model.channel]]) }
                        .surfaceButton(.secondary)
                        .disabled(!model.canCheck)
                        .accessibilityIdentifier("firmware-check")
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
