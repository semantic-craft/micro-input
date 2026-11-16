import AppKit
import CoreServices
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
            ?? LaunchAtLoginPolicy.productionBundleIdentifier
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
    case isolated(reason: HostGateReason)
    case unavailable(reason: HostGateReason)
    case ready

    public func statusDescription(_ locale: ResolvedLocale) -> String {
        switch self {
        case .isolated(let reason), .unavailable(let reason):
            return reason.localizedDescription(locale)
        case .ready:
            return locale == .en
                ? "Receiving signed updates via GitHub Releases"
                : "通过 GitHub Releases 签名更新源接收更新"
        }
    }

    public var statusDescription: String {
        statusDescription(.zhHans)
    }
}

private enum BundlePathNormalization {
    static let ephemeralMarkers = [
        "/target/",
        "/DerivedData/",
        "/.build/",
        "/tmp/",
        "/private/tmp/",
        "/var/folders/",
        "/.scratch/",
    ]

    static func normalize(_ bundlePath: String) -> (trimmed: String, resolved: String) {
        var trimmed = bundlePath
        while trimmed.count > 1 && trimmed.hasSuffix("/") {
            trimmed.removeLast()
        }
        return (trimmed, resolvedBundlePath(for: trimmed))
    }

    private static func resolvedBundlePath(for trimmedPath: String) -> String {
        let resolved = URL(fileURLWithPath: trimmedPath).resolvingSymlinksInPath().path
        if resolved != trimmedPath {
            return resolved
        }
        if let dest = try? FileManager.default.destinationOfSymbolicLink(atPath: trimmedPath) {
            let expanded: String
            if dest.hasPrefix("/") {
                expanded = dest
            } else {
                let parent = URL(fileURLWithPath: trimmedPath).deletingLastPathComponent()
                expanded = parent.appendingPathComponent(dest).standardized.path
            }
            var trimmedDest = expanded
            while trimmedDest.count > 1 && trimmedDest.hasSuffix("/") {
                trimmedDest.removeLast()
            }
            return URL(fileURLWithPath: trimmedDest).resolvingSymlinksInPath().path
        }
        return resolved
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
            return .isolated(reason: .disabledByEnvironment(variable: "MI_DISABLE_UPDATER"))
        }
        if LaunchOriginDetector.isIsolatedOrVerifyEnvironment(
            bundleIdentifier: identity.bundleIdentifier,
            environment: environment,
            arguments: arguments
        ) {
            return .isolated(reason: .qaVerifyMode)
        }
        let paths = BundlePathNormalization.normalize(bundlePath)
        if !paths.trimmed.hasSuffix(".app") || !paths.resolved.hasSuffix(".app") {
            return .unavailable(reason: .unbundledExecutable)
        }
        if paths.trimmed.contains("/AppTranslocation/") || paths.trimmed.hasPrefix("/Volumes/")
            || paths.resolved.contains("/AppTranslocation/") || paths.resolved.hasPrefix("/Volumes/")
        {
            return .unavailable(reason: .translocatedOrMountedVolume(bundlePath: bundlePath))
        }
        if BundlePathNormalization.ephemeralMarkers.contains(where: { paths.trimmed.contains($0) || paths.resolved.contains($0) }) {
            return .isolated(reason: .ephemeralBuildBundle(bundlePath: paths.resolved))
        }
        guard identity.bundleIdentifier == LaunchAtLoginPolicy.productionBundleIdentifier else {
            return .unavailable(reason: .wrongBundleIdentifier(actual: identity.bundleIdentifier))
        }
        guard let feedRaw = identity.feedURLString,
              let feedURL = URL(string: feedRaw),
              feedURL.scheme?.lowercased() == "https",
              let host = feedURL.host,
              !host.isEmpty
        else {
            return .unavailable(reason: .missingInfoPlistKey(key: "SUFeedURL"))
        }
        guard let keyRaw = identity.publicEDKey,
              let keyData = Data(base64Encoded: keyRaw),
              keyData.count == 32
        else {
            return .unavailable(reason: .missingInfoPlistKey(key: "SUPublicEDKey"))
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

    public func lastCheckSummary(_ locale: ResolvedLocale) -> String {
        guard serviceStatus == .ready else {
            return serviceStatus.statusDescription(locale)
        }
        if let lastUpdateCheckDate {
            let formatted = Self.timestampFormatter.string(from: lastUpdateCheckDate)
            return locale == .en ? "Last checked: \(formatted)" : "上次检查：\(formatted)"
        }
        return serviceStatus.statusDescription(locale)
    }

    public var lastCheckSummary: String {
        lastCheckSummary(.zhHans)
    }

    public func startIfPermitted() {
        guard serviceStatus == .ready else { return }
        guard !hasStartedUpdater else {
            refreshFromDriver()
            return
        }
        guard let driverFactory else {
            serviceStatus = .unavailable(reason: .missingDriver)
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

public enum LaunchAtLoginPolicy {
    public static let productionBundleIdentifier = "craft.semantic.micro-input.dev"

    public static func evaluate(
        bundleIdentifier: String?,
        bundlePath: String,
        environment: [String: String],
        arguments: [String] = []
    ) -> HostGateReason? {
        if LaunchOriginDetector.isIsolatedOrVerifyEnvironment(
            bundleIdentifier: bundleIdentifier,
            environment: environment,
            arguments: arguments
        ) {
            return .qaVerifyMode
        }
        if environment["MI_DISABLE_LOGIN_ITEM"] == "1" {
            return .disabledByEnvironment(variable: "MI_DISABLE_LOGIN_ITEM")
        }
        let paths = BundlePathNormalization.normalize(bundlePath)
        if !paths.trimmed.hasSuffix(".app") || !paths.resolved.hasSuffix(".app") {
            return .unbundledExecutable
        }
        if paths.trimmed.contains("/AppTranslocation/") || paths.trimmed.hasPrefix("/Volumes/")
            || paths.resolved.contains("/AppTranslocation/") || paths.resolved.hasPrefix("/Volumes/")
        {
            return .translocatedOrMountedVolume(bundlePath: bundlePath)
        }
        if BundlePathNormalization.ephemeralMarkers.contains(where: { paths.trimmed.contains($0) || paths.resolved.contains($0) }) {
            return .ephemeralBuildBundle(bundlePath: bundlePath)
        }
        guard bundleIdentifier == productionBundleIdentifier else {
            return .wrongBundleIdentifier(actual: bundleIdentifier ?? "")
        }
        return nil
    }
}

public enum LaunchAtLoginDriverStatus: Equatable, Sendable {
    case enabled
    case notRegistered
    case requiresApproval
    case notFound
}

public enum LaunchAtLoginState: Equatable, Sendable {
    case enabled
    case disabled
    case requiresApproval
    case unavailable(reason: HostGateReason)
}

@MainActor
public protocol LaunchAtLoginDriver: AnyObject {
    func currentStatus() -> LaunchAtLoginDriverStatus
    func register() throws
    func unregister() throws
    func openSystemSettingsLoginItems()
}

@MainActor
public final class IsolatedLaunchAtLoginDriver: LaunchAtLoginDriver {
    public init() {}
    public func currentStatus() -> LaunchAtLoginDriverStatus { .notRegistered }
    public func register() throws {}
    public func unregister() throws {}
    public func openSystemSettingsLoginItems() {}
}

@MainActor
@Observable
public final class LaunchAtLoginController {
    public private(set) var state: LaunchAtLoginState
    public private(set) var lastError: String? = nil

    @ObservationIgnored private let policyReason: HostGateReason?
    @ObservationIgnored private let driver: LaunchAtLoginDriver?

    public init(
        bundleIdentifier: String? = Bundle.main.bundleIdentifier,
        bundlePath: String = Bundle.main.bundlePath,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        arguments: [String] = CommandLine.arguments,
        driver: LaunchAtLoginDriver? = nil
    ) {
        let detectedReason = LaunchAtLoginPolicy.evaluate(
            bundleIdentifier: bundleIdentifier,
            bundlePath: bundlePath,
            environment: environment,
            arguments: arguments
        )
        let effectiveReason = detectedReason ?? (driver == nil ? .missingDriver : nil)
        self.policyReason = effectiveReason
        self.driver = driver
        if let effectiveReason {
            self.state = .unavailable(reason: effectiveReason)
        } else if let driver {
            self.state = Self.mapDriverStatus(driver.currentStatus())
        } else {
            self.state = .unavailable(reason: .missingDriver)
        }
    }

    public var isUnavailable: Bool {
        if case .unavailable = state { return true }
        return false
    }

    public var isToggleOn: Bool {
        switch state {
        case .enabled, .requiresApproval:
            return true
        case .disabled, .unavailable:
            return false
        }
    }

    private static func mapDriverStatus(_ status: LaunchAtLoginDriverStatus) -> LaunchAtLoginState {
        switch status {
        case .enabled:
            return .enabled
        case .notRegistered, .notFound:
            return .disabled
        case .requiresApproval:
            return .requiresApproval
        }
    }

    public func reconcileWithSystem() {
        let next: LaunchAtLoginState
        if let policyReason {
            next = .unavailable(reason: policyReason)
        } else if let driver {
            next = Self.mapDriverStatus(driver.currentStatus())
        } else {
            next = .unavailable(reason: .missingDriver)
        }
        if state != next {
            state = next
        }
    }

    public func setEnabled(_ wantEnabled: Bool) {
        guard policyReason == nil, let driver else { return }
        reconcileWithSystem()
        if case .unavailable = state { return }

        switch (state, wantEnabled) {
        case (.enabled, true), (.disabled, false), (.requiresApproval, true):
            if lastError != nil { lastError = nil }
            return
        case (.disabled, true):
            do {
                try driver.register()
                if lastError != nil { lastError = nil }
            } catch {
                lastError = error.localizedDescription
            }
        case (.enabled, false), (.requiresApproval, false):
            do {
                try driver.unregister()
                if lastError != nil { lastError = nil }
            } catch {
                lastError = error.localizedDescription
            }
        case (.unavailable, _):
            return
        }
        reconcileWithSystem()
    }

    public func openLoginItemsSettings() {
        guard policyReason == nil, let driver else { return }
        driver.openSystemSettingsLoginItems()
    }
}

public enum LaunchOriginDetector {
    public static let openApplicationEventClass: AEEventClass = 0x6165_7674
    public static let openApplicationEventID: AEEventID = 0x6f61_7070
    public static let propDataKeyword: AEKeyword = 0x7072_6474
    public static let launchedAsLogInItemCode: DescType = 0x6c67_6974

    public static func isIsolatedOrVerifyEnvironment(
        bundleIdentifier: String? = Bundle.main.bundleIdentifier,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        arguments: [String] = CommandLine.arguments
    ) -> Bool {
        if arguments.contains("--verify") || arguments.contains("--isolated") {
            return true
        }
        for envKey in ["MI_QA_SCENARIO", "MI_SYNTHETIC_SCENARIO", "MI_DOCUMENT_DIR", "MI_HOST_AUDIT_LOG"] {
            if let value = environment[envKey], !value.isEmpty {
                return true
            }
        }
        if environment["XCTestConfigurationFilePath"] != nil {
            return true
        }
        if bundleIdentifier?.hasPrefix("com.semanticcraft.microinput.qa") == true {
            return true
        }
        return false
    }

    public static func isLaunchedAtLoginAppleEvent(_ event: NSAppleEventDescriptor?) -> Bool {
        guard let event,
              event.eventClass == openApplicationEventClass,
              event.eventID == openApplicationEventID
        else {
            return false
        }
        return event.paramDescriptor(forKeyword: propDataKeyword)?.enumCodeValue == launchedAsLogInItemCode
    }

    public static func wasLaunchedAtLogin(
        arguments: [String] = CommandLine.arguments,
        environment: [String: String] = ProcessInfo.processInfo.environment,
        currentAppleEvent: NSAppleEventDescriptor? = NSAppleEventManager.shared().currentAppleEvent
    ) -> Bool {
        if arguments.contains("--launched-at-login") {
            return true
        }
        if environment["MI_LAUNCHED_AT_LOGIN"] == "1" {
            return true
        }
        return isLaunchedAtLoginAppleEvent(currentAppleEvent)
    }

    public static func shouldStartSilently(
        isVerifyMode: Bool,
        launchedAtLogin: Bool,
        silentAtLoginPref: Bool
    ) -> Bool {
        !isVerifyMode && launchedAtLogin && silentAtLoginPref
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

public struct StartupAndLanguageSettingsSection: View {
    let store: CoreStore
    let launchAtLogin: LaunchAtLoginController

    public init(store: CoreStore, launchAtLogin: LaunchAtLoginController) {
        self.store = store
        self.launchAtLogin = launchAtLogin
    }

    public var body: some View {
        let locale = store.locale
        let h = store.header
        let languageOptions = UiLanguagePreference.allCases.map { pref in
            SelectOption(
                value: pref,
                title: pref.displayTitle(locale),
                subtitle: pref.displaySubtitle(locale)
            )
        }
        let loginSubtitle: String = {
            switch launchAtLogin.state {
            case .unavailable(let reason):
                return reason.localizedDescription(locale)
            case .requiresApproval:
                return store.tr("需要在「系统设置 → 通用 → 登录项」中批准后台运行")
            case .enabled, .disabled:
                return store.tr("开机登录后自动在后台驻留菜单栏并就绪键盘控制")
            }
        }()

        VStack(alignment: .leading, spacing: 10) {
            SettingsSectionHeader(store.tr("启动与语言"))
            SettingsCard {
                SettingsRow(
                    store.tr("界面语言"),
                    subtitle: store.tr("实时切换工作台、菜单栏弹层与按键速查的显示语言"),
                    showsDivider: true
                ) {
                    SelectDropdown(
                        options: languageOptions,
                        selection: Binding(
                            get: { h.uiLanguage },
                            set: { store.setUiLanguage($0) }
                        ),
                        minTriggerWidth: 132
                    )
                    .accessibilityIdentifier("settings-ui-language")
                }

                SettingsRow(
                    store.tr("登录时打开 Micro Input"),
                    subtitle: loginSubtitle,
                    showsDivider: true
                ) {
                    HStack(spacing: 8) {
                        if launchAtLogin.state == .requiresApproval {
                            Button(store.tr("打开登录项设置…")) {
                                launchAtLogin.openLoginItemsSettings()
                            }
                            .surfaceButton(.secondary)
                            .accessibilityIdentifier("settings-open-login-items")
                        }
                        Toggle(
                            "",
                            isOn: Binding(
                                get: { launchAtLogin.isToggleOn },
                                set: { launchAtLogin.setEnabled($0) }
                            )
                        )
                        .labelsHidden()
                        .toggleStyle(NeutralSwitchToggleStyle())
                        .disabled(launchAtLogin.isUnavailable)
                        .accessibilityIdentifier("settings-launch-at-login")
                    }
                }

                if let err = launchAtLogin.lastError {
                    HStack(spacing: 6) {
                        Image(systemName: "exclamationmark.triangle")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(UITheme.textSecondary)
                        Text(store.tr("登录项变更失败：") + err)
                            .font(UITheme.micro)
                            .foregroundStyle(UITheme.textPrimary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    Rectangle().fill(UITheme.rowDivider).frame(height: 1).padding(.leading, 16)
                }

                SettingsRow(
                    store.tr("登录时静默驻留菜单栏"),
                    subtitle: store.tr("由系统登录项拉起时不弹出主窗口，仅驻留右上角菜单栏")
                ) {
                    Toggle(
                        "",
                        isOn: Binding(
                            get: { h.silentAtLogin },
                            set: { store.setSilentAtLogin($0) }
                        )
                    )
                    .labelsHidden()
                    .toggleStyle(NeutralSwitchToggleStyle())
                    .accessibilityIdentifier("settings-silent-at-login")
                }
            }
        }
    }
}

public struct AboutAndUpdatesSettingsSection: View {
    let updater: SoftwareUpdateController
    let engineVersion: String
    let locale: ResolvedLocale
    let openURLHandler: (URL) -> Void

    @MainActor
    public init(
        updater: SoftwareUpdateController,
        engineVersion: String,
        locale: ResolvedLocale = .zhHans,
        openURLHandler: @escaping (URL) -> Void = { NSWorkspace.shared.open($0) }
    ) {
        self.updater = updater
        self.engineVersion = engineVersion
        self.locale = locale
        self.openURLHandler = openURLHandler
    }

    public var body: some View {
        let kernelSubtitle = locale == .en ? "Rust Core v\(engineVersion)" : "Rust 内核 v\(engineVersion)"
        let licenseSubtitle = locale == .en
            ? "Distributed as open source under the \(updater.identity.licenseName) License"
            : "基于 \(updater.identity.licenseName) License 开放源代码分发"

        VStack(alignment: .leading, spacing: 28) {
            VStack(alignment: .leading, spacing: 10) {
                SettingsSectionHeader(L10n.tr("版本与软件更新", locale))
                SettingsCard {
                    SettingsRow(
                        L10n.tr("当前应用版本", locale),
                        subtitle: kernelSubtitle,
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

                            Button(L10n.tr("检查更新…", locale)) {
                                updater.checkForUpdates()
                            }
                            .surfaceButton(.primary)
                            .disabled(!updater.canCheckForUpdates)
                            .accessibilityIdentifier("settings-check-for-updates")
                        }
                    }

                    SettingsRow(L10n.tr("自动检查更新", locale), subtitle: updater.lastCheckSummary(locale)) {
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
                SettingsSectionHeader(L10n.tr("关于与开源", locale))
                SettingsCard {
                    SettingsRow(
                        L10n.tr("开源许可证", locale),
                        subtitle: licenseSubtitle,
                        showsDivider: true
                    ) {
                        Text("\(updater.identity.licenseName) License")
                            .font(UITheme.monoBadge)
                            .foregroundStyle(UITheme.textSecondary)
                    }

                    SettingsRow(
                        L10n.tr("源码仓库与更新日志", locale),
                        subtitle: L10n.tr("查看公开代码仓库、构建说明与历史发行记录", locale)
                    ) {
                        HStack(spacing: 8) {
                            Button(L10n.tr("GitHub 仓库", locale)) {
                                openURLHandler(updater.identity.repositoryURL)
                            }
                            .surfaceButton(.secondary)
                            .accessibilityIdentifier("settings-open-repository")

                            Button(L10n.tr("发行说明", locale)) {
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
