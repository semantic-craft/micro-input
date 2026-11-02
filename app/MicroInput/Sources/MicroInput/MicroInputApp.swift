// R-P03/R-H11: offline first-round workbench; no discovery, permission request or device open on launch.
import SwiftUI
import MicroInputCore
@main struct MicroInputApp: App {
    @NSApplicationDelegateAdaptor(AppLifetime.self) private var lifetime
    @StateObject private var store: Store
    private let statusMenu: StatusMenu
    init() {
        #if MI_SYNTHETIC_BACKEND
        let core = ["reconnect", "reconnect-transient"].contains(ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] ?? "") ? MicroInput.syntheticDiscovery() : MicroInput()
        #else
        let core = MicroInput()
        #endif
        let store = Store(backend: core)
        _store = StateObject(wrappedValue: store)
        statusMenu = StatusMenu(store: store)
    }
    @State private var appearance = "system"
    var body: some Scene {
        Window("Micro Input", id: "main") { WorkbenchHost(store: store, statusMenu: statusMenu).onAppear { lifetime.store = store }.preferredColorScheme(appearance == "system" ? nil : appearance == "dark" ? .dark : .light) }.defaultSize(width: 1180, height: 730)
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(after: .appInfo) {
                Button("显示菜单栏菜单") { statusMenu.show() }.keyboardShortcut("m", modifiers: [.command, .shift])
            }
            CommandGroup(after: .toolbar) {
                Picker("外观", selection: $appearance) {
                    Text("跟随系统").tag("system")
                    Text("浅色").tag("light")
                    Text("深色").tag("dark")
                }
                Button("紧凑窗口") { NSApp.keyWindow?.setContentSize(NSSize(width: 1020, height: 620)) }
                Button("标准窗口") { NSApp.keyWindow?.setContentSize(NSSize(width: 1180, height: 730)) }
                Divider()
                Text("⌘[ / ⌘] 选择前后控件")
                Text("⌘E 搜索动作 · ⌘D 差异 · ⌘Z 撤销")
            }
        }

    }
}

// R-H11/R-P03: native status menu with a keyboard route, including when the editor is closed.
struct WorkbenchHost: View {
    @ObservedObject var store: Store
    let statusMenu: StatusMenu
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        Workbench(store: store).onAppear {
            statusMenu.open = { openWindow(id: "main"); NSApp.activate(ignoringOtherApps: true) }
        }
    }
}
@MainActor final class StatusMenu: NSObject {
    let store: Store
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    var open: (() -> Void)?
    private var panel: NSPanel?
    init(store: Store) {
        self.store = store
        super.init()
        item.button?.image = StatusItemIcon.make()
        item.button?.toolTip = "Micro Input"
        item.button?.target = self; item.button?.action = #selector(show)
    }
    @objc func show() {
        if panel == nil {
            let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 280, height: 240), styleMask: [.titled, .utilityWindow], backing: .buffered, defer: false)
            panel.title = "Micro Input 菜单栏"
            panel.isReleasedWhenClosed = false
            panel.contentView = NSHostingView(rootView: StatusPanel(store: store, owner: self))
            panel.center(); self.panel = panel
        }
        panel?.makeKeyAndOrderFront(nil); NSApp.activate(ignoringOtherApps: true)
    }
    @objc func showWindow() { open?() }
    @objc func quitApp() { NSApp.terminate(nil) }
}

@MainActor final class AppLifetime: NSObject, NSApplicationDelegate {
    weak var store: Store? {
        didSet {
            sleepObserver?.stop()
            sleepObserver = store.map { WorkspaceSleepObserver(store: $0) }
        }
    }
    private var sleepObserver: WorkspaceSleepObserver?
    private var terminating = false
    private let reply: (Bool) -> Void
    override init() {
        reply = { NSApplication.shared.reply(toApplicationShouldTerminate: $0) }
        super.init()
    }
    init(store: Store, reply: @escaping (Bool) -> Void) {
        self.store = store; self.reply = reply
        super.init()
        sleepObserver = WorkspaceSleepObserver(store: store)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let store else { return .terminateNow }
        if !terminating {
            terminating = true
            sleepObserver?.stop(); sleepObserver = nil
            Task { await store.shutdown(); reply(true) }
        }
        return .terminateLater
    }
}

struct StatusPanel: View {
    @ObservedObject var store: Store
    let owner: StatusMenu
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(store.browsingDeviceSnapshot ? (store.stale ? "设备快照已过期 · 浏览不激活配置" : "设备快照只读 · 浏览不激活配置") : "本地草稿 · 不激活设备配置").font(.caption).foregroundStyle(.secondary)
            ForEach(Array((store.snapshot?.profiles ?? []).enumerated()), id: \.offset) { i,p in
                Button(p.name) { store.selectProfile(i); owner.showWindow() }
            }
            Divider()
            Button("打开工作台") { owner.showWindow() }
            Button("退出 Micro Input") { owner.quitApp() }
        }.padding(20).frame(width: 280, height: 240, alignment: .topLeading)
    }
}
