import AppKit
import Foundation
import SwiftUI

public enum UITheme {
    private static func adaptive(
        light: (CGFloat, CGFloat, CGFloat),
        dark: (CGFloat, CGFloat, CGFloat),
        lightAlpha: CGFloat = 1.0,
        darkAlpha: CGFloat = 1.0
    ) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let rgb = isDark ? dark : light
            let alpha = isDark ? darkAlpha : lightAlpha
            return NSColor(srgbRed: rgb.0 / 255.0, green: rgb.1 / 255.0, blue: rgb.2 / 255.0, alpha: alpha)
        })
    }

    public static let accent = adaptive(light: (24, 24, 27), dark: (244, 244, 245))
    public static let accentSoft = adaptive(light: (0, 0, 0), dark: (255, 255, 255), lightAlpha: 0.05, darkAlpha: 0.08)
    public static let accentBorder = adaptive(light: (0, 0, 0), dark: (255, 255, 255), lightAlpha: 0.15, darkAlpha: 0.18)

    public static let pageBackground = adaptive(light: (250, 250, 250), dark: (24, 24, 26))
    public static let surface = adaptive(light: (255, 255, 255), dark: (34, 34, 37))
    public static let subtleSurface = adaptive(light: (244, 244, 245), dark: (42, 42, 46))
    public static let mutedSurface = adaptive(light: (234, 234, 236), dark: (50, 50, 54))
    public static let controlHoverFill = adaptive(light: (241, 241, 243), dark: (50, 50, 54))
    public static let controlPrimaryFill = adaptive(light: (23, 23, 23), dark: (244, 244, 245))
    public static let controlPrimaryText = adaptive(light: (255, 255, 255), dark: (20, 20, 22))
    public static let switchOffFill = adaptive(light: (228, 228, 231), dark: (58, 58, 62))
    public static let switchThumbOnFill = adaptive(light: (255, 255, 255), dark: (24, 24, 26))
    public static let switchThumbOffFill = adaptive(light: (255, 255, 255), dark: (244, 244, 245))

    public static let hairlineBorder = adaptive(light: (0, 0, 0), dark: (255, 255, 255), lightAlpha: 0.08, darkAlpha: 0.10)
    public static let border = hairlineBorder
    public static let strongBorder = adaptive(light: (0, 0, 0), dark: (255, 255, 255), lightAlpha: 0.15, darkAlpha: 0.18)
    public static let rowDivider = adaptive(light: (0, 0, 0), dark: (255, 255, 255), lightAlpha: 0.05, darkAlpha: 0.07)

    public static let textPrimary = adaptive(light: (23, 23, 23), dark: (244, 244, 245))
    public static let textSecondary = adaptive(light: (113, 113, 122), dark: (161, 161, 170))
    public static let textMuted = adaptive(light: (142, 142, 147), dark: (142, 142, 152))

    public static let statusSuccess = adaptive(light: (22, 163, 74), dark: (74, 222, 128))
    public static let statusSuccessSoft = statusSuccess.opacity(0.10)

    public static let caution = textSecondary
    public static let cautionSoft = subtleSurface

    public static let title = Typography.title
    public static let heading = Typography.heading
    public static let sectionHeader = Typography.sectionHeader
    public static let body = Typography.body
    public static let bodyMedium = Typography.bodyMedium
    public static let bodySemibold = Typography.bodySemibold
    public static let caption = Typography.caption
    public static let captionMedium = Typography.captionMedium
    public static let micro = Typography.micro
    public static let monoBadge = Typography.mono
    public static let cardCornerRadius: CGFloat = Radius.panelOuter
    public static let controlCornerRadius: CGFloat = Radius.itemInner

    public enum Typography {
        public static let title = Font.system(size: 26, weight: .semibold)
        public static let heading = Font.system(size: 16, weight: .semibold)
        public static let sectionHeader = Font.system(size: 15, weight: .semibold)
        public static let body = Font.system(size: 14, weight: .regular)
        public static let bodyMedium = Font.system(size: 14, weight: .medium)
        public static let bodySemibold = Font.system(size: 14, weight: .semibold)
        public static let caption = Font.system(size: 13, weight: .regular)
        public static let captionMedium = Font.system(size: 13, weight: .medium)
        public static let micro = Font.system(size: 12, weight: .medium)
        public static let mono = Font.system(size: 12, weight: .regular, design: .monospaced)
    }

    public enum Spacing {
        public static let tight: CGFloat = 4
        public static let related: CGFloat = 8
        public static let control: CGFloat = 12
        public static let content: CGFloat = 16
        public static let group: CGFloat = 24
        public static let section: CGFloat = 32
        public static let page: CGFloat = 48
    }

    public enum Layout {
        public static let sidebarWidth: CGFloat = 220
        public static let contentMaxWidth: CGFloat = 880
        public static let pagePadding: CGFloat = Spacing.section
        public static let sectionGap: CGFloat = Spacing.section
        public static let sidebarRowHeight: CGFloat = 36
        public static let sidebarInset: CGFloat = Spacing.control
    }

    public enum Radius {
        public static let chassisOuter: CGFloat = 20
        public static let chassisPadding: CGFloat = 12
        public static let keycapInner: CGFloat = 8
        public static let panelOuter: CGFloat = 16
        public static let panelPadding: CGFloat = 6
        public static let itemInner: CGFloat = 10
        public static let segmentedOuter: CGFloat = 9
        public static let segmentedPadding: CGFloat = 3
        public static let segmentedInner: CGFloat = 6
    }
}

