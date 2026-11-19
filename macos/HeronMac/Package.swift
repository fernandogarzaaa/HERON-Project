// swift-tools-version: 5.10
import PackageDescription

// No package dependencies and no resource bundles (heron-v1.md item 40): `Bundle.module`
// can't find SwiftPM resource bundles inside an app bundle's Contents/Resources, so the
// avatar is drawn in SwiftUI code and icons are SF Symbols.
let package = Package(
    name: "HeronMac",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "HeronCore", targets: ["HeronCore"]),
        .library(name: "HeronUI", targets: ["HeronUI"]),
        .executable(name: "Heron", targets: ["Heron"]),
    ],
    targets: [
        .target(name: "HeronCore"),
        .target(name: "HeronUI", dependencies: ["HeronCore"]),
        .executableTarget(name: "Heron", dependencies: ["HeronCore", "HeronUI"]),
        .testTarget(name: "HeronCoreTests", dependencies: ["HeronCore"]),
        .testTarget(
            name: "HeronUITests",
            dependencies: ["HeronUI", "HeronCore"],
            exclude: ["__Snapshots__"]
        ),
    ]
)
