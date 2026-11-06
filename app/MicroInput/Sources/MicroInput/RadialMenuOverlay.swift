import AppKit
import SwiftUI

@MainActor final class RadialMenuOverlay {
    private var panel: PassiveHostPanel?
    private var globalKeyMonitor: Any?
    private var localKeyMonitor: Any?
    var onEscape: (() -> Void)?

    static func position(size: NSSize, point: NSPoint, screen: NSRect) -> NSRect {
        let width = min(size.width, screen.width), height = min(size.height, screen.height)
        return NSRect(x: min(max(point.x - width / 2, screen.minX), screen.maxX - width),
                      y: min(max(point.y - height / 2, screen.minY), screen.maxY - height), width: width, height: height)
    }
    func render(_ session: RadialMenuSession?) {
        guard let session else { hide(); return }
        if panel == nil {
            let panel = PassiveHostPanel(contentRect: .zero, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
            panel.title = "Micro Input 径向菜单"
            panel.level = .floating; panel.isFloatingPanel = true; panel.hidesOnDeactivate = false
            panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
            panel.isOpaque = false; panel.backgroundColor = .clear; panel.hasShadow = true
            panel.ignoresMouseEvents = true
            let mouse = NSEvent.mouseLocation
            if let screen = NSScreen.screens.first(where: { $0.frame.contains(mouse) }) ?? NSScreen.main {
                panel.setFrame(Self.position(size: NSSize(width: 500, height: 500), point: mouse, screen: screen.visibleFrame), display: true)
            }
            self.panel = panel
            installEscapeMonitors()
        }
        let view = RadialMenuView(session: session)
        if let host = panel?.contentView as? NSHostingView<RadialMenuView> { host.rootView = view }
        else { panel?.contentView = NSHostingView(rootView: view) }
        panel?.orderFrontRegardless()
    }
    @discardableResult
    func handleKeyCode(_ keyCode: UInt16) -> Bool {
        guard keyCode == 53 else { return false }
        onEscape?()
        hide()
        return true
    }
    private func installEscapeMonitors() {
        removeEscapeMonitors()
        globalKeyMonitor = NSEvent.addGlobalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard event.keyCode == 53 else { return }
            Task { @MainActor [weak self] in
                _ = self?.handleKeyCode(53)
            }
        }
        localKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard event.keyCode == 53 else { return event }
            Task { @MainActor [weak self] in
                _ = self?.handleKeyCode(53)
            }
            return nil
        }
    }
    private func removeEscapeMonitors() {
        if let globalKeyMonitor { NSEvent.removeMonitor(globalKeyMonitor); self.globalKeyMonitor = nil }
        if let localKeyMonitor { NSEvent.removeMonitor(localKeyMonitor); self.localKeyMonitor = nil }
    }
    func hide() {
        removeEscapeMonitors()
        panel?.orderOut(nil)
        panel = nil
    }
}
struct RadialMenuView: View {
    let session: RadialMenuSession
    var body: some View {
        GeometryReader { geometry in
            let center = CGPoint(x: geometry.size.width / 2, y: geometry.size.height / 2)
            let radius = min(geometry.size.width, geometry.size.height) * 0.34
            ZStack {
                RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter)
                    .fill(UITheme.surface)
                RoundedRectangle(cornerRadius: UITheme.Radius.chassisOuter)
                    .strokeBorder(UITheme.strongBorder, lineWidth: 1)
                Circle().stroke(UITheme.border, lineWidth: 1)
                    .frame(width: radius * 2, height: radius * 2).position(center)
                VStack(spacing: 6) {
                    Image(systemName: "circle.grid.cross").font(UITheme.Typography.title).foregroundStyle(UITheme.accent)
                    Text(session.routing.profile_name ?? "当前方案").font(UITheme.Typography.heading).foregroundStyle(UITheme.textPrimary).lineLimit(1)
                    Text("外圈0.75回弹确认\n旋钮按下或Esc取消").font(UITheme.Typography.caption).foregroundStyle(UITheme.textSecondary).multilineTextAlignment(.center)
                }.frame(width: geometry.size.width * 0.36).position(center)
                ForEach(Array(session.slots.enumerated()), id: \.offset) { index, slot in
                    let selected = session.selected == index
                    VStack(spacing: 4) {
                        if let sym = slot.symbol, !sym.isEmpty {
                            Image(systemName: sym).font(UITheme.Typography.caption)
                        }
                        Text(slot.label).font(UITheme.Typography.bodySemibold).lineLimit(2)
                        if let reason = slot.unavailableReason { Text(reason).font(UITheme.Typography.caption).lineLimit(2).foregroundStyle(UITheme.textSecondary) }
                    }
                    .frame(width: geometry.size.width * 0.23, height: 62)
                    .foregroundStyle(slot.intent == nil ? UITheme.textSecondary : UITheme.textPrimary)
                    .background(selected ? UITheme.accentSoft : UITheme.subtleSurface, in: RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner))
                    .overlay(RoundedRectangle(cornerRadius: UITheme.Radius.keycapInner).strokeBorder(selected ? UITheme.accent : UITheme.border, lineWidth: selected ? 1.5 : 1))
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel("\(selected ? "已选择，" : "")\(slot.label)\(slot.unavailableReason.map { "，不可用，\($0)" } ?? "")")
                    .position(x: center.x + cos(Double(index) * .pi / 4) * radius,
                              y: center.y + sin(Double(index) * .pi / 4) * radius)
                }
            }
        }
    }
}