public struct SelectOption<Value: Hashable>: Identifiable, Equatable {
    public var id: Value { value }
    public let value: Value
    public let title: String
    public let subtitle: String?

    public init(value: Value, title: String, subtitle: String? = nil) {
        self.value = value
        self.title = title
        self.subtitle = subtitle
    }
}

public struct DropdownMenuCardView<Value: Hashable>: View {
    public let options: [SelectOption<Value>]
    public let selectedValue: Value
    public let minWidth: CGFloat
    public let hostedInPanel: Bool
    public let onSelect: (Value) -> Void
    @State private var hoveredValue: Value?

    public init(
        options: [SelectOption<Value>],
        selectedValue: Value,
        minWidth: CGFloat = 176,
        hostedInPanel: Bool = false,
        onSelect: @escaping (Value) -> Void
    ) {
        self.options = options
        self.selectedValue = selectedValue
        self.minWidth = minWidth
        self.hostedInPanel = hostedInPanel
        self.onSelect = onSelect
    }

    private var hasSubtitles: Bool { options.contains { $0.subtitle?.isEmpty == false } }
    private var resolvedWidth: CGFloat { hasSubtitles ? 280 : min(max(minWidth, 176), 260) }

    @ViewBuilder
    private var optionList: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(options) { option in
                let isSelected = (option.value == selectedValue)
                let isHovered = (hoveredValue == option.value)
                Button {
                    onSelect(option.value)
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(option.title)
                            .font(.system(
                                size: hasSubtitles ? 14 : 13.5,
                                weight: hasSubtitles ? .medium : (isSelected ? .medium : .regular)
                            ))
                            .foregroundStyle(UITheme.textPrimary)
                            .fixedSize(horizontal: false, vertical: true)
                        if let subtitle = option.subtitle, !subtitle.isEmpty {
                            Text(subtitle)
                                .font(.system(size: 13, weight: .regular))
                                .foregroundStyle(UITheme.textSecondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, hasSubtitles ? 8.5 : 8)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(isHovered ? UITheme.controlHoverFill : (isSelected ? UITheme.accentSoft : Color.clear))
                    )
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .focusEffectDisabled()
                .onHover { inside in
                    hoveredValue = inside ? option.value : (hoveredValue == option.value ? nil : hoveredValue)
                }
            }
        }
    }

    public var body: some View {
        Group {
            if options.count > 8 {
                ScrollView(.vertical, showsIndicators: true) {
                    optionList
                }
                .frame(maxHeight: 320)
            } else {
                optionList
            }
        }
        .padding(6)
        .frame(width: resolvedWidth)
        .background(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .fill(UITheme.surface)
                .shadow(color: Color.black.opacity(0.10), radius: 18, x: 0, y: 8)
        )
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
        .padding(.horizontal, hostedInPanel ? 22 : 0)
        .padding(.top, hostedInPanel ? 12 : 0)
        .padding(.bottom, hostedInPanel ? 28 : 0)
    }
}

