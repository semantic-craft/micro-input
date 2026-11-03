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
        popover.animates = false
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
    private var otherProfiles: [Profile] {
        store.orderedHostProfiles.filter { DesktopAgent.named($0.name) == nil }
    }
    private var summary: String {
        if !store.hostExecutionEnabled { return "按键控制已暂停" }
        if !LiveHostActionExecutor.isAccessibilityTrusted { return "需要辅助功能权限" }
        return store.connectedID == nil ? "设备未连接" : store.passThroughHealth.label
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
            Toggle("按键控制", isOn: Binding(get: { store.hostExecutionEnabled }, set: { value in Task { await store.setHostExecutionEnabled(value) } }))
                .toggleStyle(.switch).controlSize(.small)
            // Intrinsic, non-scrolling rows: a maxHeight-only ScrollView can collapse
            // inside NSHostingController's initial popover size negotiation.
            VStack(spacing: 8) {
                HStack(spacing: 8) {
                    agentButton(DesktopAgent.all[0])
                    agentButton(DesktopAgent.all[1])
                }
                HStack(spacing: 8) {
                    agentButton(DesktopAgent.all[2])
                    agentButton(DesktopAgent.all[3])
                }
            }.fixedSize(horizontal: false, vertical: true)
            HStack {
                Menu("其他配置") {
                    ForEach(otherProfiles, id: \.id.text) { p in
                        Button { Task { await store.setHostActiveProfile(p.id.text) } } label: {
                            if store.hostActiveProfileId == p.id.text { Label(p.name, systemImage: "checkmark") }
                            else { Text(p.name) }
                        }.disabled(store.busy)
                    }
                    if otherProfiles.isEmpty { Text("没有其他配置") }
                }.fixedSize()
                Spacer()
                Button("编辑按键…") { owner.showWindow() }
            }
            Divider()
            HStack(spacing: 8) {
                Text(summary).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Spacer(minLength: 4)
                Menu("连接详情") {
                    Text(store.passThroughHealth.label)
                    if let receipt = store.lastHostActionReceipt {
                        Text("最近动作：\(receipt.succeeded ? "已执行" : "未执行") · \(receipt.detail)")
                    }
                    Divider()
                    Button(store.connectedID == nil ? "连接设备" : "检查连接") {
                        Task { await store.autoSetupHostExecutionIfNeeded() }
                    }.disabled(store.busy)
                    Button("设置辅助功能权限…") { LiveHostActionExecutor.openAccessibilitySettings() }
                }.fixedSize().font(.caption)
            }
            HStack {
                Text("配置切换仅作用于本机").font(.caption2).foregroundStyle(.secondary)
                Spacer()
                Button("退出") { owner.quitApp() }.buttonStyle(.plain).font(.caption)
            }
        }.padding(16).frame(width: 336).fixedSize(horizontal: false, vertical: true)
            .tint(Color(red: 0.20, green: 0.43, blue: 0.34))
    }
    private func agentButton(_ agent: DesktopAgent) -> some View {
        let profile = store.hostProfiles.first { $0.name == agent.name }
        let selected = profile != nil && store.hostActiveProfileId == profile?.id.text
        return Button {
            guard let profile else { return }
            Task { await store.setHostActiveProfile(profile.id.text) }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: agent.symbol).foregroundStyle(.tint).frame(width: 20)
                VStack(alignment: .leading, spacing: 2) {
                    Text(agent.name).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    if !agent.installed { Text("未找到应用").font(.caption2).foregroundStyle(.secondary) }
                }
                Spacer(minLength: 0)
                Image(systemName: "checkmark").font(.caption.weight(.semibold))
                    .foregroundStyle(.tint).opacity(selected ? 1 : 0)
            }.padding(.horizontal, 10).frame(maxWidth: .infinity).frame(height: 52)
                .contentShape(RoundedRectangle(cornerRadius: 9))
                .background(selected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(selected ? Color.accentColor.opacity(0.4) : Color.primary.opacity(0.06)))
        }.buttonStyle(.plain).disabled(store.busy || profile == nil)
            .accessibilityLabel("控制 \(agent.name)\(selected ? "，当前配置" : "")")
    }
}
