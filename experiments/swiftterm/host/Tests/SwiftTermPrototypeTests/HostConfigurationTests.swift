import Foundation

@main
struct HostConfigurationPolicyCheck {
    static func main() throws {
        let fileManager = FileManager.default
        let root = fileManager.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        let home = root.appendingPathComponent("home", isDirectory: true)
        try fileManager.createDirectory(at: home, withIntermediateDirectories: true)
        defer { try? fileManager.removeItem(at: root) }

        do {
            _ = try HostConfiguration.load(
                environment: [
                    "HOME": home.path,
                    "OVRCR_HOME": root.path,
                    "OVRCR_SOCKET": root.appendingPathComponent("server.sock").path,
                ],
                hostExecutable: root.appendingPathComponent("SwiftTermPrototype"),
                currentDirectory: root.path
            )
            preconditionFailure("Expected a fixture marker refusal")
        } catch let error as HostLaunchError {
            precondition(error == .missingFixtureMarker(root.appendingPathComponent(".swiftterm-prototype").path))
        }

        try Data().write(to: root.appendingPathComponent(".swiftterm-prototype"))
        let wrongSocket = home.appendingPathComponent("other-server.sock").path
        let expectedSocket = root.appendingPathComponent("server.sock").path
        do {
            _ = try HostConfiguration.load(
                environment: [
                    "HOME": home.path,
                    "OVRCR_HOME": root.path,
                    "OVRCR_SOCKET": wrongSocket,
                ],
                hostExecutable: root.appendingPathComponent("SwiftTermPrototype"),
                currentDirectory: root.path
            )
            preconditionFailure("Expected a socket outside the marked fixture root to be refused")
        } catch let error as HostLaunchError {
            precondition(error == .unexpectedFixtureSocket(wrongSocket, expected: expectedSocket))
        }
    }
}
