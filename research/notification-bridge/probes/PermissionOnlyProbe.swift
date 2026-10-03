// Disposable #221 diagnostic. It cannot submit notifications or navigate to a Server.
import AppKit
import UserNotifications
import Darwin

enum Mode: String {
    case status = "--status-only"
    case request = "--request-permission-only"
}

let arguments = CommandLine.arguments
guard arguments.count == 3, let mode = Mode(rawValue: arguments[1]),
      arguments[2].hasPrefix("/") else {
    fputs("Usage: PermissionOnlyProbe (--status-only|--request-permission-only) NEW_ABSOLUTE_EVENT_FILE\n", stderr)
    exit(64)
}
// Never overwrite existing evidence or follow a final-component symlink.
let descriptor = open(arguments[2], O_WRONLY | O_CREAT | O_EXCL | O_APPEND | O_NOFOLLOW, S_IRUSR | S_IWUSR)
guard descriptor >= 0 else {
    perror("create new permission evidence")
    exit(73)
}
let evidence = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)

// Every caller is on the main queue; persistence is independent of launcher stdout.
func emit(_ fields: [String: Any]) {
    var event = fields
    event["pid"] = ProcessInfo.processInfo.processIdentifier
    event["unixMS"] = Int(Date().timeIntervalSince1970 * 1000)
    do {
        var bytes = try JSONSerialization.data(withJSONObject: event, options: [.sortedKeys])
        bytes.append(10)
        try evidence.write(contentsOf: bytes)
    } catch {
        fputs("Cannot persist permission evidence\n", stderr)
        exit(74)
    }
}

func settingsFields(_ settings: UNNotificationSettings) -> [String: Any] {
    ["authorization": settings.authorizationStatus.rawValue,
     "alerts": settings.alertSetting.rawValue, "sound": settings.soundSetting.rawValue]
}

final class Probe: NSObject, NSApplicationDelegate {
    let mode: Mode

    init(mode: Mode) {
        self.mode = mode
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        emit(["event": "launched", "bundle": Bundle.main.bundleIdentifier ?? "missing",
              "activationPolicy": NSApp.activationPolicy().rawValue,
              "uiElement": Bundle.main.object(forInfoDictionaryKey: "LSUIElement") as? Bool ?? false,
              "mode": mode.rawValue])
        let center = UNUserNotificationCenter.current()
        Timer.scheduledTimer(withTimeInterval: mode == .request ? 120 : 10, repeats: false) { _ in
            emit(["event": "deadline"])
            NSApp.terminate(nil)
        }
        center.getNotificationSettings { settings in
            DispatchQueue.main.async {
                var initial = settingsFields(settings)
                initial["event"] = "initial-settings"
                emit(initial)
                if self.mode == .status {
                    NSApp.terminate(nil)
                    return
                }
                // The requested experiment is fresh first-use only; an old identity
                // must not silently turn into a denial/recovery or repeat request.
                guard settings.authorizationStatus == .notDetermined else {
                    emit(["event": "not-fresh-no-request"])
                    NSApp.terminate(nil)
                    return
                }
                Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { _ in
                    center.getNotificationSettings { current in
                        DispatchQueue.main.async {
                            var fields = settingsFields(current)
                            fields["event"] = "periodic-settings"
                            emit(fields)
                        }
                    }
                }
                emit(["event": "request-start", "options": ["alert", "sound"]])
                center.requestAuthorization(options: [.alert, .sound]) { granted, error in
                    DispatchQueue.main.async {
                        var receipt: [String: Any] = ["event": "authorization-completion", "granted": granted]
                        if let error = error as NSError? {
                            receipt["errorDomain"] = error.domain
                            receipt["errorCode"] = error.code
                        }
                        emit(receipt)
                        center.getNotificationSettings { final in
                            DispatchQueue.main.async {
                                var fields = settingsFields(final)
                                fields["event"] = "final-settings"
                                emit(fields)
                                NSApp.terminate(nil)
                            }
                        }
                    }
                }
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        emit(["event": "terminating"])
        do {
            try evidence.synchronize()
        } catch {
            fputs("Cannot flush permission evidence\n", stderr)
            exit(74)
        }
    }
}

let application = NSApplication.shared
let delegate = Probe(mode: mode)
application.setActivationPolicy(.accessory)
application.delegate = delegate
application.run()
