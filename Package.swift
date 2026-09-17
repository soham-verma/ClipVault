// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "ClipVault",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "ClipVault",
            path: "Sources/ClipVault",
            linkerSettings: [
                .linkedLibrary("sqlite3"),
                .linkedFramework("Carbon"),
                .linkedFramework("AppKit"),
                .linkedFramework("ServiceManagement")
            ]
        )
    ]
)
