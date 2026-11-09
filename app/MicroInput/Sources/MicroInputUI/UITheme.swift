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

    public static let accent = adaptive(light: (54, 82, 109), dark: (106, 147, 184))
    public static let accentSoft = accent.opacity(0.11)
    public static let accentBorder = accent.opacity(0.38)

    public static let pageBackground = adaptive(light: (242, 239, 233), dark: (24, 23, 22))
    public static let surface = adaptive(light: (250, 248, 245), dark: (32, 30, 28))
    public static let subtleSurface = adaptive(light: (243, 239, 234), dark: (40, 37, 34))
    public static let mutedSurface = adaptive(light: (234, 228, 220), dark: (50, 46, 42))
    public static let border = adaptive(light: (228, 222, 213), dark: (58, 54, 49))
    public static let strongBorder = adaptive(light: (210, 201, 189), dark: (76, 70, 63))

    public static let textPrimary = adaptive(light: (36, 34, 32), dark: (237, 232, 225))
    public static let textSecondary = adaptive(light: (110, 104, 97), dark: (168, 160, 150))
    public static let textMuted = adaptive(light: (148, 140, 130), dark: (125, 118, 109))

    public static let caution = adaptive(light: (140, 74, 62), dark: (201, 122, 107))
    public static let cautionSoft = caution.opacity(0.10)

    public static let title = Typography.title
    public static let heading = Typography.heading
    public static let body = Typography.body
    public static let bodyMedium = Typography.bodyMedium
    public static let bodySemibold = Typography.bodySemibold
    public static let caption = Typography.caption
    public static let captionMedium = Typography.captionMedium
    public static let micro = Font.system(size: 10.5, weight: .medium)
    public static let monoBadge = Typography.mono
    public static let cardCornerRadius: CGFloat = Radius.panelOuter
    public static let controlCornerRadius: CGFloat = Radius.itemInner

    public enum Typography {
        public static let title = Font.system(size: 20, weight: .semibold)
        public static let heading = Font.system(size: 15, weight: .semibold)
        public static let body = Font.system(size: 13, weight: .regular)
        public static let bodyMedium = Font.system(size: 13, weight: .medium)
        public static let bodySemibold = Font.system(size: 13, weight: .semibold)
        public static let caption = Font.system(size: 11, weight: .regular)
        public static let captionMedium = Font.system(size: 11, weight: .medium)
        public static let mono = Font.system(size: 11, weight: .regular, design: .monospaced)
    }

    public enum Radius {
        public static let chassisOuter: CGFloat = 20
        public static let chassisPadding: CGFloat = 12
        public static let keycapInner: CGFloat = 8

        public static let panelOuter: CGFloat = 14
        public static let panelPadding: CGFloat = 6
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
            return "Deepseek Harness"
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
