// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "SwiftTermPrototype",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "SwiftTermPrototype", targets: ["SwiftTermPrototype"]),
    ],
    dependencies: [
        .package(
            url: "https://github.com/migueldeicaza/SwiftTerm.git",
            revision: "d4f52faaae413b2e69c728ad79cd225cdbebc042"
        ),
    ],
    targets: [
        .executableTarget(
            name: "SwiftTermPrototype",
            dependencies: [.product(name: "SwiftTerm", package: "swiftterm")]
        ),
    ]
)
