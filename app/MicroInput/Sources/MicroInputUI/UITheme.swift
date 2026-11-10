import AppKit
import SwiftUI

public enum UITheme {
    private static func adaptive(light: (CGFloat, CGFloat, CGFloat), dark: (CGFloat, CGFloat, CGFloat)) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let rgb = isDark ? dark : light
            return NSColor(srgbRed: rgb.0 / 255.0, green: rgb.1 / 255.0, blue: rgb.2 / 255.0, alpha: 1.0)
        })
    }

    public static let accent = adaptive(light: (98, 87, 130), dark: (181, 169, 212))
    public static let accentSoft = accent.opacity(0.11)
    public static let accentBorder = accent.opacity(0.38)

    public static let pageBackground = adaptive(light: (255, 255, 255), dark: (27, 26, 25))
    public static let surface = adaptive(light: (255, 255, 255), dark: (32, 30, 28))
    public static let subtleSurface = adaptive(light: (248, 247, 244), dark: (35, 33, 31))
    public static let mutedSurface = adaptive(light: (238, 235, 230), dark: (45, 42, 39))
    public static let border = adaptive(light: (226, 223, 218), dark: (60, 56, 52))
    public static let strongBorder = adaptive(light: (205, 200, 192), dark: (83, 77, 70))

    public static let textPrimary = adaptive(light: (41, 39, 35), dark: (239, 235, 229))
    public static let textSecondary = adaptive(light: (105, 100, 93), dark: (183, 176, 166))
    public static let textMuted = adaptive(light: (112, 106, 98), dark: (169, 162, 152))

    public static let caution = textSecondary
    public static let cautionSoft = caution.opacity(0.10)

    public static let title = Typography.title
    public static let heading = Typography.heading
    public static let body = Typography.body
    public static let bodyMedium = Typography.bodyMedium
    public static let bodySemibold = Typography.bodySemibold
    public static let caption = Typography.caption
    public static let captionMedium = Typography.captionMedium
    public static let micro = Typography.captionMedium
    public static let monoBadge = Typography.mono
    public static let cardCornerRadius: CGFloat = Radius.panelOuter
    public static let controlCornerRadius: CGFloat = Radius.itemInner

    public enum Typography {
        public static let title = Font.system(size: 28, weight: .semibold)
        public static let heading = Font.system(size: 20, weight: .semibold)
        public static let body = Font.system(size: 14, weight: .regular)
        public static let bodyMedium = Font.system(size: 14, weight: .medium)
        public static let bodySemibold = Font.system(size: 14, weight: .semibold)
        public static let caption = Font.system(size: 13, weight: .regular)
        public static let captionMedium = Font.system(size: 13, weight: .medium)
        public static let mono = Font.system(size: 13, weight: .regular, design: .monospaced)
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

        public static let panelOuter: CGFloat = 12
        public static let panelPadding: CGFloat = 4
        public static let itemInner: CGFloat = 8

        public static let segmentedOuter: CGFloat = 9
        public static let segmentedPadding: CGFloat = 3
        public static let segmentedInner: CGFloat = 6
    }
}

public enum StatusItemIcon {
    public static func make() -> NSImage {
        let image = NSImage(size: NSSize(width: 20, height: 18), flipped: false) { _ in
            NSColor.black.setStroke()
            NSColor.black.setFill()
            let body = NSBezierPath(
                roundedRect: NSRect(x: 1, y: 2.5, width: 18, height: 13),
                xRadius: 2,
                yRadius: 2
            )
            body.lineWidth = 1.25
            body.stroke()
            for y in [7.0, 10.5] {
                for x in [3.5, 7.0, 10.5] {
                    NSBezierPath(
                        roundedRect: NSRect(x: x, y: y, width: 2, height: 2),
                        xRadius: 0.35,
                        yRadius: 0.35
                    ).fill()
                }
            }
            NSBezierPath(
                roundedRect: NSRect(x: 5, y: 4.5, width: 7.5, height: 1.25),
                xRadius: 0.4,
                yRadius: 0.4
            ).fill()
            let knob = NSBezierPath(ovalIn: NSRect(x: 14, y: 10, width: 3, height: 3))
            knob.lineWidth = 1
            knob.stroke()
            NSBezierPath(
                roundedRect: NSRect(x: 14.5, y: 5, width: 2, height: 3),
                xRadius: 0.4,
                yRadius: 0.4
            ).fill()
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
        if isGeneralFallback { return true }
        return Self.cachedAppURL(for: bundleID) != nil
    }

    @MainActor
    public static func cachedAppURL(for bundleID: String) -> URL? {
        if bundleID == otherAppsWildcardBundleID {
            return nil
        }
        if let existing = appURLCache[bundleID] {
            return existing
        }
        let resolved = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID)
        appURLCache[bundleID] = resolved
        return resolved
    }

    public static let all: [DesktopAgent] = [
        .init(name: "Cindy", bundleID: "com.xd.cindy", symbol: "sparkles"),
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
        case "Cindy · Micro Input":
            return "Cindy"
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
        if bundleID == DesktopAgent.otherAppsWildcardBundleID {
            return nil
        }
        if let existing = iconCache[bundleID] {
            return existing
        }
        let resolved: NSImage? = DesktopAgent.cachedAppURL(for: bundleID)
            .map { NSWorkspace.shared.icon(forFile: $0.path) }
        iconCache[bundleID] = resolved
        return resolved
    }

    public var body: some View {
        Group {
            if let agent = DesktopAgent.named(name),
               let icon = Self.cachedIcon(for: agent.bundleID)
            {
                Image(nsImage: icon)
                    .resizable()
                    .interpolation(.high)
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
    case cindy = "Cindy"
    case codex = "Codex"
    case antigravity = "Antigravity"
    case dsh = "DeepSeek Coder (DSH)"
    case herdr = "Herdr"
    case custom = "Custom"

    public static func from(profileName: String?) -> AgentProfileKind {
        guard let name = profileName else { return .custom }
        if name == "Deepseek Harness" || name == "DeepSeek Harness" || name == "DSH" {
            return .dsh
        }
        return AgentProfileKind(rawValue: name) ?? .custom
    }

    public var supportsLiveSessionSlots: Bool {
        switch self {
        case .cindy, .codex, .antigravity, .dsh:
            return true
        case .herdr, .custom:
            return false
        }
    }

    public var bridgeDescription: String {
        switch self {
        case .cindy:
            return "支持 Cindy 原生控制与本机会话切换"
        case .codex:
            return "支持 Codex 会话切换、模型选择与语音听写"
        case .antigravity:
            return "支持跨工作区会话切换、审批与实时语音"
        case .dsh:
            return "支持实时插话、停止生成与语音回复"
        case .herdr:
            return "终端工作区全动作键模式，直控标签、分栏与面板焦点"
        case .custom:
            return "其他软件与通用快捷键映射（默认底排麦克风键触发苹果 Fn 语音输入）"
        }
    }
}
