// R-P03/R-H11: offline first-round workbench; no discovery, permission request or device open on launch.
import SwiftUI
import Darwin
import MicroInputCore

final class DocumentInstanceLock {
    let lockURL: URL
    private var fd: Int32

    private init(lockURL: URL, fd: Int32) {
        self.lockURL = lockURL
        self.fd = fd
    }

    static func acquire(directory: URL, pid: Int32 = ProcessInfo.processInfo.processIdentifier) -> DocumentInstanceLock? {
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let lockURL = directory.appendingPathComponent("instance.lock")
        let fd = Darwin.open(lockURL.path, O_RDWR | O_CREAT, S_IRUSR | S_IWUSR)
        guard fd >= 0 else { return nil }
        if flock(fd, LOCK_EX | LOCK_NB) != 0 {
            Darwin.close(fd)
            return nil
        }
        _ = Darwin.ftruncate(fd, 0)
        _ = Darwin.lseek(fd, 0, SEEK_SET)
        let payload = "\(pid)\n"
        payload.withCString { ptr in
            _ = Darwin.write(fd, ptr, strlen(ptr))
        }
        _ = Darwin.fsync(fd)
        return DocumentInstanceLock(lockURL: lockURL, fd: fd)
    }

    static func ownerPID(directory: URL) -> Int32? {
        let lockURL = directory.appendingPathComponent("instance.lock")
        let fd = Darwin.open(lockURL.path, O_RDONLY)
        guard fd >= 0 else { return nil }
        defer { Darwin.close(fd) }
        if flock(fd, LOCK_EX | LOCK_NB) == 0 {
            _ = flock(fd, LOCK_UN)
            return nil
        }
        guard let text = try? String(contentsOf: lockURL, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines),
              let pid = Int32(text) else { return -1 }
        return pid
    }

    func release() {
        guard fd >= 0 else { return }
        _ = flock(fd, LOCK_UN)
        Darwin.close(fd)
        fd = -1
    }

    deinit {
        release()
    }
}

@main struct MicroInputApp: App {
    @NSApplicationDelegateAdaptor(AppLifetime.self) private var lifetime
    @StateObject private var store: Store
    private let instanceLock: DocumentInstanceLock
    private let statusMenu: StatusMenu
    init() {
        PerformanceProbe.start()
        let docDir = Store.defaultDocumentDirectoryURL()
        guard let acquiredLock = DocumentInstanceLock.acquire(directory: docDir) else {
            if let holderPID = DocumentInstanceLock.ownerPID(directory: docDir),
               holderPID > 0,
               let running = NSRunningApplication(processIdentifier: holderPID) {
                running.activate(options: [])
            } else if let bundleID = Bundle.main.bundleIdentifier {
                NSRunningApplication.runningApplications(withBundleIdentifier: bundleID)
                    .first(where: { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier && !$0.isTerminated })?
                    .activate(options: [])
            }
            fputs("MicroInput instance already holds \(docDir.appendingPathComponent("instance.lock").path); exiting duplicate instance.\n", stderr)
            Darwin.exit(0)
        }
        self.instanceLock = acquiredLock
        let scenario = ProcessInfo.processInfo.environment["MI_SYNTHETIC_SCENARIO"] ?? ""
        #if MI_SYNTHETIC_BACKEND
        let core = scenario == "radial-menu" ? MicroInput.syntheticRadialDevice() : ["config-write", "lighting-control"].contains(scenario) ? MicroInput.syntheticConfigWriter() : ["reconnect", "reconnect-transient", "apply-preparation", "host-execution", "zcode-navigation", "dsh-navigation", "cindy-navigation", "antigravity-status", "native-ownership", "diagnostic-export", "push-to-talk", "push-to-talk-live"].contains(scenario) ? MicroInput.syntheticDiscovery() : MicroInput()
        let ownership = scenario == "native-ownership" ? NativeOwnershipController(transport: nil) : NativeOwnershipController(isolated: true)
        let store = Store(nativeOwnership: ownership, backend: core, actionExecutor: ["native-ownership", "zcode-navigation", "dsh-navigation", "cindy-navigation", "antigravity-status"].contains(scenario) ? RecordingHostActionExecutor() : nil, zcodeSessions: scenario == "zcode-navigation" ? ZCodeSessionController(client: ZCodeNavigationFixture.shared, pollNanoseconds: 60_000_000_000) : nil, dshSessions: scenario == "dsh-navigation" ? DSHSessionController(client: DSHNavigationFixture.shared, pollNanoseconds: 60_000_000_000) : nil, cindySessions: scenario == "cindy-navigation" ? CindySessionController(client: CindyNavigationFixture.shared, pollNanoseconds: 60_000_000_000) : nil, antigravityStatus: scenario == "antigravity-status" ? AntigravityStatusController(client: AntigravityStatusFixture.shared, pollNanoseconds: 60_000_000_000) : nil)
        #else
        let core = MicroInput()
        let store = Store(nativeOwnership: NativeOwnershipController(allowUnbridgedDesktop: true), backend: core)
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
                Text("⌘E 分配动作，⌘D 查看修改，⌘Z 撤销")
            }
        }

    }
}

