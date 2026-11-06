import AppKit
import SwiftUI

enum UITheme {
    private static func adaptive(light: (CGFloat, CGFloat, CGFloat), dark: (CGFloat, CGFloat, CGFloat)) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let rgb = isDark ? dark : light
            return NSColor(srgbRed: rgb.0 / 255.0, green: rgb.1 / 255.0, blue: rgb.2 / 255.0, alpha: 1.0)
        })
    }

    // 唯一强调色：中国传统色「黛蓝」（浅色 #36526D，深色提亮至 #6A93B8 保证无障碍对比度）
    static let accent = adaptive(light: (54, 82, 109), dark: (106, 147, 184))
    static let accentSoft = accent.opacity(0.11)
    static let accentBorder = accent.opacity(0.38)

    // 暖灰基底色阶（宣纸 / 亚麻 / 砚黑，不使用冷蓝灰）
    static let pageBackground = adaptive(light: (242, 239, 233), dark: (24, 23, 22))
    static let surface = adaptive(light: (250, 248, 245), dark: (32, 30, 28))
    static let subtleSurface = adaptive(light: (243, 239, 234), dark: (40, 37, 34))
    static let mutedSurface = adaptive(light: (234, 228, 220), dark: (50, 46, 42))
    static let border = adaptive(light: (228, 222, 213), dark: (58, 54, 49))
    static let strongBorder = adaptive(light: (210, 201, 189), dark: (76, 70, 63))

    // 文字色阶（满足 WCAG AA 对比度）
    static let textPrimary = adaptive(light: (36, 34, 32), dark: (237, 232, 225))
    static let textSecondary = adaptive(light: (110, 104, 97), dark: (168, 160, 150))
    static let textMuted = adaptive(light: (148, 140, 130), dark: (125, 118, 109))

    // 克制提示色：传统绛褐（替代刺眼大红与亮橙）
    static let caution = adaptive(light: (140, 74, 62), dark: (201, 122, 107))
    static let cautionSoft = caution.opacity(0.10)

    // 全界面严格控制 4 阶字号：20 / 15 / 13 / 11
    enum Typography {
        static let title = Font.system(size: 20, weight: .semibold)
        static let heading = Font.system(size: 15, weight: .semibold)
        static let body = Font.system(size: 13, weight: .regular)
        static let bodyMedium = Font.system(size: 13, weight: .medium)
        static let bodySemibold = Font.system(size: 13, weight: .semibold)
        static let caption = Font.system(size: 11, weight: .regular)
        static let captionMedium = Font.system(size: 11, weight: .medium)
        // 等宽字体仅用于代码、路径、SHA 校验指标
        static let mono = Font.system(size: 11, weight: .regular, design: .monospaced)
    }

    // 同心圆角规范：内圆角 = 外圆角 - 内边距
    enum Radius {
        static let chassisOuter: CGFloat = 20
        static let chassisPadding: CGFloat = 12
        static let keycapInner: CGFloat = 8 // 20 - 12 = 8

        static let panelOuter: CGFloat = 14
        static let panelPadding: CGFloat = 6
        static let itemInner: CGFloat = 8 // 14 - 6 = 8

        static let segmentedOuter: CGFloat = 9
        static let segmentedPadding: CGFloat = 3
        static let segmentedInner: CGFloat = 6 // 9 - 3 = 6
    }
}