private struct TriggerAnchorProbe: NSViewRepresentable {
    @Binding var anchorView: NSView?

    func makeNSView(context: Context) -> NSView {
        let view = NSView(frame: .zero)
        DispatchQueue.main.async { anchorView = view }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        if anchorView !== nsView {
            DispatchQueue.main.async { anchorView = nsView }
        }
    }
}

private final class DropdownMonitorBag {
    var localMouseMonitor: Any?
    var globalMouseMonitor: Any?
    var scrollMonitor: Any?
    var keyMonitor: Any?
    var resignObserver: NSObjectProtocol?
    var closeObserver: NSObjectProtocol?

    func removeAll() {
        if let m = localMouseMonitor { NSEvent.removeMonitor(m); localMouseMonitor = nil }
        if let m = globalMouseMonitor { NSEvent.removeMonitor(m); globalMouseMonitor = nil }
        if let m = scrollMonitor { NSEvent.removeMonitor(m); scrollMonitor = nil }
        if let m = keyMonitor { NSEvent.removeMonitor(m); keyMonitor = nil }
        if let o = resignObserver { NotificationCenter.default.removeObserver(o); resignObserver = nil }
        if let o = closeObserver { NotificationCenter.default.removeObserver(o); closeObserver = nil }
    }

    deinit {
        removeAll()
    }
}

@MainActor
final class DropdownPanelController {
    private static weak var activeController: DropdownPanelController?
    static let horizontalInset: CGFloat = 22
    static let topInset: CGFloat = 12
    static let bottomInset: CGFloat = 28

    private(set) var panel: NSPanel?
    private let monitors = DropdownMonitorBag()
    var onClose: (() -> Void)?
    var isOpen: Bool { panel != nil }

    deinit {
        monitors.removeAll()
        if Thread.isMainThread {
            MainActor.assumeIsolated {
                if let popup = panel {
                    popup.parent?.removeChildWindow(popup)
                    popup.orderOut(nil)
                    panel = nil
                }
                if Self.activeController === self { Self.activeController = nil }
            }
        }
    }

