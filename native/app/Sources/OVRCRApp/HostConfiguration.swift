import Foundation

struct HostConfiguration {
    let environment: [String]
    let dashboardExecutable: URL
    let currentDirectory: String

    static func load(environment: [String: String], hostExecutable: URL) throws -> Self {
        let manager = FileManager.default
        let bin = hostExecutable.deletingLastPathComponent()
        let cli = bin.appendingPathComponent("ovrcr")
        guard manager.isExecutableFile(atPath: cli.path) else {
            throw NSError(domain: NSCocoaErrorDomain, code: NSFileNoSuchFileError,
                          userInfo: [NSLocalizedDescriptionKey: "The bundled Dashboard is missing at \(cli.path). Reinstall OVRCR.app."])
        }

        let home = environment["HOME"] ?? manager.homeDirectoryForCurrentUser.path
        var env = environment
        env["HOME"] = home
        env["TERM"] = "xterm-256color"
        env.removeValue(forKey: "NO_COLOR")  // The native terminal renders the Dashboard’s selected theme.
        env["COLORTERM"] = "truecolor"
        env["TERM_PROGRAM"] = "OVRCR"
        env.removeValue(forKey: "ITERM_SESSION_ID")

        // Finder starts with a minimal PATH. Keep shell-provided tool paths in order.
        var paths = [bin.path]
        for directory in (environment["PATH"] ?? "").components(separatedBy: ":")
            + [home + "/.local/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
            if directory.hasPrefix("/") && !paths.contains(directory) { paths.append(directory) }
        }
        env["PATH"] = paths.joined(separator: ":")

        // A managed Server checks the path registered with launchd as well as its build.
        if env["OVRCR_SERVER_EXECUTABLE"] == nil {
            let installed = home + "/.local/bin/ovrcr"
            env["OVRCR_SERVER_EXECUTABLE"] = manager.isExecutableFile(atPath: installed)
                && manager.contentsEqual(atPath: installed, andPath: cli.path) ? installed : cli.path
        }
        return Self(environment: env.map { "\($0.key)=\($0.value)" }.sorted(),
                    dashboardExecutable: cli, currentDirectory: home)
    }
}
