// swift-tools-version: 5.9
import Foundation
import PackageDescription

let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let repoRoot = packageDir.deletingLastPathComponent().deletingLastPathComponent().path
let rustLibDir = ProcessInfo.processInfo.environment["CARGO_TARGET_DIR"].map { "\($0)/debug" } ?? "\(repoRoot)/target/debug"

let commonLinkerSettings: [LinkerSetting] = [
    .unsafeFlags(["-L\(rustLibDir)", "-lmi_ffi"]),
    .linkedFramework("AppKit"),
    .linkedFramework("SwiftUI"),
    .linkedFramework("IOKit"),
    .linkedFramework("CoreFoundation"),
    .linkedFramework("CoreGraphics"),
    .linkedFramework("Security"),
]

let package = Package(
    name: "MicroInput",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "MicroInputUI", targets: ["MicroInputUI"]),
        .executable(name: "MicroInput", targets: ["MicroInput"]),
    ],
    targets: [
        .systemLibrary(
            name: "MicroInputCoreFFI",
            path: "Sources/MicroInputCoreFFI"
        ),
        .target(
            name: "MicroInputUI",
            dependencies: ["MicroInputCoreFFI"],
            path: "Sources/MicroInputUI",
            swiftSettings: [.unsafeFlags(["-O"])],
            linkerSettings: commonLinkerSettings
        ),
        .executableTarget(
            name: "MicroInput",
            dependencies: ["MicroInputUI", "MicroInputCoreFFI"],
            path: "Sources/MicroInput",
            swiftSettings: [.unsafeFlags(["-O"])],
            linkerSettings: commonLinkerSettings
        ),
        .testTarget(
            name: "MicroInputTests",
            dependencies: ["MicroInputUI", "MicroInputCoreFFI"],
            path: "Tests/MicroInputTests",
            linkerSettings: commonLinkerSettings
        ),
    ]
)
