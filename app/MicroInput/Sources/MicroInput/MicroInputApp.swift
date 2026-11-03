// R-P03/R-H11: offline first-round workbench; no discovery, permission request or device open on launch.
import SwiftUI
import MicroInputCore
@main struct MicroInputApp: App {
    @NSApplicationDelegateAdaptor(AppLifetime.self) private var lifetime
    @StateObject private var store: Store
    private let statusMenu: StatusMenu
    init() {
        let scenario = ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] ?? ""
        #if MI_SYNTHETIC_BACKEND
        let core = ["reconnect", "reconnect-transient", "apply-preparation", "host-execution"].contains(scenario) ? MicroInput.syntheticDiscovery() : MicroInput()
        let store = Store(backend: core)
        #else
        let core = MicroInput()
        let store = Store(backend: core)
        #endif
        if scenario.isEmpty, ProcessInfo.processInfo.environment["MI_AUTO_HOST_READY"] != "0" {
            store.autoHostReady = true
        }
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
    private let popover = NSPopover()
    init(store: Store) {
        self.store = store
        super.init()
        item.button?.image = StatusItemIcon.make()
        item.button?.toolTip = "Micro Input"
        item.button?.target = self; item.button?.action = #selector(show)
        popover.behavior = .transient
        popover.contentViewController = NSHostingController(rootView: StatusPanel(store: store, owner: self))
    }
    @objc func show() {
        guard let button = item.button else { return }
        if popover.isShown { popover.performClose(nil); return }
        NSApp.activate(ignoringOtherApps: true)
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        popover.contentViewController?.view.window?.makeKey()
    }
    @objc func showWindow() { popover.performClose(nil); open?() }
    @objc func quitApp() { NSApp.terminate(nil) }
}

@MainActor final class AppLifetime: NSObject, NSApplicationDelegate {
    weak var store: Store? {
        didSet {
            // The main window may appear again while wake is waiting for close/join.
            // Keep that lifecycle task when onAppear reassigns the same Store.
            guard oldValue !== store else { return }
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
    @State private var search = ""
    @State private var showOther = false
    var profiles: [Profile] {
        store.orderedHostProfiles.filter { p in
            (showOther || DesktopAgent.named(p.name) != nil) && (search.isEmpty || p.name.localizedCaseInsensitiveContains(search))
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Micro Input").font(.headline)
                    Text("Creator Micro 2").font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Label(store.connectedID == nil ? "未连接" : "已连接", systemImage: "cable.connector")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Toggle("启用按键控制", isOn: Binding(get: { store.hostExecutionEnabled }, set: { value in Task { await store.setHostExecutionEnabled(value) } }))
                .toggleStyle(.switch).controlSize(.small)
            TextField("搜索 Agent 配置", text: $search).textFieldStyle(.roundedBorder)
            ScrollView {
                VStack(spacing: 4) {
                    ForEach(profiles, id: \.id.text) { p in
                        Button { Task { await store.setHostActiveProfile(p.id.text) } } label: {
                            HStack(spacing: 10) {
                                AgentBadge(name: p.name)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(p.name).fontWeight(.medium)
                                    if let agent = DesktopAgent.named(p.name) {
                                        Text(agent.installed ? "已安装" : "未找到应用").font(.caption2).foregroundStyle(.secondary)
                                    }
                                }
                                Spacer()
                                if store.hostActiveProfileId == p.id.text { Image(systemName: "checkmark").foregroundStyle(.tint) }
                            }.padding(8).contentShape(Rectangle())
                                .background(store.hostActiveProfileId == p.id.text ? Color.accentColor.opacity(0.10) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
                        }.buttonStyle(.plain).disabled(store.busy)
                        .accessibilityLabel("控制 \(p.name)\(store.hostActiveProfileId == p.id.text ? "，当前配置" : "")")
                    }
                    if profiles.isEmpty { Text("没有匹配的配置").foregroundStyle(.secondary).padding() }
                }
            }.frame(maxHeight: 290)
            Toggle("显示其他配置", isOn: $showOther).toggleStyle(.checkbox).font(.caption)
            Divider()
            Text(store.hostExecutionEnabled ? store.passThroughHealth.label : "按键控制已暂停")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if !LiveHostActionExecutor.isAccessibilityTrusted {
                Button("设置辅助功能权限…") { LiveHostActionExecutor.openAccessibilitySettings() }.font(.caption)
            }
            if let receipt = store.lastHostActionReceipt {
                Text("最近动作：\(receipt.succeeded ? "已执行" : "未执行") · \(receipt.detail)")
                    .font(.caption2).foregroundStyle(.secondary).lineLimit(2)
            }
            HStack {
                Button(store.connectedID == nil ? "连接设备" : "检查连接") { Task { await store.autoSetupHostExecutionIfNeeded() } }.disabled(store.busy)
                Spacer()
                Button("编辑按键…") { owner.showWindow() }
            }
            HStack {
                Text("配置切换仅作用于本机").font(.caption2).foregroundStyle(.secondary)
                Spacer()
                Button("退出") { owner.quitApp() }.buttonStyle(.plain).font(.caption)
            }
        }.padding(16).frame(width: 336).tint(Color(red: 0.20, green: 0.43, blue: 0.34))
    }
}