    func present<Content: View>(from anchorView: NSView, content: Content) {
        if Self.activeController !== self { Self.activeController?.close() }
        close()
        guard let parentWindow = anchorView.window else { return }
        Self.activeController = self

        let triggerScreenRect = parentWindow.convertToScreen(anchorView.convert(anchorView.bounds, to: nil))
        let hostingView = NSHostingView(rootView: content)
        hostingView.layoutSubtreeIfNeeded()
        let visible = parentWindow.screen?.visibleFrame ?? NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let rawSize = hostingView.fittingSize
        let maxAllowedHeight = max(120, visible.height - 16 + Self.topInset + Self.bottomInset)
        let panelSize = NSSize(width: rawSize.width, height: min(rawSize.height, maxAllowedHeight))

        var panelX = triggerScreenRect.maxX - (panelSize.width - Self.horizontalInset)
        let minX = visible.minX + 8 - Self.horizontalInset
        let maxX = visible.maxX - 8 - (panelSize.width - Self.horizontalInset)
        if minX <= maxX { panelX = min(max(panelX, minX), maxX) }

        var panelY = triggerScreenRect.minY - 6 - (panelSize.height - Self.topInset)
        if panelY + Self.bottomInset < visible.minY + 8 {
            panelY = triggerScreenRect.maxY + 6 - Self.bottomInset
        }
        let minY = visible.minY + 8 - Self.bottomInset
        let maxY = visible.maxY - 8 - (panelSize.height - Self.topInset)
        if minY <= maxY {
            panelY = min(max(panelY, minY), maxY)
        } else {
            panelY = minY
        }

        let popup = NSPanel(
            contentRect: NSRect(x: panelX, y: panelY, width: panelSize.width, height: panelSize.height),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        popup.isOpaque = false
        popup.backgroundColor = .clear
        popup.hasShadow = false
        popup.level = .popUpMenu
        popup.hidesOnDeactivate = true
        popup.appearance = anchorView.effectiveAppearance
        popup.contentView = hostingView
        parentWindow.addChildWindow(popup, ordered: .above)
        popup.orderFront(nil)
        self.panel = popup

        monitors.localMouseMonitor = NSEvent.addLocalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self, weak anchorView] event in
            guard let self, let popup = self.panel else { return event }
            if event.window === popup { return event }
            if let anchorView, event.window === anchorView.window,
               anchorView.bounds.contains(anchorView.convert(event.locationInWindow, from: nil)) {
                self.close()
                return nil
            }
            self.close()
            return event
        }
        monitors.globalMouseMonitor = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] _ in
            Task { @MainActor in self?.close() }
        }
        monitors.scrollMonitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
            guard let self, let popup = self.panel else { return event }
            if event.window !== popup {
                self.close()
            }
            return event
        }
        monitors.keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, self.panel != nil else { return event }
            if event.keyCode == 53 { self.close(); return nil }
            return event
        }
        monitors.resignObserver = NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: parentWindow, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.close() }
        }
        monitors.closeObserver = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: parentWindow, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.close() }
        }
    }

    func close() {
        monitors.removeAll()
        if let popup = panel {
            popup.parent?.removeChildWindow(popup)
            popup.orderOut(nil)
            panel = nil
            onClose?()
        }
        if Self.activeController === self { Self.activeController = nil }
    }
}

public struct SelectDropdown<Value: Hashable>: View {
    public let options: [SelectOption<Value>]
    public let selection: Value
    public let overrideLabel: String?
    public let minTriggerWidth: CGFloat
    public let onSelect: (Value) -> Void

    @Environment(\.isEnabled) private var isEnabled
    @State private var anchorView: NSView?
    @State private var isHovered = false
    @State private var isOpen = false
    @State private var controller = DropdownPanelController()

    public init(
        options: [SelectOption<Value>],
        selection: Value,
        overrideLabel: String? = nil,
        minTriggerWidth: CGFloat = 0,
        onSelect: @escaping (Value) -> Void
    ) {
        self.options = options
        self.selection = selection
        self.overrideLabel = overrideLabel
        self.minTriggerWidth = minTriggerWidth
        self.onSelect = onSelect
    }

    public init(
        options: [SelectOption<Value>],
        selection: Binding<Value>,
        overrideLabel: String? = nil,
        minTriggerWidth: CGFloat = 0
    ) {
        self.init(options: options, selection: selection.wrappedValue, overrideLabel: overrideLabel, minTriggerWidth: minTriggerWidth) {
            selection.wrappedValue = $0
        }
    }

    private var currentTitle: String {
        if let overrideLabel, !overrideLabel.isEmpty { return overrideLabel }
        return options.first(where: { $0.value == selection })?.title ?? ""
    }

