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
        present(
            VStack(alignment: .leading, spacing: 4) {
                Text(name).font(UITheme.Typography.heading).foregroundStyle(UITheme.textPrimary)
                Text(mode).font(UITheme.Typography.caption).foregroundStyle(UITheme.textSecondary)
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(UITheme.surface, in: RoundedRectangle(cornerRadius: UITheme.Radius.panelOuter))
            .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.panelOuter).strokeBorder(UITheme.strongBorder, lineWidth: 1)),
            size: NSSize(width: 280, height: 76),
            acceptsMouse: false
        )
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
            HStack {
                Text("按键速查").font(UITheme.Typography.heading)
                Spacer()
                Button(action: close) { Image(systemName: "xmark.circle.fill").foregroundStyle(UITheme.textSecondary) }
                    .buttonStyle(.plain)
                    .accessibilityLabel("关闭按键速查")
            }
            Text(store.routing?.profile_name ?? "未选择方案").font(UITheme.Typography.title)
            Text("\(store.routingMode)，\(store.hostExecutionEnabled ? "按键控制已启用" : "按键控制已暂停")")
                .font(UITheme.Typography.caption)
                .foregroundStyle(UITheme.textSecondary)
            if let adapter = store.activeDesktopAgentAdapter() {
                TargetAgentStatusCard(store: store, adapter: adapter)
            }
            ScrollView {
                LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], spacing: 8) {
                    ForEach(store.routing?.keys ?? []) { key in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(controlName(key)).font(UITheme.Typography.caption).foregroundStyle(UITheme.textMuted)
                            Text(key.label).font(UITheme.Typography.bodyMedium).lineLimit(2).foregroundStyle(key.available ? UITheme.textPrimary : UITheme.textSecondary)
                        }
                        .frame(maxWidth: .infinity, minHeight: 42, alignment: .leading)
                        .padding(8)
                        .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
                        .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.itemInner).strokeBorder(UITheme.border, lineWidth: 1))
                    }
                }
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(UITheme.surface, in: RoundedRectangle(cornerRadius: UITheme.Radius.panelOuter))
        .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.panelOuter).strokeBorder(UITheme.strongBorder, lineWidth: 1))
    }
    private func controlName(_ key: HostKey) -> String {
        let names = ["AG10":"底部宽键（2U）", "AG12":"底行右键", "AG13":"旋钮左旋", "AG14":"旋钮右旋", "ENC_CLK":"旋钮按下", "AG15":"摇杆（上）", "AG16":"摇杆（左）", "AG17":"摇杆（下）", "AG18":"摇杆（右）"]
        let keys = ["AG00":"左上键", "AG01":"右上键", "AG02":"按键 03", "AG03":"按键 04", "AG04":"按键 05", "AG05":"按键 06", "AG06":"按键 07", "AG07":"按键 08", "AG08":"按键 09", "AG09":"按键 10"]
        return names[key.binding_id] ?? keys[key.binding_id] ?? key.control
    }
}

struct TargetAgentStatusCard: View {
    @ObservedObject var store: Store
    let adapter: String

    var body: some View {
        let phase = store.fourAppLighting.deriveState(for: adapter, in: store)
        let voiceMode = store.fourAppVoice.getVoiceReplyMode(app: adapter, store: store)
        let isRecording = store.fourAppVoice.dictationState.isRecording || store.pushToTalk.isActive
        let confirmMode = store.radialMenu.confirmMode

        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(DesktopAgent.forAdapter(adapter)?.name ?? adapter)
                    .font(UITheme.Typography.bodySemibold)
                Spacer()
                Label(phase.label, systemImage: phase.symbol)
                    .font(UITheme.Typography.captionMedium)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(UITheme.accentSoft, in: RoundedRectangle(cornerRadius: 4))
                    .foregroundStyle(UITheme.accent)
            }

            HStack(spacing: 12) {
                HStack(spacing: 4) {
                    Image(systemName: isRecording ? "waveform.circle.fill" : "mic.fill")
                        .foregroundStyle(isRecording ? UITheme.accent : UITheme.textSecondary)
                    Text(isRecording ? "语音输入中" : (voiceMode.isSupported ? (voiceMode.isEnabled ? "语音播报已开启" : "语音播报已关闭") : "暂不支持语音播报"))
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }

                HStack(spacing: 4) {
                    Image(systemName: confirmMode == .springBack ? "arrow.uturn.backward.circle" : "button.programmable")
                        .foregroundStyle(UITheme.textSecondary)
                    Text(confirmMode == .springBack ? "摇杆回弹确认" : "旋钮按下确认")
                        .font(UITheme.Typography.caption)
                        .foregroundStyle(UITheme.textSecondary)
                }
            }
        }
        .padding(8)
        .background(UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.itemInner))
        .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.itemInner).strokeBorder(UITheme.border, lineWidth: 1))
    }
}

private extension Color {
    init(hexColor: String) {
        let clean = hexColor.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var int: UInt64 = 0
        Scanner(string: clean).scanHexInt64(&int)
        let r, g, b: UInt64
        switch clean.count {
        case 6:
            (r, g, b) = ((int >> 16) & 0xFF, (int >> 8) & 0xFF, int & 0xFF)
        default:
            (r, g, b) = (255, 255, 255)
        }
        self.init(.sRGB, red: Double(r) / 255, green: Double(g) / 255, blue: Double(b) / 255, opacity: 1)
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
