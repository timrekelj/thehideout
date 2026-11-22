// swift-tools-version:6.2
import PackageDescription

// The generated design tokens (Theme, ThemeSpec, palette, type, metrics, motion), shared by the app,
// HideoutUI and every extension. The source file is a symlink to ../../../Shared/DesignTokens.generated.swift,
// which mobile/tokens/generate.py writes; do not edit it.
let package = Package(
    name: "HideoutDesign",
    platforms: [.iOS("26.0"), .macOS("26.0")],
    products: [
        .library(name: "HideoutDesign", targets: ["HideoutDesign"]),
    ],
    targets: [
        .target(
            name: "HideoutDesign",
            swiftSettings: [.treatAllWarnings(as: .error)]
        ),
    ]
)