    public var body: some View {
        let minTextWidth = max(0, minTriggerWidth - 46)
        Button(action: toggleMenu) {
            HStack(spacing: 8) {
                Text(currentTitle)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(1)
                    .frame(minWidth: minTextWidth > 0 ? minTextWidth : nil, alignment: .leading)
                Image(systemName: "chevron.down")
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(UITheme.textSecondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .background(Capsule().fill((isHovered || isOpen) ? UITheme.controlHoverFill : UITheme.surface))
            .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
            .background(TriggerAnchorProbe(anchorView: $anchorView))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .opacity(isEnabled ? 1.0 : 0.42)
        .onHover { isHovered = $0 }
        .onDisappear { controller.close() }
    }

    private func toggleMenu() {
        if controller.isOpen {
            controller.close()
            isOpen = false
            return
        }
        guard let anchorView else { return }
        let triggerWidth = max(anchorView.bounds.width, minTriggerWidth)
        controller.onClose = { isOpen = false }
        isOpen = true
        let menuCard = DropdownMenuCardView(options: options, selectedValue: selection, minWidth: triggerWidth, hostedInPanel: true) { chosen in
            onSelect(chosen)
            controller.close()
        }
        controller.present(from: anchorView, content: menuCard)
    }
}

public enum SurfaceButtonVariant {
    case primary, secondary, ghost, icon
}

public struct SurfaceButtonStyle: ButtonStyle {
    public let variant: SurfaceButtonVariant
    public init(_ variant: SurfaceButtonVariant = .secondary) { self.variant = variant }
    public func makeBody(configuration: Configuration) -> some View {
        SurfaceButtonBody(variant: variant, configuration: configuration)
    }
}

private struct SurfaceButtonBody: View {
    let variant: SurfaceButtonVariant
    let configuration: ButtonStyle.Configuration
    @Environment(\.isEnabled) private var isEnabled
    @State private var isHovered = false

    var body: some View {
        Group {
            switch variant {
            case .primary:
                configuration.label
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(UITheme.controlPrimaryText)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
                    .background(Capsule().fill(UITheme.controlPrimaryFill.opacity(configuration.isPressed ? 0.82 : (isHovered ? 0.90 : 1.0))))
            case .secondary:
                configuration.label
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(UITheme.textPrimary)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 6.5)
                    .background(Capsule().fill((configuration.isPressed || isHovered) ? UITheme.controlHoverFill : UITheme.surface))
                    .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
            case .ghost:
                configuration.label
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(isHovered ? UITheme.textPrimary : UITheme.textSecondary)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(Capsule().fill((configuration.isPressed || isHovered) ? UITheme.controlHoverFill : Color.clear))
            case .icon:
                configuration.label
                    .font(.system(size: 12.5, weight: .medium))
                    .foregroundStyle(UITheme.textPrimary)
                    .frame(width: 28, height: 28)
                    .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill((configuration.isPressed || isHovered) ? UITheme.controlHoverFill : UITheme.surface))
                    .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
            }
        }
        .contentShape(Rectangle())
        .opacity(isEnabled ? 1.0 : 0.42)
        .onHover { isHovered = $0 }
    }
}

public extension View {
    func surfaceButton(_ variant: SurfaceButtonVariant = .secondary) -> some View {
        buttonStyle(SurfaceButtonStyle(variant)).focusEffectDisabled()
    }
}

public struct CapsuleMenuLabel: View {
    public let title: String
    public let icon: String?

    public init(_ title: String, icon: String? = nil) {
        self.title = title
        self.icon = icon
    }

