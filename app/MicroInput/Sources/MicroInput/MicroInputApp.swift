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
        let core = ["config-write", "lighting-control"].contains(scenario) ? MicroInput.syntheticConfigWriter() : ["reconnect", "reconnect-transient", "apply-preparation", "host-execution"].contains(scenario) ? MicroInput.syntheticDiscovery() : MicroInput()
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
    @AppStorage("menuFavoriteProfileIDs") private var favoriteIDsJSON = "[]"
    private var menuProfiles: [Profile] {
        store.orderedHostProfiles.filter { !["通用 Coding · 演示", "Herdr · 示例"].contains($0.name) }
    }
    private var favorites: [Profile] {
        let ids = (try? JSONDecoder().decode([String].self, from: Data(favoriteIDsJSON.utf8))) ?? []
        return MenuFavorites.resolve(ids: ids, profiles: menuProfiles)
    }
    private var otherProfiles: [Profile] {
        let pinned = Set(favorites.map { $0.id.text })
        return menuProfiles.filter { !pinned.contains($0.id.text) }
    }
    private func pin(_ profile: Profile, at index: Int) {
        let ids = MenuFavorites.replacing(ids: favorites.map { $0.id.text }, at: index, with: profile.id.text)
        if let data = try? JSONEncoder().encode(ids), let json = String(data: data, encoding: .utf8) {
            favoriteIDsJSON = json
        }
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
                    Text("Micro Input").font(.system(size: 14, weight: .semibold))
                    Text("Creator Micro 2").font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Spacer()
                Toggle("按键控制", isOn: Binding(get: { store.hostExecutionEnabled }, set: { value in Task { await store.setHostExecutionEnabled(value) } }))
                    .labelsHidden().toggleStyle(.switch).controlSize(.small).help("启用或暂停按键控制")
            }
            // Intrinsic, non-scrolling rows: a maxHeight-only ScrollView can collapse
            // inside NSHostingController's initial popover size negotiation.
            VStack(spacing: 8) {
                HStack(spacing: 8) {
                    favoriteButton(at: 0)
                    favoriteButton(at: 1)
                }
                HStack(spacing: 8) {
                    favoriteButton(at: 2)
                    favoriteButton(at: 3)
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
                    Divider()
                    Menu("调整常用配置") {
                        ForEach(0..<4, id: \.self) { index in
                            Menu(["左上", "右上", "左下", "右下"][index]) {
                                ForEach(menuProfiles, id: \.id.text) { profile in
                                    Button { pin(profile, at: index) } label: {
                                        if favorites.indices.contains(index), favorites[index].id.text == profile.id.text {
                                            Label(profile.name, systemImage: "checkmark")
                                        } else { Text(profile.name) }
                                    }
                                }
                            }
                        }
                        Divider()
                        Button("恢复默认排列") { favoriteIDsJSON = "[]" }
                    }
                }.menuStyle(.borderlessButton).fixedSize().font(.system(size: 11))
                Spacer()
                Button("编辑按键…") { owner.showWindow() }.buttonStyle(.plain).font(.system(size: 11)).foregroundStyle(.secondary)
            }
            Divider()
            HStack(spacing: 8) {
                Circle().fill(store.connectedID == nil ? Color.secondary : Color.green).frame(width: 5, height: 5)
                Text(store.connectedID == nil ? "设备未连接" : "Creator Micro 2 已连接").font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                Spacer(minLength: 4)
                Menu {
                    Text(summary)
                    Text(store.passThroughHealth.label)
                    if let receipt = store.lastHostActionReceipt {
                        Text("最近动作：\(receipt.succeeded ? "已执行" : "未执行") · \(receipt.detail)")
                    }
                    Divider()
                    Button(store.connectedID == nil ? "连接设备" : "检查连接") {
                        Task { await store.autoSetupHostExecutionIfNeeded() }
                    }.disabled(store.busy)
                    Button("设置辅助功能权限…") { LiveHostActionExecutor.openAccessibilitySettings() }
                    Divider()
                    Button("退出 Micro Input") { owner.quitApp() }
                } label: { Image(systemName: "ellipsis.circle").font(.system(size: 13)) }
                    .menuStyle(.borderlessButton).fixedSize().accessibilityLabel("连接详情")
            }
        }.padding(16).frame(width: 352).fixedSize(horizontal: false, vertical: true)
            .tint(.accentColor)
    }
    private func favoriteButton(at index: Int) -> some View {
        let profile = favorites.indices.contains(index) ? favorites[index] : nil
        let name = profile?.name ?? "未设置"
        let agent = DesktopAgent.named(name)
        let title = name == "DeepSeek Coder (DSH)" ? "DSH" : name
        let selected = profile != nil && store.hostActiveProfileId == profile?.id.text
        return Button {
            guard let profile else { return }
            Task { await store.setHostActiveProfile(profile.id.text) }
        } label: {
            HStack(spacing: 8) {
                AgentBadge(name: name, size: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.system(size: 12, weight: .medium)).lineLimit(1)
                    Text(selected ? "正在控制" : (agent?.installed == false ? "未安装" : "切换配置"))
                        .font(.system(size: 10)).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
                Image(systemName: "checkmark").font(.caption.weight(.semibold))
                    .foregroundStyle(.tint).opacity(selected ? 1 : 0)
            }.padding(.horizontal, 10).frame(maxWidth: .infinity).frame(height: 64)
                .contentShape(RoundedRectangle(cornerRadius: 9))
                .background(selected ? Color.accentColor.opacity(0.08) : Color(nsColor: .controlBackgroundColor).opacity(0.65), in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(selected ? Color.accentColor.opacity(0.45) : Color.primary.opacity(0.06)))
        }.buttonStyle(.plain).disabled(store.busy || profile == nil)
            .accessibilityLabel("控制 \(name)\(selected ? "，当前配置" : "")")
    }
}
