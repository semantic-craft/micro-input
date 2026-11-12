import AppKit
import Foundation
import Observation
import SwiftUI

public struct AppReleaseIdentity: Equatable, Sendable {
    public static let defaultRepositoryURL = URL(string: "https://github.com/semantic-craft/micro-input")!
    public static let defaultReleasesURL = URL(string: "https://github.com/semantic-craft/micro-input/releases")!

    public let marketingVersion: String
    public let buildVersion: String
    public let bundleIdentifier: String
    public let feedURLString: String?
    public let publicEDKey: String?
    public let automaticallyChecksDefault: Bool
    public let repositoryURL: URL
    public let releasesURL: URL
    public let licenseName: String

    public init(
        marketingVersion: String,
        buildVersion: String,
        bundleIdentifier: String,
        feedURLString: String?,
        publicEDKey: String?,
        automaticallyChecksDefault: Bool = true,
        repositoryURL: URL = AppReleaseIdentity.defaultRepositoryURL,
        releasesURL: URL = AppReleaseIdentity.defaultReleasesURL,
        licenseName: String = "MIT"
    ) {
        self.marketingVersion = marketingVersion
        self.buildVersion = buildVersion
        self.bundleIdentifier = bundleIdentifier
        self.feedURLString = feedURLString
        self.publicEDKey = publicEDKey
        self.automaticallyChecksDefault = automaticallyChecksDefault
        self.repositoryURL = repositoryURL
        self.releasesURL = releasesURL
        self.licenseName = licenseName
    }

