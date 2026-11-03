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
            let panel = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 320, height: 380), styleMask: [.titled, .utilityWindow], backing: .buffered, defer: false)
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
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(store.passThroughHealth.label)
                .font(.caption.bold())
                .foregroundStyle(store.passThroughHealth == .ready ? Color.green : Color.secondary)
            if case .layerDriftWarning = store.passThroughHealth {
                Text("键盘当前不在宿主透传层，请轻触键盘左下角 Logo 切回第 0 层")
                    .font(.caption2)
                    .foregroundStyle(.orange)
            }
            if store.connectedID != nil, store.passThroughHealth != .ready {
                Button("重新检查身份/层状态") {
                    Task {
                        await store.readIdentity()
                        if store.passThroughHealth == .snapshotMissing,
                           ["0.6.3", "synthetic-0.6.3"].contains(store.identity?.version ?? "") {
                            await store.readDeviceConfig()
                            if store.browsingDeviceSnapshot {
                                store.showLocalDraft()
                            }
                        }
                    }
                }
                .font(.caption2)
            }
            Toggle("启用宿主动作执行", isOn: Binding(
                get: { store.hostExecutionEnabled },
                set: { enabled in Task { await store.setHostExecutionEnabled(enabled) } }
            ))
            .font(.caption)
            let axTrusted = LiveHostActionExecutor.isAccessibilityTrusted
            HStack(spacing: 6) {
                Text(axTrusted ? "辅助功能权限：已授权" : "辅助功能权限：未授权（快捷键与粘贴需要）")
                    .font(.caption2)
                    .foregroundStyle(axTrusted ? Color.secondary : Color.orange)
                if !axTrusted {
                    Button("打开系统辅助功能设置…") {
                        LiveHostActionExecutor.openAccessibilitySettings()
                    }
                    .font(.caption2)
                }
            }
            if let receipt = store.lastHostActionReceipt {
                Text("最近触发：\(receipt.bindingId) → \(receipt.actionType) (\(receipt.detail))")
                    .font(.caption2)
                    .foregroundStyle(receipt.succeeded ? Color.secondary : Color.red)
                    .lineLimit(1)
            }
            Divider()
            Text("宿主激活 Profile（切换不写设备 Flash）：")
                .font(.caption2)
                .foregroundStyle(.secondary)
            ForEach(Array(store.hostProfiles.enumerated()), id: \.offset) { _, p in
                let active = store.hostActiveProfileId == p.id.text
                Button(active ? "✓ \(p.name)" : p.name) {
                    Task { await store.setHostActiveProfile(p.id.text) }
                }
            }
            Divider()
            Button("一键就绪（连接 USB + 预设 + 开启接管）") {
                Task { await store.autoSetupHostExecutionIfNeeded() }
            }
            .disabled(store.busy)
            Button("安装 Coding Agent 预设") {
                Task { await store.installCodingAgentPresets() }
            }
            .disabled(!store.canEditDraft || store.busy)
            Button("打开工作台") { owner.showWindow() }
            Button("退出 Micro Input") { owner.quitApp() }
        }.padding(16).frame(width: 320, height: 450, alignment: .topLeading)
    }
}
