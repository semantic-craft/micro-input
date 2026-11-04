import AppKit
import SwiftUI

@MainActor final class RadialMenuOverlay {
    private var panel: PassiveHostPanel?
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
        }
        let view = RadialMenuView(session: session)
        if let host = panel?.contentView as? NSHostingView<RadialMenuView> { host.rootView = view }
        else { panel?.contentView = NSHostingView(rootView: view) }
        panel?.orderFrontRegardless()
    }
    func hide() { panel?.orderOut(nil); panel = nil }
}
struct RadialMenuView: View {
    let session: RadialMenuSession
    var body: some View {
        GeometryReader { geometry in
            let center = CGPoint(x: geometry.size.width / 2, y: geometry.size.height / 2)
            let radius = min(geometry.size.width, geometry.size.height) * 0.34
            ZStack {
                RoundedRectangle(cornerRadius: 26).fill(.regularMaterial)
                Circle().stroke(Color.primary.opacity(0.07), lineWidth: 1)
                    .frame(width: radius * 2, height: radius * 2).position(center)
                VStack(spacing: 8) {
                    Image(systemName: "circle.grid.cross").font(.title2).foregroundStyle(Color.accentColor)
                    Text(session.routing.profile_name ?? "当前配置").font(.headline).lineLimit(1)
                    Text("旋钮按下确认\n摇杆回中取消").font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
                }.frame(width: geometry.size.width * 0.36).position(center)
                ForEach(Array(session.slots.enumerated()), id: \.offset) { index, slot in
                    let selected = session.selected == index
                    VStack(spacing: 4) {
                        Text(slot.label).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                        if let reason = slot.unavailableReason { Text(reason).font(.system(size: 10)).lineLimit(2).foregroundStyle(.secondary) }
                    }
                    .frame(width: geometry.size.width * 0.23, height: 62)
                    .foregroundStyle(slot.intent == nil ? Color.secondary : Color.primary)
                    .background(selected ? Color.accentColor.opacity(0.2) : Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(selected ? Color.accentColor : .clear, lineWidth: 2))
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel("\(selected ? "已选择，" : "")\(slot.label)\(slot.unavailableReason.map { "，不可用，\($0)" } ?? "")")
                    .position(x: center.x + cos(Double(index) * .pi / 4) * radius,
                              y: center.y + sin(Double(index) * .pi / 4) * radius)
                }
            }
        }.accessibilityLabel("径向菜单")
    }
}
