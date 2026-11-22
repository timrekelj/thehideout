// swift-tools-version:6.0
import PackageDescription

// The generated Connect clients and protobuf messages for thehideout's API, and nothing else.
// Sources/HideoutAPI/Generated is written by mobile/api/generate.sh; do not edit it.
// Versions are pinned exactly: they must match the plugin versions in mobile/api/buf.gen.yaml
// (connectrpc/swift v1.2.3, apple/swift v1.38.0).
let package = Package(
    name: "HideoutAPI",
    platforms: [.iOS("26.0"), .macOS("26.0")],
    products: [
        .library(name: "HideoutAPI", targets: ["HideoutAPI"]),
    ],
    dependencies: [
        .package(url: "https://github.com/connectrpc/connect-swift", exact: "1.2.3"),
        .package(url: "https://github.com/apple/swift-protobuf", exact: "1.38.0"),
    ],
    targets: [
        .target(
            name: "HideoutAPI",
            dependencies: [
                .product(name: "Connect", package: "connect-swift"),
                .product(name: "SwiftProtobuf", package: "swift-protobuf"),
            ],
        ),
    ]
)
