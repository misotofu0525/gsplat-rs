// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "GsplatKit",
    platforms: [
        .iOS(.v17),
    ],
    products: [
        .library(name: "GsplatKit", targets: ["GsplatKit"]),
    ],
    targets: [
        .binaryTarget(
            name: "GsplatFFI",
            path: "Binaries/GsplatFFI.xcframework"
        ),
        .target(
            name: "GsplatKit",
            dependencies: ["GsplatFFI"],
            path: "Sources/GsplatKit"
        ),
    ]
)