    public static func fromBundle(_ bundle: Bundle = .main) -> AppReleaseIdentity {
        let info = bundle.infoDictionary ?? [:]
        let shortVersion = (info["CFBundleShortVersionString"] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let build = (info["CFBundleVersion"] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let bundleId = bundle.bundleIdentifier
            ?? (info["CFBundleIdentifier"] as? String)
            ?? "craft.semantic.micro-input.dev"
        let feedURL = (info["SUFeedURL"] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let publicKey = (info["SUPublicEDKey"] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let autoDefault = (info["SUEnableAutomaticChecks"] as? Bool) ?? true
        return AppReleaseIdentity(
            marketingVersion: (shortVersion?.isEmpty == false) ? shortVersion! : "0.5.0",
            buildVersion: (build?.isEmpty == false) ? build! : "5",
            bundleIdentifier: bundleId,
            feedURLString: (feedURL?.isEmpty == false) ? feedURL : nil,
            publicEDKey: (publicKey?.isEmpty == false) ? publicKey : nil,
            automaticallyChecksDefault: autoDefault
        )
    }

    public var displayVersionLabel: String {
        "v\(marketingVersion) (\(buildVersion))"
    }
}

public enum UpdateServiceStatus: Equatable, Sendable {
    case isolated(reason: String)
    case unavailable(reason: String)
    case ready

    public var statusDescription: String {
        switch self {
        case .isolated(let reason), .unavailable(let reason):
            return reason
        case .ready:
            return "通过 GitHub Releases 签名更新源接收更新"
        }
    }
}

public enum UpdateLaunchPolicy {
    public static func evaluate(
        identity: AppReleaseIdentity,
        bundlePath: String,
        environment: [String: String],
        arguments: [String] = []
    ) -> UpdateServiceStatus {
        if environment["MI_DISABLE_UPDATER"] == "1" {
            return .isolated(reason: "当前会话已通过环境变量停用软件更新")
        }
        if environment["MI_SYNTHETIC_SCENARIO"] != nil {
            return .isolated(reason: "合成验收场景下已停用软件更新")
        }
        if environment["MI_DOCUMENT_DIR"] != nil {
            return .isolated(reason: "隔离数据目录会话下已停用软件更新")
        }
        if environment["XCTestConfigurationFilePath"] != nil {
            return .isolated(reason: "单元测试环境下已停用软件更新")
        }
        if arguments.contains("--isolated") || arguments.contains("--verify") {
            return .isolated(reason: "隔离或验收启动模式下已停用软件更新")
        }
        if identity.bundleIdentifier.hasPrefix("com.semanticcraft.microinput.qa") {
            return .isolated(reason: "QA 隔离副本不连接外部更新源")
        }
        if bundlePath.contains("/target/") || bundlePath.contains("/.build/") {
            return .isolated(reason: "本地构建目录副本不连接外部更新源")
        }
        if bundlePath.contains("/AppTranslocation/") {
            return .unavailable(reason: "应用处于系统隔离运行路径，请移至「应用程序」文件夹后检查更新")
        }
        if bundlePath.hasPrefix("/Volumes/") {
            return .unavailable(reason: "应用正在挂载卷中运行，请移至「应用程序」文件夹后检查更新")
        }
        if !bundlePath.hasSuffix(".app") {
            return .unavailable(reason: "当前未以完整 .app 应用包运行，软件更新不可用")
        }
        guard let feedRaw = identity.feedURLString,
              let feedURL = URL(string: feedRaw),
              feedURL.scheme?.lowercased() == "https",
              let host = feedURL.host,
              !host.isEmpty
        else {
            return .unavailable(reason: "未配置有效的 HTTPS 更新源（SUFeedURL）")
        }
        guard let keyRaw = identity.publicEDKey,
              let keyData = Data(base64Encoded: keyRaw),
              keyData.count == 32
        else {
            return .unavailable(reason: "未配置有效的 Ed25519 更新签名公钥（SUPublicEDKey）")
        }
        return .ready
    }
}

@MainActor
public protocol SoftwareUpdateDriver: AnyObject {
    var canCheckForUpdates: Bool { get }
    var automaticallyChecksForUpdates: Bool { get set }
    var lastUpdateCheckDate: Date? { get }
    func startUpdater()
    func checkForUpdates()
}

@MainActor
@Observable
public final class SoftwareUpdateController {
    private static let timestampFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd HH:mm"
        return formatter
    }()

    public let identity: AppReleaseIdentity
    public private(set) var serviceStatus: UpdateServiceStatus
    public private(set) var canCheckForUpdates: Bool = false
    public private(set) var automaticallyChecksForUpdates: Bool
    public private(set) var lastUpdateCheckDate: Date? = nil

    @ObservationIgnored private let driverFactory: (@MainActor () -> SoftwareUpdateDriver)?
    @ObservationIgnored private var driver: SoftwareUpdateDriver?
    @ObservationIgnored private var hasStartedUpdater: Bool = false

    public init(
        identity: AppReleaseIdentity = .fromBundle(),
        bundlePath: String = Bundle.main.bundlePath,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        arguments: [String] = CommandLine.arguments,
        driverFactory: (@MainActor () -> SoftwareUpdateDriver)? = nil
    ) {
        self.identity = identity
        let status = UpdateLaunchPolicy.evaluate(
            identity: identity,
            bundlePath: bundlePath,
            environment: environment,
            arguments: arguments
        )
        self.serviceStatus = status
        self.automaticallyChecksForUpdates = (status == .ready) ? identity.automaticallyChecksDefault : false
        self.driverFactory = driverFactory
    }

    public var lastCheckSummary: String {
        guard serviceStatus == .ready else {
            return serviceStatus.statusDescription
        }
        if let lastUpdateCheckDate {
            return "上次检查：\(Self.timestampFormatter.string(from: lastUpdateCheckDate))"
        }
        return serviceStatus.statusDescription
    }

    public func startIfPermitted() {
        guard serviceStatus == .ready else { return }
        guard !hasStartedUpdater else {
            refreshFromDriver()
            return
        }
        guard let driverFactory else {
            serviceStatus = .unavailable(reason: "当前构建未挂载 Sparkle 更新驱动")
            canCheckForUpdates = false
            automaticallyChecksForUpdates = false
            return
        }
        let resolvedDriver = driver ?? driverFactory()
        driver = resolvedDriver
        hasStartedUpdater = true
        resolvedDriver.startUpdater()
        refreshFromDriver()
    }

    public func refreshFromDriver() {
        guard serviceStatus == .ready, let driver else {
            if serviceStatus != .ready, canCheckForUpdates {
                canCheckForUpdates = false
            }
            return
        }
        if canCheckForUpdates != driver.canCheckForUpdates {
            canCheckForUpdates = driver.canCheckForUpdates
        }
        if automaticallyChecksForUpdates != driver.automaticallyChecksForUpdates {
            automaticallyChecksForUpdates = driver.automaticallyChecksForUpdates
        }
        if lastUpdateCheckDate != driver.lastUpdateCheckDate {
            lastUpdateCheckDate = driver.lastUpdateCheckDate
        }
    }

    public func setAutomaticallyChecks(_ enabled: Bool) {
        guard serviceStatus == .ready else { return }
        if !hasStartedUpdater {
            startIfPermitted()
        }
        guard let driver else { return }
        driver.automaticallyChecksForUpdates = enabled
        refreshFromDriver()
    }

    public func checkForUpdates() {
        guard serviceStatus == .ready else { return }
        if !hasStartedUpdater {
            startIfPermitted()
        }
        guard let driver, driver.canCheckForUpdates else { return }
        driver.checkForUpdates()
        refreshFromDriver()
    }
}

@MainActor
@Observable
public final class WorkbenchNavigationModel {
    public var destination: WorkbenchDestination

    public init(destination: WorkbenchDestination = .keyboard) {
        self.destination = destination
    }

    public func openGeneral() {
        if destination != .general {
            destination = .general
        }
    }
}

public struct AboutAndUpdatesSettingsSection: View {
    let updater: SoftwareUpdateController
    let engineVersion: String
    let openURLHandler: (URL) -> Void

    @MainActor
    public init(
        updater: SoftwareUpdateController,
        engineVersion: String,
        openURLHandler: @escaping (URL) -> Void = { NSWorkspace.shared.open($0) }
    ) {
        self.updater = updater
        self.engineVersion = engineVersion
        self.openURLHandler = openURLHandler
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 28) {
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader("版本与软件更新")
                SettingsCard {
                    SettingsRow(
                        "当前应用版本",
                        subtitle: "标识 \(updater.identity.bundleIdentifier) · Rust 内核 v\(engineVersion)",
                        showsDivider: true
                    ) {
                        HStack(spacing: 10) {
                            Text(updater.identity.displayVersionLabel)
                                .font(UITheme.monoBadge)
                                .foregroundStyle(UITheme.textSecondary)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(Capsule().fill(UITheme.subtleSurface))
                                .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))

                            Button("检查更新…") {
                                updater.checkForUpdates()
                            }
                            .surfaceButton(.primary)
                            .disabled(!updater.canCheckForUpdates)
                            .accessibilityIdentifier("settings-check-for-updates")
                        }
                    }

                    SettingsRow("自动检查更新", subtitle: updater.lastCheckSummary) {
                        Toggle(
                            "",
                            isOn: Binding(
                                get: { updater.automaticallyChecksForUpdates },
                                set: { updater.setAutomaticallyChecks($0) }
                            )
                        )
                        .labelsHidden()
                        .toggleStyle(NeutralSwitchToggleStyle())
                        .disabled(updater.serviceStatus != .ready)
                        .accessibilityIdentifier("settings-auto-check-updates")
                    }
                }
            }

            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader("关于与开源")
                SettingsCard {
                    SettingsRow(
                        "开源许可证",
                        subtitle: "基于 \(updater.identity.licenseName) License 开放源代码分发",
                        showsDivider: true
                    ) {
                        Text("\(updater.identity.licenseName) License")
                            .font(UITheme.monoBadge)
                            .foregroundStyle(UITheme.textSecondary)
                    }

                    SettingsRow(
                        "源码仓库与更新日志",
                        subtitle: "查看公开代码仓库、构建说明与历史发行记录"
                    ) {
                        HStack(spacing: 8) {
                            Button("GitHub 仓库") {
                                openURLHandler(updater.identity.repositoryURL)
                            }
                            .surfaceButton(.secondary)
                            .accessibilityIdentifier("settings-open-repository")

                            Button("发行说明") {
                                openURLHandler(updater.identity.releasesURL)
                            }
                            .surfaceButton(.secondary)
                            .accessibilityIdentifier("settings-open-releases")
                        }
                    }
                }
            }
        }
    }
}