    public var body: some View {
        HStack(spacing: 6) {
            if let icon {
                Image(systemName: icon)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(UITheme.textSecondary)
            }
            Text(title)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(UITheme.textPrimary)
                .lineLimit(1)
            Image(systemName: "chevron.down")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(UITheme.textSecondary)
        }
        .padding(.horizontal, 13)
        .padding(.vertical, 6.5)
        .background(Capsule().fill(UITheme.surface))
        .overlay(Capsule().stroke(UITheme.hairlineBorder, lineWidth: 1))
        .contentShape(Rectangle())
    }
}

public struct NeutralSwitchToggleStyle: ToggleStyle {
    public let showsLabel: Bool
    public init(showsLabel: Bool = false) { self.showsLabel = showsLabel }
    public func makeBody(configuration: Configuration) -> some View {
        NeutralSwitchBody(showsLabel: showsLabel, configuration: configuration)
    }
}

private struct NeutralSwitchBody: View {
    let showsLabel: Bool
    let configuration: ToggleStyle.Configuration
    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        Button { configuration.isOn.toggle() } label: {
            HStack(spacing: 8) {
                if showsLabel {
                    configuration.label
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(UITheme.textPrimary)
                }
                ZStack(alignment: configuration.isOn ? .trailing : .leading) {
                    Capsule()
                        .fill(configuration.isOn ? UITheme.controlPrimaryFill : UITheme.switchOffFill)
                        .frame(width: 40, height: 24)
                    Circle()
                        .fill(configuration.isOn ? UITheme.switchThumbOnFill : UITheme.switchThumbOffFill)
                        .frame(width: 20, height: 20)
                        .shadow(color: Color.black.opacity(0.14), radius: 1.5, x: 0, y: 0.5)
                        .padding(2)
                }
                .animation(.easeInOut(duration: 0.15), value: configuration.isOn)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .opacity(isEnabled ? 1.0 : 0.42)
    }
}

public struct NeutralTrackSlider: View {
    @Binding public var value: Double
    public let range: ClosedRange<Double>
    public let trackWidth: CGFloat
    public let showsPercentage: Bool
    @Environment(\.isEnabled) private var isEnabled

    public init(
        value: Binding<Double>,
        in range: ClosedRange<Double> = 0...100,
        trackWidth: CGFloat = 156,
        showsPercentage: Bool = true
    ) {
        self._value = value
        self.range = range
        self.trackWidth = trackWidth
        self.showsPercentage = showsPercentage
    }

    private var normalized: CGFloat {
        let span = max(0.0001, range.upperBound - range.lowerBound)
        let clamped = min(max(value, range.lowerBound), range.upperBound)
        return CGFloat((clamped - range.lowerBound) / span)
    }

    public var body: some View {
        HStack(spacing: 12) {
            GeometryReader { geo in
                let width = max(1, geo.size.width)
                let thumbSize: CGFloat = 18
                let usable = max(0, width - thumbSize)
                let thumbX = usable * normalized

                ZStack(alignment: .leading) {
                    Capsule().fill(UITheme.mutedSurface).frame(height: 3)
                    Capsule().fill(UITheme.controlPrimaryFill).frame(width: thumbX + thumbSize * 0.5, height: 3)
                    Circle().fill(UITheme.controlPrimaryFill).frame(width: thumbSize, height: thumbSize).offset(x: thumbX)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0).onChanged { drag in
                        guard isEnabled else { return }
                        let fraction = min(max((drag.location.x - thumbSize * 0.5) / max(1, usable), 0), 1)
                        value = range.lowerBound + Double(fraction) * (range.upperBound - range.lowerBound)
                    }
                )
            }
            .frame(width: trackWidth, height: 22)

            if showsPercentage {
                Text("\(Int(value.rounded()))%")
                    .font(.system(size: 13, weight: .regular).monospacedDigit())
                    .foregroundStyle(UITheme.textSecondary)
                    .frame(width: 40, alignment: .trailing)
            }
        }
        .opacity(isEnabled ? 1.0 : 0.40)
    }
}

public struct SettingsSectionHeader<Trailing: View>: View {
    public let title: String
    @ViewBuilder public let trailing: () -> Trailing

    public init(_ title: String, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.trailing = trailing
    }

    public var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Text(title).font(UITheme.sectionHeader).foregroundStyle(UITheme.textPrimary)
            Spacer(minLength: 8)
            trailing()
        }
    }
}

public extension SettingsSectionHeader where Trailing == EmptyView {
    init(_ title: String) { self.init(title) { EmptyView() } }
}

public struct SettingsCard<Content: View>: View {
    @ViewBuilder public let content: () -> Content
    public init(@ViewBuilder content: @escaping () -> Content) { self.content = content }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) { content() }
            .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(UITheme.surface))
            .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).stroke(UITheme.hairlineBorder, lineWidth: 1))
    }
}

public struct SettingsRow<Trailing: View>: View {
    public let title: String
    public let subtitle: String?
    public let showsDivider: Bool
    @ViewBuilder public let trailing: () -> Trailing