enum PerformanceProbe {
    private static var startedAt: DispatchTime?
    private static var startUptime: Double?
    private static var marked = false

    static func start() {
        guard startedAt == nil else { return }
        startedAt = DispatchTime.now()
        startUptime = ProcessInfo.processInfo.systemUptime
    }

    static func markWorkbenchReady() {
        guard !marked, let auditLog = ProcessInfo.processInfo.environment["MI_HOST_AUDIT_LOG"] else { return }
        marked = true
        guard let startedAt, let startUptime else { return }
        let evidenceDirectory = URL(fileURLWithPath: auditLog).deletingLastPathComponent()
        let payload: [String: Any] = [
            "pid": ProcessInfo.processInfo.processIdentifier,
            "ready_at_unix_ms": Int(Date().timeIntervalSince1970 * 1000),
            "monotonic_ready_ms": Double(DispatchTime.now().uptimeNanoseconds - startedAt.uptimeNanoseconds) / 1_000_000,
            "process_ready_ms": (ProcessInfo.processInfo.systemUptime - startUptime) * 1000,
            "trigger": "workbench-on-appear",
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys]) else { return }
        let path = evidenceDirectory.appendingPathComponent("app-ready.json")
        FileManager.default.createFile(atPath: path.path, contents: data, attributes: [.posixPermissions: 0o600])
    }
}

