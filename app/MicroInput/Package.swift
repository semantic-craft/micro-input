// swift-tools-version: 5.9
import PackageDescription
import Foundation
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
let package = Package(
    name: "MicroInput", platforms: [.macOS(.v14)],
    products: [.executable(name: "MicroInput", targets: ["MicroInput"])],
    targets: [
        .systemLibrary(name: "MicroInputCoreFFI"),
        .target(name: "MicroInputCore", dependencies: ["MicroInputCoreFFI"], linkerSettings: [
            .unsafeFlags([root.appendingPathComponent("target/debug/libmi_ffi.a").path]),
            .linkedFramework("IOKit"), .linkedFramework("CoreFoundation"), .linkedFramework("CoreGraphics"), .linkedFramework("Security"), .linkedLibrary("c++")
        ]),
        .executableTarget(name: "MicroInput", dependencies: ["MicroInputCore"]),
        .testTarget(name: "MicroInputTests", dependencies: ["MicroInput", "MicroInputCore"])
    ]
)
