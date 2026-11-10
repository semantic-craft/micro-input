import AppKit
import MicroInputCoreFFI
import MicroInputUI
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var instanceLock: OpaquePointer?
    private var statusItem: NSStatusItem?
    private let popover = NSPopover()
    private var mainWindow: NSWindow?
    let store: CoreStore

    override init() {
        let qaCode = mi_core_run_qa_from_env()
        if qaCode >= 0 {
            Darwin.exit(qaCode)
        }

        guard let lock = mi_core_acquire_instance_lock(nil) else {
            Darwin.exit(0)
        }
        self.instanceLock = lock
        self.store = CoreStore()
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        if let iconURL = Bundle.main.url(forResource: "AppIcon", withExtension: "icns"),
           let iconImage = NSImage(contentsOf: iconURL)
        {
            NSApp.applicationIconImage = iconImage
        }
        setupStatusItem()
        openMainWindow()
    }

    func applicationWillTerminate(_ notification: Notification) {
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

    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        let menu = NSMenu(title: "Micro Input")
        let openItem = NSMenuItem(
            title: "打开主配置台",
            action: #selector(handleDockOpenMainWindow(_:)),
            keyEquivalent: ""
        )
        openItem.target = self
        menu.addItem(openItem)

        let toggleExecItem = NSMenuItem(
            title: store.statusMenu.hostExecutionEnabled ? "暂停按键控制" : "启用按键控制",
            action: #selector(handleDockToggleExecution(_:)),
            keyEquivalent: ""
        )
        toggleExecItem.target = self
        menu.addItem(toggleExecItem)

        menu.addItem(.separator())

        for fav in store.statusMenu.favorites2x2 {
            let item = NSMenuItem(
                title: "切换到 \(DesktopAgent.displayName(fav.title))",
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

    private func setupStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        if let button = item.button {
            button.image = StatusItemIcon.make()
            button.imagePosition = .imageOnly
            button.target = self
            button.action = #selector(toggleStatusPopover(_:))
        }
        self.statusItem = item

        popover.behavior = .transient
        popover.contentSize = NSSize(width: 340, height: 480)
        popover.contentViewController = NSHostingController(
            rootView: StatusMenuPopoverView(store: store) { [weak self] in
                self?.popover.performClose(nil)
                self?.openMainWindow()
            }
        )
    }

    @objc private func toggleStatusPopover(_ sender: AnyObject?) {
        guard let button = statusItem?.button else { return }
        if popover.isShown {
            popover.performClose(sender)
        } else {
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }
    }

    func openMainWindow() {
        if let win = mainWindow {
            if win.isMiniaturized {
                win.deminiaturize(nil)
            }
            win.makeKeyAndOrderFront(nil)
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
        win.minSize = NSSize(width: 1020, height: 680)
        win.title = "Micro Input"
        win.center()
        win.isReleasedWhenClosed = false
        win.delegate = self
        win.contentView = NSHostingView(rootView: WorkbenchRootView(store: store))
        win.makeKeyAndOrderFront(nil)
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
