import AppKit

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let source = root.appendingPathComponent("app/MicroInput/Resources/AppIcon-artwork.png")
guard let image = NSImage(contentsOf: source),
      let original = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    fatalError("Cannot read approved artwork")
}
let output = root.appendingPathComponent("target/AppIcon.iconset")
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = points * scale
        let context = CGContext(data: nil, width: pixels, height: pixels, bitsPerComponent: 8,
                                bytesPerRow: pixels * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let side = CGFloat(pixels)
        // Match the legacy macOS 824 px artwork frame on a 1024 px canvas.
        let bounds = CGRect(x: 0, y: 0, width: side, height: side)
            .insetBy(dx: side * 100 / 1024, dy: side * 100 / 1024)
        context.addPath(CGPath(roundedRect: bounds, cornerWidth: bounds.width * 2 / 9,
                              cornerHeight: bounds.height * 2 / 9, transform: nil))
        context.clip()
        context.interpolationQuality = .high
        context.draw(original, in: bounds)
        let bitmap = NSBitmapImageRep(cgImage: context.makeImage()!)
        let suffix = scale == 2 ? "@2x" : ""
        try bitmap.representation(using: .png, properties: [:])!.write(
            to: output.appendingPathComponent("icon_\(points)x\(points)\(suffix).png"))
    }
}