// R-H11/R-P03: native status menu with a keyboard route, including when the editor is closed.
struct WorkbenchHost: View {
    @ObservedObject var store: Store
    let statusMenu: StatusMenu
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        Workbench(store: store).onAppear {
            PerformanceProbe.markWorkbenchReady()
            statusMenu.open = { openWindow(id: "main"); NSApp.activate(ignoringOtherApps: true) }
        }
    }
}
@MainActor final class StatusMenu: NSObject {
    let store: Store
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
    var open: (() -> Void)?
    private let popover = NSPopover()
    private var previousApp: NSRunningApplication?
    private var linksWindow: NSWindow?
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
        _ = store.refreshAccessibilityTrust()
        let front = NSWorkspace.shared.frontmostApplication
        if front?.bundleIdentifier != Bundle.main.bundleIdentifier { previousApp = front }
        NSApp.activate(ignoringOtherApps: true)
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        popover.contentViewController?.view.window?.makeKey()
    }
    func toggleCheatSheet() {
        popover.performClose(nil)
        if NSWorkspace.shared.frontmostApplication?.processIdentifier == ProcessInfo.processInfo.processIdentifier,
           let previousApp, !previousApp.isTerminated { previousApp.activate(options: []) }
        store.overlay.toggleSheet()
    }
    func editAppLinks() {
        popover.performClose(nil)
        if linksWindow == nil {
            let window = NSWindow(contentRect: .zero, styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.title = "应用与 Profile"; window.isReleasedWhenClosed = false
            window.contentView = NSHostingView(rootView: AppProfileLinksView(store: store, close: { [weak window] in window?.close() })); window.center()
            linksWindow = window
        }
        NSApp.activate(ignoringOtherApps: true); linksWindow?.makeKeyAndOrderFront(nil)
    }
    @objc func showWindow() { popover.performClose(nil); store.syncWorkbenchToActiveProfile(); open?() }
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
        if !store.accessibilityTrusted { return "需要辅助功能权限" }
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
            Toggle("命令执行同意", isOn: Binding(get: { store.commandExecutionEnabled }, set: { value in Task { await store.setCommandExecutionEnabled(value) } }))
                .toggleStyle(.switch).controlSize(.small).font(.caption)
                .help("默认关闭。开启后，配置过的 shell_command 才会路由执行")
            if !store.accessibilityTrusted {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(UITheme.caution)
                    Text("辅助功能未就绪，按键与按住说话暂不可用").font(UITheme.Typography.caption).foregroundStyle(UITheme.caution)
                    Spacer()
                    Button("开启…") {
                        store.requestAccessibilityAuthorization()
                    }.font(UITheme.Typography.captionMedium)
                    Button("重置…") {
                        store.resetAndRequestAccessibilityAuthorization()
                    }.font(UITheme.Typography.caption).help("若系统设置中已打开开关仍提示未授权，点击重置旧签名登记并重新授权")
                }
            }
            if store.dictationNeedsAttention {
                Text("上次语音输入停止状态待确认").font(UITheme.Typography.caption).foregroundStyle(UITheme.caution)
                Button("重新检查麦克风状态") { Task { await store.verifyDictationStopped() } }
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
                Menu("其他方案") {
                    ForEach(otherProfiles, id: \.id.text) { p in
                        Button { Task { await store.setHostActiveProfile(p.id.text, syncWorkbench: true) } } label: {
                            if store.hostActiveProfileId == p.id.text { Label(DesktopAgent.displayName(p.name), systemImage: "checkmark") }
                            else { Text(DesktopAgent.displayName(p.name)) }
                        }.disabled(store.busy)
                    }
                    if otherProfiles.isEmpty { Text("没有其他方案") }
                    Divider()
                    Menu("调整常用排列") {
                        ForEach(0..<4, id: \.self) { index in
                            Menu(["左上", "右上", "左下", "右下"][index]) {
                                ForEach(menuProfiles, id: \.id.text) { profile in
                                    Button { pin(profile, at: index) } label: {
                                        if favorites.indices.contains(index), favorites[index].id.text == profile.id.text {
                                            Label(DesktopAgent.displayName(profile.name), systemImage: "checkmark")
                                        } else { Text(DesktopAgent.displayName(profile.name)) }
                                    }
                                }
                            }
                        }
                        Divider()
                        Button("恢复默认排列") { favoriteIDsJSON = "[]" }
                    }
                }.menuStyle(.borderlessButton).fixedSize().font(UITheme.Typography.captionMedium)
                Spacer()
                Button("打开设置窗口…") { owner.showWindow() }.buttonStyle(.plain).font(UITheme.Typography.captionMedium).foregroundStyle(UITheme.accent)
            }
            Divider()
            Toggle("摇杆八向轮盘菜单", isOn: Binding(get: { store.radialMenu.enabled }, set: store.setRadialMenuEnabled))
                .toggleStyle(.switch).controlSize(.small).font(UITheme.Typography.body)
            if store.radialMenu.enabled { Text(store.radialReadiness.label).font(UITheme.Typography.caption).foregroundStyle(UITheme.textSecondary) }
            DisclosureGroup("应用会话直连") {
                VStack(alignment: .leading, spacing: 8) {
                    ZCodeSessionPanel(controller: store.zcodeSessions, open: { store.openZCodeSession($0) })
                    DSHSessionPanel(
                        controller: store.dshSessions,
                        open: { store.openDSHSession($0) },
                        create: { wsId in
                            if let cmd = store.dshSessions.createIntent(workspaceId: wsId) { store.createDSHSession(cmd) }
                        },
                        cancel: { sessionId in
                            if let cmd = store.dshSessions.cancelIntent(for: sessionId) { store.cancelDSHSession(cmd) }
                        }
                    )
                    CindySessionPanel(controller: store.cindySessions, open: { store.openCindySession($0) })
                    AntigravityStatusPanel(controller: store.antigravityStatus, open: { store.openAntigravitySession($0) })
                }.padding(.top, 6)
            }.font(UITheme.Typography.captionMedium)
            Divider()
            Toggle("切换到软件时自动启用", isOn: Binding(get: { store.routing?.auto_follow_enabled ?? false }, set: { value in Task { await store.setAutoFollow(value); if value, store.routing?.manual_override == true { await store.resumeFollowing() } } })).toggleStyle(.switch).controlSize(.small).font(UITheme.Typography.body)
            HStack {
                Text(store.routingMode).font(UITheme.Typography.caption).foregroundStyle(UITheme.textSecondary)
                Spacer()
                if store.routing?.manual_override == true { Button("恢复自动跟随") { Task { await store.resumeFollowing() } }.font(UITheme.Typography.captionMedium) }
            }
            HStack {
                Button("软件与方案关联…") { owner.editAppLinks() }
                Spacer()
                Button("按键速查") { owner.toggleCheatSheet() }
            }.font(UITheme.Typography.captionMedium)
            if store.inputNeedsResume { Button("恢复按键监听") { store.resumeInput() } }
            Divider()
            HStack(spacing: 8) {
                Circle().fill(store.connectedID == nil ? UITheme.textMuted : UITheme.accent).frame(width: 6, height: 6)
                Text(store.connectedID == nil ? "键盘未连接" : "Creator Micro 2 已就绪").font(UITheme.Typography.caption).foregroundStyle(UITheme.textSecondary).lineLimit(1)
                Spacer(minLength: 4)
                Menu {
                    Text(summary)
                    Text(store.passThroughHealth.label)
                    if let receipt = store.lastHostActionReceipt {
                        Text("最近动作：\(receipt.detail)")
                    }
                    Divider()
                    Button(store.connectedID == nil ? "连接键盘" : "重新检查连接") {
                        Task { await store.autoSetupHostExecutionIfNeeded() }
                    }.disabled(store.busy)
                    Button("设置辅助功能权限…") { store.requestAccessibilityAuthorization() }
                    if !store.accessibilityTrusted {
                        Button("重置辅助功能签名登记…") { store.resetAndRequestAccessibilityAuthorization() }
                    }
                    Divider()
                    Button("退出 Micro Input") { owner.quitApp() }
                } label: { Image(systemName: "ellipsis.circle").font(UITheme.Typography.body) }
                    .menuStyle(.borderlessButton).fixedSize().accessibilityLabel("连接详情")
            }
        }.padding(14).frame(width: 348).fixedSize(horizontal: false, vertical: true)
            .background(UITheme.surface)
            .tint(UITheme.accent)
            .accentColor(UITheme.accent)
    }
    private func favoriteButton(at index: Int) -> some View {
        let profile = favorites.indices.contains(index) ? favorites[index] : nil
        let name = profile?.name ?? "未设置"
        let agent = DesktopAgent.named(name)
        let title = DesktopAgent.displayName(name)
        let selected = profile != nil && store.hostActiveProfileId == profile?.id.text
        return Button {
            guard let profile else { return }
            Task { await store.setHostActiveProfile(profile.id.text, syncWorkbench: true) }
        } label: {
            HStack(spacing: 8) {
                AgentBadge(name: name, size: 26)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(UITheme.Typography.bodyMedium).foregroundStyle(UITheme.textPrimary).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                    Text(selected ? "使用中" : (agent?.installed == false ? "未安装" : "点击启用"))
                        .font(UITheme.Typography.caption).foregroundStyle(selected ? UITheme.accent : UITheme.textSecondary)
                }
                Spacer(minLength: 0)
                Image(systemName: "checkmark").font(UITheme.Typography.captionMedium)
                    .foregroundStyle(UITheme.accent).opacity(selected ? 1 : 0)
            }.padding(.horizontal, 10).frame(maxWidth: .infinity).frame(height: 62)
                .contentShape(RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                .background(selected ? UITheme.accentSoft : UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.itemInner).strokeBorder(selected ? UITheme.accentBorder : UITheme.border))
        }.buttonStyle(.plain).disabled(store.busy || profile == nil)
            .accessibilityLabel("启用 \(title)\(selected ? "，当前方案" : "")")
    }
}

