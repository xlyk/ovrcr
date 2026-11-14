import Foundation

@main
struct HostConfigurationCheck {
    static func main() throws {
        let manager = FileManager.default
        let home = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try manager.createDirectory(at: home, withIntermediateDirectories: true)
        defer { try? manager.removeItem(at: home) }
        let host = home.appendingPathComponent("OVRCRApp")
        let cli = home.appendingPathComponent("ovrcr")
        let env = ["NO_COLOR": "1", "HOME": home.path, "PATH": "/custom/bin:/bin", "TERM": "dumb",
                   "ITERM_SESSION_ID": "outer", "OVRCR_SOCKET": home.path + "/other.sock"]
        do {
            _ = try HostConfiguration.load(environment: env, hostExecutable: host)
            preconditionFailure("Missing bundled Dashboard was accepted")
        } catch let error as NSError {
            precondition(error.code == NSFileNoSuchFileError)
        }

        try Data([1]).write(to: cli)
        try manager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: cli.path)
        let config = try HostConfiguration.load(environment: env, hostExecutable: host)
        precondition(config.currentDirectory == home.path && config.dashboardExecutable == cli)
        precondition(config.environment.contains("OVRCR_SERVER_EXECUTABLE=" + cli.path))
        precondition(config.environment.contains("OVRCR_SOCKET=" + env["OVRCR_SOCKET"]!))
        precondition(config.environment.contains("TERM=xterm-256color"))
        precondition(!config.environment.contains(where: { $0.hasPrefix("NO_COLOR=") }), "Terminal host must render the Dashboard theme")
        precondition(!config.environment.contains(where: { $0.hasPrefix("ITERM_SESSION_ID=") }))
        precondition(config.environment.contains(where: { $0.hasPrefix("PATH=" + home.path + ":/custom/bin:/bin:") }))

        let installed = home.appendingPathComponent(".local/bin/ovrcr")
        try manager.createDirectory(at: installed.deletingLastPathComponent(), withIntermediateDirectories: true)
        try manager.copyItem(at: cli, to: installed)
        func serverPath(_ environment: [String: String]) throws -> String? {
            try HostConfiguration.load(environment: environment, hostExecutable: host)
                .environment.first { $0.hasPrefix("OVRCR_SERVER_EXECUTABLE=") }
        }
        let matched = try serverPath(env)
        precondition(matched == "OVRCR_SERVER_EXECUTABLE=" + installed.path)
        try Data([2]).write(to: installed)
        let mismatched = try serverPath(env)
        precondition(mismatched == "OVRCR_SERVER_EXECUTABLE=" + cli.path)
        let explicit = try serverPath(env.merging(["OVRCR_SERVER_EXECUTABLE": "/managed/ovrcr"]) { _, new in new })
        precondition(explicit == "OVRCR_SERVER_EXECUTABLE=/managed/ovrcr")
    }
}
