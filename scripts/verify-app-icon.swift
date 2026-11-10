import AppKit

// Compare the packaged artwork frame with a system app, rather than the PNG canvas.
guard CommandLine.arguments.count == 2 else {
    fputs("Usage: swift scripts/verify-app-icon.swift <AppIcon.iconset>\n", stderr)
    exit(2)
}
let iconset = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
let reference = NSWorkspace.shared.icon(forFile: "/System/Applications/Mail.app")

func opaqueBounds(_ image: NSImage, pixels: Int) -> [Int] {
    let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    )!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels),
               from: .zero, operation: .copy, fraction: 1)
    NSGraphicsContext.restoreGraphicsState()
    var left = pixels, top = pixels, right = -1, bottom = -1
    for y in 0..<pixels {
        for x in 0..<pixels where bitmap.colorAt(x: x, y: y)!.alphaComponent >= 0.5 {
            left = min(left, x)
            top = min(top, y)
            right = max(right, x)
            bottom = max(bottom, y)
        }
    }
    return [left, top, right + 1, bottom + 1]
}

let systemBounds = opaqueBounds(reference, pixels: 256)
guard systemBounds[2] > systemBounds[0], systemBounds[3] > systemBounds[1] else {
    fputs("FAIL: could not render the system reference icon\n", stderr)
    exit(1)
}
var failures = 0
for (filename, pixels) in [
    ("icon_128x128.png", 128), ("icon_256x256.png", 256),
    ("icon_512x512.png", 512), ("icon_512x512@2x.png", 1024)
] {
    guard let image = NSImage(contentsOf: iconset.appendingPathComponent(filename)) else {
        fputs("FAIL: missing or unreadable \(filename)\n", stderr)
        exit(1)
    }
    let actual = opaqueBounds(image, pixels: pixels)
    let expected = systemBounds.map { Int((Double($0) * Double(pixels) / 256).rounded()) }
    if zip(actual, expected).contains(where: { abs($0 - $1) > 1 }) {
        fputs("FAIL: \(filename) opaque bounds \(actual), system \(expected)\n", stderr)
        failures += 1
    }
}
guard failures == 0 else { exit(1) }
print("App icon artwork frame matches the system at 128, 256, 512 and 1024 px: PASS")
