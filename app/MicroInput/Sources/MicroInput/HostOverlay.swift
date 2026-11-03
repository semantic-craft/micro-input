import AppKit
import SwiftUI

final class PassiveHostPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}
@MainActor final class HostOverlay {
    weak var store: Store?
    private(set) var sheetVisible = false
    private var panel: NSPanel?
    private var dismissal: Task<Void, Never>?
    init(store: Store) { self.store = store }
    static func position(size: NSSize, screen: NSRect) -> NSRect {
        let width = min(size.width, max(0, screen.width - 40))
        let height = min(size.height, max(0, screen.height - 40))
        return NSRect(x: max(screen.minX, screen.maxX - width - 20), y: max(screen.minY, screen.maxY - height - 20), width: width, height: height)
    }
    private func present<Content: View>(_ content: Content, size: NSSize, acceptsMouse: Bool) {
        panel?.orderOut(nil)
        let p = PassiveHostPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        p.level = .floating; p.isFloatingPanel = true; p.hidesOnDeactivate = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        p.isOpaque = false; p.backgroundColor = .clear; p.hasShadow = true
        p.ignoresMouseEvents = !acceptsMouse
        p.contentView = NSHostingView(rootView: content)
        let mouse = NSEvent.mouseLocation
        if let screen = NSScreen.screens.first(where: { $0.frame.contains(mouse) }) ?? NSScreen.main {
            p.setFrame(Self.position(size: size, screen: screen.visibleFrame), display: true)
        }
        panel = p; p.orderFrontRegardless()
    }
    func showNotice(name: String, mode: String) {
        dismissal?.cancel()
        guard !sheetVisible else { refresh(); return }
        present(VStack(alignment: .leading, spacing: 5) { Text(name).font(.headline); Text(mode).font(.caption).foregroundStyle(.secondary) }.padding(16).frame(maxWidth: .infinity, alignment: .leading).background(.regularMaterial).clipShape(RoundedRectangle(cornerRadius: 14)), size: NSSize(width: 280, height: 78), acceptsMouse: false)
        dismissal = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(1500)) } catch { return }
            self?.panel?.orderOut(nil)
        }
    }
    func toggleSheet() { sheetVisible ? hide() : showSheet() }
    func showSheet() {
        dismissal?.cancel(); sheetVisible = true; refresh()
    }
    func refresh() {
        guard sheetVisible, let store else { return }
        present(HostCheatSheet(store: store, close: { [weak self] in self?.hide() }), size: NSSize(width: 400, height: 610), acceptsMouse: true)
    }
    func hide() { dismissal?.cancel(); panel?.orderOut(nil); panel = nil; sheetVisible = false }
}
struct HostCheatSheet: View {
    @ObservedObject var store: Store
    let close: () -> Void
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack { Text("按键速查").font(.headline); Spacer(); Button(action: close) { Image(systemName: "xmark.circle.fill") }.buttonStyle(.plain).accessibilityLabel("关闭按键速查") }
            Text(store.routing?.profile_name ?? "Profile 不可用").font(.title3)
            Text("\(store.routingMode) · \(store.hostExecutionEnabled ? "按键控制开启" : "按键控制暂停") · 首层映射").font(.caption).foregroundStyle(.secondary)
            ScrollView {
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 8) {
                    ForEach(store.routing?.keys ?? []) { key in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(controlName(key)).font(.caption2).foregroundStyle(.secondary)
                            Text(key.label).font(.caption).lineLimit(2).foregroundStyle(key.available ? .primary : .secondary)
                        }.frame(maxWidth: .infinity, minHeight: 38, alignment: .leading).padding(7).background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 6))
                    }
                }
            }
        }.padding(16).frame(maxWidth: .infinity, maxHeight: .infinity).background(.regularMaterial).clipShape(RoundedRectangle(cornerRadius: 14))
    }
    private func controlName(_ key: HostKey) -> String {
        let names = ["AG10":"宽键（合并）", "AG12":"底行右键", "AG13":"旋钮逆时针", "AG14":"旋钮顺时针", "ENC_CLK":"编码器按下", "AG15":"摇杆 ↑", "AG16":"摇杆 ←", "AG17":"摇杆 ↓", "AG18":"摇杆 →"]
        let keys = ["AG01":"第 1 行 · 左", "AG00":"第 1 行 · 右", "AG02":"第 2 行 · 1", "AG03":"第 2 行 · 2", "AG04":"第 2 行 · 3", "AG05":"第 2 行 · 4", "AG06":"第 3 行 · 1", "AG07":"第 3 行 · 2", "AG08":"第 3 行 · 3", "AG09":"第 3 行 · 4"]
        return names[key.binding_id] ?? keys[key.binding_id] ?? key.control
    }
}
struct AppProfileLinksView: View {
    @ObservedObject var store: Store
    let close: () -> Void
    @State private var bundleID = ""
    @State private var profileID = ""
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack { Text("应用与 Profile").font(.title2); Spacer(); Button("完成", action: close).keyboardShortcut(.cancelAction) }
            Text("未关联的应用保留当前配置。手动选择后保持不变，直到恢复跟随。").font(.callout).foregroundStyle(.secondary)
            HStack {
                Button("选择应用…") {
                    let picker = NSOpenPanel(); picker.canChooseDirectories = false; picker.allowedContentTypes = [.applicationBundle]
                    picker.directoryURL = URL(fileURLWithPath: "/Applications")
                    if picker.runModal() == .OK, let url = picker.url, let id = Bundle(url: url)?.bundleIdentifier { bundleID = id }
                }
                Text(bundleID.isEmpty ? "尚未选择应用" : bundleID).lineLimit(1)
            }
            Picker("Profile", selection: $profileID) {
                Text("请选择").tag("")
                ForEach(store.hostProfiles, id: \.id.text) { Text($0.name).tag($0.id.text) }
            }
            Button("保存关联") { Task { await store.linkApp(bundleID, profileID: profileID) } }.disabled(bundleID.isEmpty || profileID.isEmpty)
            Divider()
            List((store.routing?.app_profile_links ?? [:]).keys.sorted(), id: \.self) { app in
                HStack {
                    VStack(alignment: .leading) {
                        Text(app)
                        Text(store.hostProfiles.first(where: { $0.id.text == store.routing?.app_profile_links[app] })?.name ?? "Profile 已不存在").foregroundStyle(.secondary)
                    }
                    Spacer(); Button("移除") { Task { await store.unlinkApp(app) } }
                }
            }
        }.padding(20).frame(width: 480, height: 430)
    }
}
