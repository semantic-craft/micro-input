import AppKit
import MicroInputCoreFFI
import MicroInputUI
import ServiceManagement
import Sparkle
import SwiftUI

@MainActor
private final class SMAppServiceLaunchAtLoginDriver: LaunchAtLoginDriver {
    func currentStatus() -> LaunchAtLoginDriverStatus {
        switch SMAppService.mainApp.status {
        case .enabled:
            return .enabled
        case .notRegistered:
            return .notRegistered
        case .requiresApproval:
            return .requiresApproval
        case .notFound:
            return .notFound
        @unknown default:
            return .notRegistered
        }
    }

    func register() throws {
        try SMAppService.mainApp.register()
    }

    func unregister() throws {
        try SMAppService.mainApp.unregister()
    }

    func openSystemSettingsLoginItems() {
        SMAppService.openSystemSettingsLoginItems()
    }
}

@MainActor
private final class SparkleSoftwareUpdateDriver: SoftwareUpdateDriver {
    private let controller: SPUStandardUpdaterController
    private var observations: [NSKeyValueObservation] = []
    private let onStateChange: @MainActor @Sendable () -> Void

    init(onStateChange: @escaping @MainActor @Sendable () -> Void) {
        self.controller = SPUStandardUpdaterController(
            startingUpdater: false,
            updaterDelegate: nil,
            userDriverDelegate: nil
        )
        self.onStateChange = onStateChange
    }

    var canCheckForUpdates: Bool {
        controller.updater.canCheckForUpdates
    }

    var automaticallyChecksForUpdates: Bool {
        get { controller.updater.automaticallyChecksForUpdates }
        set { controller.updater.automaticallyChecksForUpdates = newValue }
    }

    var lastUpdateCheckDate: Date? {
        controller.updater.lastUpdateCheckDate
    }

    func startUpdater() {
        controller.startUpdater()
        let callback = onStateChange
        let notify: @Sendable () -> Void = {
            Task { @MainActor in
                callback()
            }
        }
        observations = [
            controller.updater.observe(\.canCheckForUpdates, options: [.initial, .new]) { _, _ in notify() },
            controller.updater.observe(\.automaticallyChecksForUpdates, options: [.new]) { _, _ in notify() },
            controller.updater.observe(\.lastUpdateCheckDate, options: [.new]) { _, _ in notify() },
        ]
    }

