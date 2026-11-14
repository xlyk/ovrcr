// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "OVRCRApp",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "OVRCRApp", targets: ["OVRCRApp"]),
    ],
    dependencies: [
        .package(
            url: "https://github.com/migueldeicaza/SwiftTerm.git",
            revision: "d4f52faaae413b2e69c728ad79cd225cdbebc042"
        ),
    ],
    targets: [
        .executableTarget(
            name: "OVRCRApp",
            dependencies: [.product(name: "SwiftTerm", package: "swiftterm")]
        ),
    ]
)