struct ZCodeSessionPanel: View {
    @ObservedObject var controller: ZCodeSessionController
    let open: (ZCodeSelection) -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Toggle("ZCode 会话连接", isOn: Binding(get: { controller.enabled }, set: controller.setEnabled))
                .toggleStyle(.switch).controlSize(.small)
            Text(controller.state.label).font(.caption).foregroundStyle(.secondary)
            if controller.enabled {
                if case .unavailable = controller.state {
                    Button("重连") { controller.setEnabled(true) }.font(.caption)
                }
                ForEach(controller.rows) { row in
                    Button {
                        if let selection = controller.selection(for: row.address) { open(selection) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(row.title.isEmpty ? "未命名会话" : row.title).lineLimit(1)
                                if controller.selected == row.address { Image(systemName: "checkmark").accessibilityLabel("已选中") }
                            }
                            Text(row.address.workspacePath).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                                .truncationMode(.middle).help(row.address.workspacePath)
                        }
                    }.buttonStyle(.plain).disabled(controller.state != .ready)
                }
                if let result = controller.lastResult {
                    Text(result.label).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct DSHSessionPanel: View {
    @ObservedObject var controller: DSHSessionController
    let open: (DSHSelection) -> Void
    var create: ((String) -> Void)? = nil
    var cancel: ((String) -> Void)? = nil
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Toggle("DSH 会话连接", isOn: Binding(get: { controller.enabled }, set: controller.setEnabled))
                .toggleStyle(.switch).controlSize(.small)
            Text(controller.state.label).font(.caption).foregroundStyle(.secondary)
            if controller.enabled {
                if case .unavailable = controller.state {
                    Button("重连") { controller.setEnabled(true) }.font(.caption)
                }
                if controller.state == .ready, let create,
                   let selectedWorkspace = controller.rows.first(where: { $0.sessionId == controller.selected })?.workspaceId
                       ?? controller.rows.first?.workspaceId {
                    HStack(spacing: 8) {
                        Button("新建会话") { create(selectedWorkspace) }.font(.caption)
                        if let selectedId = controller.selected,
                           let cancel,
                           controller.rows.first(where: { $0.sessionId == selectedId })?.running == true {
                            Button("停止当前轮次") { cancel(selectedId) }.font(.caption)
                        }
                    }
                }
                ForEach(controller.rows) { row in
                    Button {
                        if let selection = controller.selection(for: row.sessionId) { open(selection) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(row.title.isEmpty ? "未命名会话" : row.title).lineLimit(1)
                                if controller.selected == row.sessionId { Image(systemName: "checkmark").accessibilityLabel("已选中") }
                            }
                            Text(row.workspaceId.isEmpty ? "未分组" : row.workspaceId).font(.caption2)
                                .foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                            HStack(spacing: 8) {
                                Text(row.turnStatusLabel).font(.caption2).foregroundStyle(.secondary)
                                    .accessibilityLabel("DSH当前轮次：\(row.turnStatusLabel)")
                                Text(row.queueStatusLabel).font(.caption2).foregroundStyle(.secondary)
                                    .accessibilityLabel("DSH队列：\(row.queueStatusLabel)")
                            }
                            if row.pendingInput > 0 {
                                Text("待输入 \(row.pendingInput)").font(.caption2).foregroundStyle(.secondary)
                            }
                        }
                    }.buttonStyle(.plain).disabled(controller.state != .ready)
                }
                if !controller.unavailableSessionIds.isEmpty {
                    Text("另有 \(controller.unavailableSessionIds.count) 个授权会话当前不可用，状态未知")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if let result = controller.lastResult {
                    Text(result.label).font(.caption).foregroundStyle(.secondary)
                }
                if let commandResult = controller.lastCommandResult {
                    Text(commandResult.label).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct AntigravityStatusPanel: View {
    @ObservedObject var controller: AntigravityStatusController
    let open: (AntigravitySelection) -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Toggle("Antigravity 状态连接", isOn: Binding(get: { controller.enabled }, set: controller.setEnabled))
                .toggleStyle(.switch).controlSize(.small)
            Text(controller.state.label).font(.caption).foregroundStyle(.secondary)
            if controller.enabled {
                if case .unavailable = controller.state {
                    Button("重连") { controller.setEnabled(true) }.font(.caption)
                }
                if let caps = controller.capabilities {
                    Text("版本 \(caps.installedVersion) · CL \(caps.languageServerCl)")
                        .font(.caption2).foregroundStyle(.secondary)
                }
                ForEach(controller.rows) { row in
                    Button {
                        if let selection = controller.selection(for: row.key) { open(selection) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(row.title.isEmpty ? row.conversationId : row.title).lineLimit(1)
                                if controller.selected == row.key {
                                    Image(systemName: "checkmark").accessibilityLabel("已选中")
                                }
                            }
                            Text(row.workspacePath).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                                .truncationMode(.middle).help(row.workspacePath)
                            Text("\(row.conversationId) · \(row.statusLabel)").font(.caption2).foregroundStyle(.secondary)
                        }
                    }.buttonStyle(.plain).disabled(controller.state != .ready)
                }
                if let result = controller.lastResult {
                    Text(result.label).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}

struct CindySessionPanel: View {
    @ObservedObject var controller: CindySessionController
    let open: (CindySelection) -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Toggle("Cindy 任务连接", isOn: Binding(get: { controller.enabled }, set: controller.setEnabled))
                .toggleStyle(.switch).controlSize(.small)
            Text(controller.state.label).font(.caption).foregroundStyle(.secondary)
            if controller.enabled {
                if case .unavailable = controller.state {
                    Button("重连") { controller.setEnabled(true) }.font(.caption)
                }
                ForEach(controller.rows) { row in
                    Button {
                        if let selection = controller.selection(for: row.address) { open(selection) }
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack {
                                Text(row.title.isEmpty ? "未命名任务" : row.title).lineLimit(1)
                                if controller.selected == row.address {
                                    Image(systemName: "checkmark").accessibilityLabel("已确认")
                                } else if controller.delivered == row.address {
                                    Image(systemName: "arrow.up.right").accessibilityLabel("已投递")
                                }
                            }
                            Text(row.address.workspacePath).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                                .truncationMode(.middle).help(row.address.workspacePath)
                            Text("\(row.address.sessionId) · \(row.phase)").font(.caption2).foregroundStyle(.secondary)
                        }
                    }.buttonStyle(.plain).disabled(controller.state != .ready)
                }
                if let result = controller.lastResult {
                    Text(result.label).font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }
}
