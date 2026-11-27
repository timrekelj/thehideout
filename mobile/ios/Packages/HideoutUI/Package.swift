// swift-tools-version:6.2
import PackageDescription

// SwiftUI screens, components and theme injection. The app target is thin; everything visible lives here so
// previews, snapshot tests and extensions can use it.
let package = Package(
    name: "HideoutUI",
    platforms: [.iOS("26.0")],
    products: [
        .library(name: "HideoutUI", targets: ["HideoutUI"]),
    ],
    dependencies: [
        .package(path: "../HideoutDesign"),
        .package(path: "../HideoutCore"),
    ],
    targets: [
        .target(
            name: "HideoutUI",
            dependencies: ["HideoutDesign", "HideoutCore"],
            resources: [.copy("Resources/FakeMedia")],
            swiftSettings: [.treatAllWarnings(as: .error)]
        ),
    ]
)