    public init(_ title: String, subtitle: String? = nil, showsDivider: Bool = false, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.subtitle = subtitle
        self.showsDivider = showsDivider
        self.trailing = trailing
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .center, spacing: 16) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(title)
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(UITheme.textPrimary)
                        .fixedSize(horizontal: false, vertical: true)
                    if let subtitle, !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.system(size: 13, weight: .regular))
                            .foregroundStyle(UITheme.textSecondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 16)
                trailing()
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 14)

            if showsDivider {
                Rectangle().fill(UITheme.rowDivider).frame(height: 1).padding(.horizontal, 20)
            }
        }
    }
}

public extension SettingsRow where Trailing == EmptyView {
    init(_ title: String, subtitle: String? = nil, showsDivider: Bool = false) {
        self.init(title, subtitle: subtitle, showsDivider: showsDivider) { EmptyView() }
    }
}

public struct PopoverActionRow: View {
    public let icon: String
    public let title: String
    public let shortcut: String?
    public let action: () -> Void
    @State private var isHovered = false

    public init(icon: String, title: String, shortcut: String? = nil, action: @escaping () -> Void) {
        self.icon = icon
        self.title = title
        self.shortcut = shortcut
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: icon)
                    .font(.system(size: 13.5, weight: .regular))
                    .foregroundStyle(UITheme.textSecondary)
                    .frame(width: 18, alignment: .center)
                Text(title)
                    .font(.system(size: 13, weight: .regular))
                    .foregroundStyle(UITheme.textPrimary)
                Spacer(minLength: 8)
                if let shortcut, !shortcut.isEmpty {
                    Text(shortcut)
                        .font(.system(size: 12, weight: .regular, design: .monospaced))
                        .foregroundStyle(UITheme.textMuted)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(isHovered ? UITheme.controlHoverFill : Color.clear))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .focusEffectDisabled()
        .onHover { isHovered = $0 }
    }
}

public enum StatusItemIcon {
    public static func make() -> NSImage {
        let image = NSImage(size: NSSize(width: 20, height: 18), flipped: false) { _ in
            NSColor.black.setStroke()
            NSColor.black.setFill()
            let body = NSBezierPath(roundedRect: NSRect(x: 1, y: 2.5, width: 18, height: 13), xRadius: 2, yRadius: 2)
            body.lineWidth = 1.25
            body.stroke()
            for y in [7.0, 10.5] {
                for x in [3.5, 7.0, 10.5] {
                    NSBezierPath(roundedRect: NSRect(x: x, y: y, width: 2, height: 2), xRadius: 0.35, yRadius: 0.35).fill()
                }
            }
            NSBezierPath(roundedRect: NSRect(x: 5, y: 4.5, width: 7.5, height: 1.25), xRadius: 0.4, yRadius: 0.4).fill()
            let knob = NSBezierPath(ovalIn: NSRect(x: 14, y: 10, width: 3, height: 3))
            knob.lineWidth = 1
            knob.stroke()
            NSBezierPath(roundedRect: NSRect(x: 14.5, y: 5, width: 2, height: 3), xRadius: 0.4, yRadius: 0.4).fill()
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = "Micro Input 键盘"
        return image
    }
}

public struct DesktopAgent: Identifiable {
    public static let otherAppsWildcardBundleID = "*"
    @MainActor private static var appURLCache: [String: URL?] = [:]

    public let name: String
    public let bundleID: String
    public let symbol: String
    public var id: String { name }
    public var isGeneralFallback: Bool { bundleID == Self.otherAppsWildcardBundleID }

    @MainActor
    public var installed: Bool {
        isGeneralFallback || Self.cachedAppURL(for: bundleID) != nil
    }

    @MainActor
    public static func cachedAppURL(for bundleID: String) -> URL? {
        if bundleID == otherAppsWildcardBundleID { return nil }
        if let existing = appURLCache[bundleID] { return existing }
        let resolved = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID)
        appURLCache[bundleID] = resolved
        return resolved
    }

