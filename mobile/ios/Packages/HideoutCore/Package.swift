// swift-tools-version:6.2
import PackageDescription

// Foundation-only app core: models mapped from the protos, the AppStore + Reducer (a port of desktop's
// AppState + apply.rs), GatewayStream, the Connect ApiClient, Keychain TokenStore, file Cache and the
// in-memory FakeBackend. No UIKit or SwiftUI: `swift test` runs on macOS.
let package = Package(
    name: "HideoutCore",
    platforms: [.iOS("26.0"), .macOS("26.0")],
    products: [
        .library(name: "HideoutCore", targets: ["HideoutCore"]),
    ],
    dependencies: [
        .package(path: "../HideoutAPI"),
        .package(url: "https://github.com/connectrpc/connect-swift", exact: "1.2.3"),
        .package(url: "https://github.com/apple/swift-protobuf", exact: "1.38.0"),
    ],
    targets: [
        .target(
            name: "HideoutCore",
            dependencies: [
                "HideoutAPI",
                .product(name: "Connect", package: "connect-swift"),
                .product(name: "SwiftProtobuf", package: "swift-protobuf"),
            ],
            swiftSettings: [.treatAllWarnings(as: .error)]
        ),
        .testTarget(
            name: "HideoutCoreTests",
            dependencies: [
                "HideoutCore",
                "HideoutAPI",
                .product(name: "Connect", package: "connect-swift"),
                .product(name: "SwiftProtobuf", package: "swift-protobuf"),
            ],
            swiftSettings: [.treatAllWarnings(as: .error)]
        ),
    ]
)