    func checkForUpdates() {
        controller.checkForUpdates(nil)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, NSMenuItemValidation {
    private var instanceLock: OpaquePointer?
    private var statusItem: NSStatusItem?
    private let popover = NSPopover()
    private var mainWindow: NSWindow?
    private var notificationObservers: [NSObjectProtocol] = []
    private var lastRenderedLocale: ResolvedLocale = .zhHans
    private var lastRenderedHostCaution: String? = nil
    private var lastRenderedCanRetryHostCleanup: Bool = false
    private var lastRenderedHostExecutionEnabled: Bool = false
    let store: CoreStore
    let navigation = WorkbenchNavigationModel()
    let launchAtLogin: LaunchAtLoginController
    private(set) lazy var updater: SoftwareUpdateController = SoftwareUpdateController { [weak self] in
        SparkleSoftwareUpdateDriver { [weak self] in
            self?.updater.refreshFromDriver()
        }
    }

    override init() {
        let qaCode = mi_core_run_qa_from_env()
        if qaCode >= 0 {
            Darwin.exit(qaCode)
        }

        guard let lock = mi_core_acquire_instance_lock(nil) else {
            Darwin.exit(0)
        }
        self.instanceLock = lock
        let isIsolated = LaunchOriginDetector.isIsolatedOrVerifyEnvironment()
        if Bundle.main.bundleIdentifier?.hasPrefix("com.semanticcraft.microinput.qa") == true {
            guard let directory = ProcessInfo.processInfo.environment["MI_DOCUMENT_DIR"], !directory.isEmpty else {
                Darwin.exit(1)
            }
            self.store = CoreStore(isolatedDirectory: directory, pollInIsolation: true)
        } else {
            self.store = CoreStore()
        }
        self.launchAtLogin = LaunchAtLoginController(
            driver: isIsolated ? IsolatedLaunchAtLoginDriver() : SMAppServiceLaunchAtLoginDriver()
        )
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let isVerifyMode = LaunchOriginDetector.isIsolatedOrVerifyEnvironment()
        if !isVerifyMode {
            store.syncSystemLocale(Locale.preferredLanguages)
        }
        launchAtLogin.reconcileWithSystem()

        let silentBoot = LaunchOriginDetector.shouldStartSilently(
            isVerifyMode: isVerifyMode,
            launchedAtLogin: LaunchOriginDetector.wasLaunchedAtLogin(),
            silentAtLoginPref: store.header.silentAtLogin
        )
        NSApp.setActivationPolicy(silentBoot ? .accessory : .regular)
        NSApp.applicationIconImage = NSWorkspace.shared.icon(forFile: Bundle.main.bundlePath)
        lastRenderedLocale = store.locale
        lastRenderedHostCaution = store.statusMenu.hostCautionNotice
        lastRenderedCanRetryHostCleanup = store.statusMenu.canRetryHostCleanup
        lastRenderedHostExecutionEnabled = store.statusMenu.hostExecutionEnabled
        setupMainMenu()
        setupStatusItem()
        updater.startIfPermitted()
        installHostObservers(isVerifyMode: isVerifyMode)
        observeStoreLocale()
        observeStoreStatusItemState()
        if !silentBoot {
            openMainWindow()
        }
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard store.firmware.preventsTermination else { return .terminateNow }
        let alert = NSAlert()
        alert.messageText = store.tr("设备固件更新尚未结束")
        alert.informativeText = store.tr("正在准备、备份或写入设备。请保持 USB 连接，等待当前操作完成后再退出。")
        alert.addButton(withTitle: store.tr("继续等待"))
        alert.runModal()
        return .terminateCancel
    }

    func applicationWillTerminate(_ notification: Notification) {
        for observer in notificationObservers {
            NotificationCenter.default.removeObserver(observer)
        }
        notificationObservers.removeAll()
        store.prepareForTermination()
        store.stopEventLoops()
        if let lock = instanceLock {
            mi_core_release_instance_lock(lock)
            instanceLock = nil
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag || mainWindow?.isMiniaturized == true || mainWindow?.isVisible != true {
            openMainWindow()
        } else {
            mainWindow?.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
        }
        return true
    }

    func validateMenuItem(_ menuItem: NSMenuItem) -> Bool {
        if menuItem.action == #selector(handleCheckForUpdates(_:)) {
            return updater.canCheckForUpdates
        }
        return true
    }

    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        let menu = NSMenu(title: "Micro Input")
        let openItem = NSMenuItem(
            title: store.tr("打开主配置台"),
            action: #selector(handleDockOpenMainWindow(_:)),
            keyEquivalent: ""
        )
        openItem.target = self
        menu.addItem(openItem)

        let settingsItem = NSMenuItem(
            title: store.tr("通用设置…"),
            action: #selector(handleOpenGeneralSettings(_:)),
            keyEquivalent: ""
        )
        settingsItem.target = self
        menu.addItem(settingsItem)

        let updateItem = NSMenuItem(
            title: store.tr("检查更新…"),
            action: #selector(handleCheckForUpdates(_:)),
            keyEquivalent: ""
        )
        updateItem.target = self
        menu.addItem(updateItem)

        let toggleExecItem = NSMenuItem(
            title: store.tr(store.statusMenu.hostExecutionEnabled ? "暂停按键映射" : "启用按键映射"),
            action: #selector(handleDockToggleExecution(_:)),
            keyEquivalent: ""
        )
        toggleExecItem.target = self
        menu.addItem(toggleExecItem)

        menu.addItem(.separator())

        for fav in store.statusMenu.favorites2x2 where fav.profileId != nil {
            let display = DesktopAgent.displayName(fav.title, locale: store.locale)
            let item = NSMenuItem(
                title: store.locale == .en ? "Switch to \(display)" : "切换到 \(display)",
                action: #selector(handleDockSelectFavorite(_:)),
                keyEquivalent: ""
            )
            item.target = self
            item.tag = fav.slotIndex
            item.state = fav.isActive ? .on : .off
            menu.addItem(item)
        }
        return menu
    }

    private func installHostObservers(isVerifyMode: Bool) {
        let activeObs = NotificationCenter.default.addObserver(
            forName: NSApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.launchAtLogin.reconcileWithSystem()
            }
        }
        let localeObs = NotificationCenter.default.addObserver(
            forName: NSLocale.currentLocaleDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !isVerifyMode else { return }
                self.store.syncSystemLocale(Locale.preferredLanguages)
            }
        }
        notificationObservers = [activeObs, localeObs]
    }

    private func observeStoreLocale() {
        withObservationTracking {
            _ = store.locale
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                if self.lastRenderedLocale != self.store.locale {
                    self.lastRenderedLocale = self.store.locale
                    self.refreshLocalizedAppKitChrome()
                }
                self.observeStoreLocale()
            }
        }
    }

    private func observeStoreStatusItemState() {
        withObservationTracking {
            _ = store.statusMenu.hostCautionNotice
            _ = store.statusMenu.canRetryHostCleanup
            _ = store.statusMenu.hostExecutionEnabled
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                let nextNotice = self.store.statusMenu.hostCautionNotice
                let nextCanRetry = self.store.statusMenu.canRetryHostCleanup
                let nextHostExecutionEnabled = self.store.statusMenu.hostExecutionEnabled
                if self.lastRenderedHostCaution != nextNotice
                    || self.lastRenderedCanRetryHostCleanup != nextCanRetry
                    || self.lastRenderedHostExecutionEnabled != nextHostExecutionEnabled
                {
                    self.lastRenderedHostCaution = nextNotice
                    self.lastRenderedCanRetryHostCleanup = nextCanRetry
                    self.lastRenderedHostExecutionEnabled = nextHostExecutionEnabled
                    self.refreshStatusItemAppearance()
                    if self.popover.isShown {
                        self.syncPopoverContentSize()
                    }
                }
                self.observeStoreStatusItemState()
            }
        }
    }

    private func refreshStatusItemAppearance() {
        guard let button = statusItem?.button else { return }
        let hostExecutionEnabled = store.statusMenu.hostExecutionEnabled
        let notice = store.statusMenu.hostCautionNotice
        let presentation = StatusItemIcon.presentation(
            hostExecutionEnabled: hostExecutionEnabled,
            hasCaution: notice != nil
        )
        button.image = StatusItemIcon.make(locale: store.locale, presentation: presentation)
        button.toolTip = StatusItemIcon.toolTip(
            locale: store.locale,
            hostExecutionEnabled: hostExecutionEnabled,
            hostCautionNotice: notice
        )
    }

    private func refreshLocalizedAppKitChrome() {
        setupMainMenu()
        refreshStatusItemAppearance()
        if popover.isShown {
            syncPopoverContentSize()
        }
    }

    private func setupMainMenu() {
        lastRenderedLocale = store.locale
        let mainMenu = NSMenu()

        let appMenuItem = NSMenuItem()
        let appMenu = NSMenu(title: "Micro Input")

        let aboutItem = NSMenuItem(
            title: store.tr("关于 Micro Input"),
            action: #selector(handleOpenAboutSettings(_:)),
            keyEquivalent: ""
        )
        aboutItem.target = self
        appMenu.addItem(aboutItem)
        appMenu.addItem(.separator())

        let settingsItem = NSMenuItem(
            title: store.tr("设置…"),
            action: #selector(handleOpenGeneralSettings(_:)),
            keyEquivalent: ","
        )
        settingsItem.keyEquivalentModifierMask = [.command]
        settingsItem.target = self
        appMenu.addItem(settingsItem)

        let checkUpdatesItem = NSMenuItem(
            title: store.tr("检查更新…"),
            action: #selector(handleCheckForUpdates(_:)),
            keyEquivalent: ""
        )
        checkUpdatesItem.target = self
        appMenu.addItem(checkUpdatesItem)
        appMenu.addItem(.separator())

        let hideItem = NSMenuItem(
            title: store.tr("隐藏 Micro Input"),
            action: #selector(NSApplication.hide(_:)),
            keyEquivalent: "h"
        )
        appMenu.addItem(hideItem)

        let hideOthersItem = NSMenuItem(
            title: store.tr("隐藏其他"),
            action: #selector(NSApplication.hideOtherApplications(_:)),
            keyEquivalent: "h"
        )
        hideOthersItem.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(hideOthersItem)

        appMenu.addItem(
            NSMenuItem(
                title: store.tr("全部显示"),
                action: #selector(NSApplication.unhideAllApplications(_:)),
                keyEquivalent: ""
            )
        )
        appMenu.addItem(.separator())
        appMenu.addItem(
            NSMenuItem(
                title: store.tr("退出 Micro Input"),
                action: #selector(NSApplication.terminate(_:)),
                keyEquivalent: "q"
            )
        )
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        let editMenuItem = NSMenuItem(title: store.tr("编辑"), action: nil, keyEquivalent: "")
        let editMenu = NSMenu(title: store.tr("编辑"))
        editMenu.addItem(NSMenuItem(title: store.tr("撤销"), action: Selector(("undo:")), keyEquivalent: "z"))
        let redoItem = NSMenuItem(title: store.tr("重做"), action: Selector(("redo:")), keyEquivalent: "Z")
        redoItem.keyEquivalentModifierMask = [.command, .shift]
        editMenu.addItem(redoItem)
        editMenu.addItem(.separator())
        editMenu.addItem(NSMenuItem(title: store.tr("剪切"), action: #selector(NSText.cut(_:)), keyEquivalent: "x"))
        editMenu.addItem(NSMenuItem(title: store.tr("拷贝"), action: #selector(NSText.copy(_:)), keyEquivalent: "c"))
        editMenu.addItem(NSMenuItem(title: store.tr("粘贴"), action: #selector(NSText.paste(_:)), keyEquivalent: "v"))
        editMenu.addItem(NSMenuItem(title: store.tr("全选"), action: #selector(NSText.selectAll(_:)), keyEquivalent: "a"))
        editMenuItem.submenu = editMenu
        mainMenu.addItem(editMenuItem)

        let windowMenuItem = NSMenuItem(title: store.tr("窗口"), action: nil, keyEquivalent: "")
        let windowMenu = NSMenu(title: "Window")
        windowMenu.addItem(NSMenuItem(title: store.tr("最小化"), action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m"))
        windowMenu.addItem(NSMenuItem(title: store.tr("缩放"), action: #selector(NSWindow.performZoom(_:)), keyEquivalent: ""))
        windowMenu.addItem(NSMenuItem(title: store.tr("关闭窗口"), action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w"))
        windowMenu.addItem(.separator())
        windowMenu.addItem(NSMenuItem(title: store.tr("前置全部窗口"), action: #selector(NSApplication.arrangeInFront(_:)), keyEquivalent: ""))
        windowMenuItem.submenu = windowMenu
        mainMenu.addItem(windowMenuItem)

        NSApp.mainMenu = mainMenu
        NSApp.windowsMenu = windowMenu
    }

    @objc private func handleOpenAboutSettings(_ sender: Any?) {
        openGeneralSettings()
    }

    @objc private func handleOpenGeneralSettings(_ sender: Any?) {
        openGeneralSettings()
    }

    @objc private func handleCheckForUpdates(_ sender: Any?) {
        updater.checkForUpdates()
    }

    @objc private func handleDockOpenMainWindow(_ sender: Any?) {
        store.dispatch("status_menu", "OpenMainWorkbenchWindow")
        openMainWindow()
    }

    @objc private func handleDockToggleExecution(_ sender: Any?) {
        let next = !store.statusMenu.hostExecutionEnabled
        store.dispatch("status_menu", ["SetHostExecutionEnabled": next])
    }

    @objc private func handleDockSelectFavorite(_ sender: NSMenuItem) {
        store.dispatch("status_menu", ["ActivateFavoriteSlot": sender.tag])
    }

    private func openGeneralSettings() {
        navigation.openGeneral()
        store.dispatch("status_menu", "OpenMainWorkbenchWindow")
        openMainWindow()
    }

    private func setupStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = item.button {
            button.imagePosition = .imageOnly
            button.target = self
            button.action = #selector(toggleStatusPopover(_:))
        }
        self.statusItem = item
        refreshStatusItemAppearance()

        let hosting = NSHostingController(
            rootView: StatusMenuPopoverView(
                store: store,
                updater: updater,
                onOpenMainWindow: { [weak self] in
                    self?.popover.performClose(nil)
                    self?.openMainWindow()
                },
                onOpenSettings: { [weak self] in
                    self?.popover.performClose(nil)
                    self?.openGeneralSettings()
                },
                onCheckForUpdates: { [weak self] in
                    self?.popover.performClose(nil)
                    self?.updater.checkForUpdates()
                }
            )
        )
        hosting.sizingOptions = [.preferredContentSize]
        popover.behavior = .transient
        popover.contentViewController = hosting
        syncPopoverContentSize()
    }

    private func syncPopoverContentSize() {
        guard let hosting = popover.contentViewController as? NSHostingController<StatusMenuPopoverView> else {
            return
        }
        hosting.view.layoutSubtreeIfNeeded()
        popover.contentSize = hosting.sizeThatFits(
            in: NSSize(width: CGFloat(348), height: .greatestFiniteMagnitude)
        )
    }

    @objc private func toggleStatusPopover(_ sender: AnyObject?) {
        guard let button = statusItem?.button else { return }
        if popover.isShown {
            popover.performClose(sender)
        } else {
            syncPopoverContentSize()
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }
    }

    func openMainWindow() {
        if NSApp.activationPolicy() != .regular {
            NSApp.setActivationPolicy(.regular)
            NSApp.applicationIconImage = NSWorkspace.shared.icon(forFile: Bundle.main.bundlePath)
            refreshLocalizedAppKitChrome()
        }
        if let win = mainWindow {
            if win.isMiniaturized {
                win.deminiaturize(nil)
            }
            win.makeKeyAndOrderFront(nil)
            win.makeFirstResponder(nil)
            NSApp.activate(ignoringOtherApps: true)
            store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": true]])
            return
        }

        let win = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1180, height: 740),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        win.minSize = NSSize(width: 820, height: 640)
        win.title = "Micro Input"
        win.center()
        win.isReleasedWhenClosed = false
        win.delegate = self
        win.contentView = NSHostingView(
            rootView: WorkbenchRootView(
                store: store,
                navigation: navigation,
                updater: updater,
                launchAtLogin: launchAtLogin
            )
        )
        win.makeKeyAndOrderFront(nil)
        win.makeFirstResponder(nil)
        NSApp.activate(ignoringOtherApps: true)
        self.mainWindow = win
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": true]])
    }

    func windowWillClose(_ notification: Notification) {
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": false]])
    }

    func windowDidBecomeKey(_ notification: Notification) {
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": true]])
    }

    func windowDidResignKey(_ notification: Notification) {
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": false]])
    }

    func windowDidMiniaturize(_ notification: Notification) {
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": false]])
    }

    func windowDidDeminiaturize(_ notification: Notification) {
        store.dispatchRaw(["domain": "main_window_active_changed", "action": ["active": true]])
    }
}

@main
struct MicroInputMain {
    static func main() {
        let app = NSApplication.shared
        let delegate = MainActor.assumeIsolated { AppDelegate() }
        app.delegate = delegate
        app.run()
    }
}
