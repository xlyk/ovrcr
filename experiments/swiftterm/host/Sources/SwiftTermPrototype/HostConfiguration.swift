import Foundation

enum HostLaunchError: LocalizedError, Equatable {
    case missingEnvironment(String)
    case invalidEnvironmentPath(String)
    case missingHome(String)
    case missingFixtureMarker(String)
    case unexpectedFixtureSocket(String, expected: String)
    case missingFixtureSocket(String)
    case missingDashboardExecutable(String)
    case missingWorkingDirectory(String)

    var errorDescription: String? {
        switch self {
        case .missingEnvironment(let key):
            "The launcher must set \(key)."
        case .invalidEnvironmentPath(let key):
            "The launcher's \(key) must be an absolute path."
        case .missingHome(let path):
            "The private HOME directory is missing: \(path)"
        case .missingFixtureMarker(let path):
            "This app only attaches to a runner-owned prototype fixture. Missing marker: \(path)"
        case .unexpectedFixtureSocket(let path, let expected):
            "The fixture socket must be \(expected), not \(path)."
        case .missingFixtureSocket(let path):
            "The prototype fixture socket is missing: \(path)"
        case .missingDashboardExecutable(let path):
            "The bundled ovrcr executable is missing or not executable: \(path)"
        case .missingWorkingDirectory(let path):
            "The launcher's working directory is missing: \(path)"
        }
    }
}

struct HostConfiguration {
    let environment: [String]
    let dashboardExecutable: URL
    let currentDirectory: String

    static func load(
        environment: [String: String],
        hostExecutable: URL,
        currentDirectory: String,
        fileManager: FileManager = .default
    ) throws -> HostConfiguration {
        func path(for key: String) throws -> String {
            guard let value = environment[key], !value.isEmpty else {
                throw HostLaunchError.missingEnvironment(key)
            }
            guard value.hasPrefix("/") else {
                throw HostLaunchError.invalidEnvironmentPath(key)
            }
            return value
        }

        let home = try path(for: "HOME")
        let root = try path(for: "OVRCR_HOME")
        let socket = try path(for: "OVRCR_SOCKET")
        guard currentDirectory.hasPrefix("/") else {
            throw HostLaunchError.missingWorkingDirectory(currentDirectory)
        }

        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: home, isDirectory: &isDirectory), isDirectory.boolValue else {
            throw HostLaunchError.missingHome(home)
        }

        let marker = URL(fileURLWithPath: root, isDirectory: true)
            .appendingPathComponent(".swiftterm-prototype")
        guard let markerAttributes = try? fileManager.attributesOfItem(atPath: marker.path),
              markerAttributes[.type] as? FileAttributeType == .typeRegular else {
            throw HostLaunchError.missingFixtureMarker(marker.path)
        }

        let expectedSocket = URL(fileURLWithPath: root, isDirectory: true)
            .appendingPathComponent("server.sock").standardizedFileURL.path
        guard URL(fileURLWithPath: socket).standardizedFileURL.path == expectedSocket else {
            throw HostLaunchError.unexpectedFixtureSocket(socket, expected: expectedSocket)
        }
        guard let socketAttributes = try? fileManager.attributesOfItem(atPath: socket),
              socketAttributes[.type] as? FileAttributeType == .typeSocket else {
            throw HostLaunchError.missingFixtureSocket(socket)
        }

        let dashboard = hostExecutable.deletingLastPathComponent().appendingPathComponent("ovrcr")
        guard dashboard.isFileURL, fileManager.isExecutableFile(atPath: dashboard.path) else {
            throw HostLaunchError.missingDashboardExecutable(dashboard.path)
        }

        var workingDirectoryIsDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: currentDirectory, isDirectory: &workingDirectoryIsDirectory),
              workingDirectoryIsDirectory.boolValue else {
            throw HostLaunchError.missingWorkingDirectory(currentDirectory)
        }

        var childEnvironment = environment
        childEnvironment["OVRCR_SERVER_EXECUTABLE"] = dashboard.path

        return HostConfiguration(
            environment: childEnvironment.map { "\($0.key)=\($0.value)" }.sorted(),
            dashboardExecutable: dashboard,
            currentDirectory: currentDirectory
        )
    }
}
