import AppKit

/// A compact keyboard with a rotary control. Black/clear template artwork lets
/// AppKit supply the appearance and selection tint (Apple HIG: The menu bar).
enum StatusItemIcon {
    static func make() -> NSImage {
        let image = NSImage(size: NSSize(width: 20, height: 18), flipped: false) { _ in
            NSColor.black.setStroke()
            NSColor.black.setFill()
            let body = NSBezierPath(roundedRect: NSRect(x: 1, y: 2.5, width: 18, height: 13),
                                    xRadius: 2, yRadius: 2)
            body.lineWidth = 1.25
            body.stroke()
            // Two key rows and a long bottom key make the keyboard legible at 1x.
            for y in [7.0, 10.5] {
                for x in [3.5, 7.0, 10.5] {
                    NSBezierPath(roundedRect: NSRect(x: x, y: y, width: 2, height: 2),
                                 xRadius: 0.35, yRadius: 0.35).fill()
                }
            }
            NSBezierPath(roundedRect: NSRect(x: 5, y: 4.5, width: 7.5, height: 1.25),
                         xRadius: 0.4, yRadius: 0.4).fill()
            let knob = NSBezierPath(ovalIn: NSRect(x: 14, y: 10, width: 3, height: 3))
            knob.lineWidth = 1
            knob.stroke()
            NSBezierPath(roundedRect: NSRect(x: 14.5, y: 5, width: 2, height: 3),
                         xRadius: 0.4, yRadius: 0.4).fill()
            return true
        }
        image.isTemplate = true
        image.accessibilityDescription = "Micro Input 键盘"
        return image
    }
}