    public static let all: [DesktopAgent] = [
        .init(name: "Codex", bundleID: "com.openai.codex", symbol: "terminal"),
        .init(name: "Antigravity", bundleID: "com.google.antigravity", symbol: "sparkles"),
        .init(name: "DeepSeek Coder (DSH)", bundleID: "com.deepseek.dsh", symbol: "terminal"),
        .init(name: "Herdr", bundleID: "dev.herdr.app", symbol: "rectangle.split.2x2"),
        .init(name: "其他软件", bundleID: otherAppsWildcardBundleID, symbol: "keyboard"),
    ]

    public static func displayName(_ name: String) -> String {
        switch name {
        case "DeepSeek Coder (DSH)", "DSH", "DeepSeek Harness":
            return "DeepSeek Harness"
        default:
            return name
        }
    }

    public static func named(_ name: String) -> DesktopAgent? {
        all.first { $0.name == name || displayName($0.name) == displayName(name) }
    }
}

public struct AgentBadge: View {
    @MainActor private static var iconCache: [String: NSImage?] = [:]
    public let name: String
    public var size: CGFloat = 28

    public init(name: String, size: CGFloat = 28) {
        self.name = name
        self.size = size
    }

    @MainActor
    private static func cachedIcon(for bundleID: String) -> NSImage? {
        if bundleID == DesktopAgent.otherAppsWildcardBundleID { return nil }
        if let existing = iconCache[bundleID] { return existing }
        let resolved: NSImage? = DesktopAgent.cachedAppURL(for: bundleID).map { NSWorkspace.shared.icon(forFile: $0.path) }
        iconCache[bundleID] = resolved
        return resolved
    }

    public var body: some View {
        Group {
            if let agent = DesktopAgent.named(name), let icon = Self.cachedIcon(for: agent.bundleID) {
                Image(nsImage: icon).resizable().interpolation(.high)
            } else {
                Image(systemName: DesktopAgent.named(name)?.symbol ?? "square.stack")
                    .resizable()
                    .scaledToFit()
                    .padding(5)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

public enum AgentProfileKind: String {
    case codex = "Codex"
    case antigravity = "Antigravity"
    case dsh = "DeepSeek Coder (DSH)"
    case herdr = "Herdr"
    case custom = "Custom"

    public static func from(profileName: String?) -> AgentProfileKind {
        guard let name = profileName else { return .custom }
        if name == "Codex · Micro Input" || name == "Codex · 原生" { return .codex }
        if name == "Deepseek Harness" || name == "DeepSeek Harness" || name == "DSH" { return .dsh }
        return AgentProfileKind(rawValue: name) ?? .custom
    }

    public var supportsLiveSessionSlots: Bool {
        switch self {
        case .codex, .antigravity, .dsh: return true
        case .herdr, .custom: return false
        }
    }

    public var bridgeDescription: String {
        switch self {
        case .codex: return "支持 Codex 会话切换、模型选择与语音听写"
        case .antigravity: return "支持跨工作区会话切换、审批与实时语音"
        case .dsh: return "支持实时插话、停止生成与语音回复"
        case .herdr: return "终端工作区全动作键模式，直控标签、分栏与面板焦点"
        case .custom: return "其他软件与通用快捷键映射（默认底排麦克风键触发苹果 Fn 语音输入）"
        }
    }
}

public extension Color {
    init(hex: String) {
        let cleaned = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        var val: UInt64 = 0
        Scanner(string: cleaned).scanHexInt64(&val)
        let r, g, b: Double
        if cleaned.count == 6 {
            r = Double((val >> 16) & 0xFF) / 255.0
            g = Double((val >> 8) & 0xFF) / 255.0
            b = Double(val & 0xFF) / 255.0
        } else {
            r = 0.23
            g = 0.51
            b = 0.96
        }
        self.init(red: r, green: g, blue: b)
    }
}
