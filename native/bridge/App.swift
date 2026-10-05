import AppKit
import UserNotifications

final class BridgeApp: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    private let center = UNUserNotificationCenter.current()
    private let state = NSLock()
    private var authorizationPending = false
    private var operationBusy = false
    var onLaunch: (() -> Void)?

    override init() {
        super.init()
        center.delegate = self // Before application launch finishes.
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        onLaunch?()
        onLaunch = nil
    }

    private func pending() -> Bool {
        state.lock(); defer { state.unlock() }
        return authorizationPending
    }

    private func endOperation() {
        state.lock(); operationBusy = false; state.unlock()
    }

    private func status(_ settings: UNNotificationSettings) -> BridgeStatus {
        if pending() { return .permissionPending }
        switch settings.authorizationStatus {
        case .notDetermined: return .notDetermined
        case .denied: return .denied
        case .authorized, .provisional:
            return settings.alertSetting == .enabled ? .available : .denied
        @unknown default: return .failed
        }
    }

    func handle(_ data: Data, deadline: Deadline) -> Data {
        let request: BridgeRequest
        switch admit(data) {
        case .request(let accepted): request = accepted
        case .rejected(let failure): return BridgeReply(failure).data()
        }
        if request.op.type != .settings && pending() { return BridgeReply(.permissionPending).data() }
        state.lock()
        guard !operationBusy else { state.unlock(); return BridgeReply(.failed).data() }
        operationBusy = true
        state.unlock()
        let reply = BoundedReply(seconds: deadline.remaining)
        if request.op.type == .settings {
            DispatchQueue.main.async {
                let performed = reply.perform {
                    guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.apple.systempreferences") else {
                        reply.finish(.failed); self.endOperation(); return
                    }
                    let config = NSWorkspace.OpenConfiguration()
                    NSWorkspace.shared.openApplication(at: url, configuration: config) { application, error in
                        reply.finish(application != nil && error == nil ? .settingsOpened : .failed)
                        self.endOperation()
                    }
                }
                if !performed { self.endOperation() }
            }
        } else {
            center.getNotificationSettings { settings in
                let current = self.status(settings)
                if request.op.type == .authorize && current == .notDetermined {
                    let performed = reply.perform {
                        self.state.lock(); self.authorizationPending = true; self.state.unlock()
                        self.center.requestAuthorization(options: [.alert, .sound]) { _, _ in
                            self.state.lock(); self.authorizationPending = false; self.state.unlock()
                        }
                    }
                    reply.finish(performed ? .permissionPending : .failed)
                    self.endOperation()
                } else if request.op.type == .deliver && current == .available {
                    let performed = reply.perform {
                        let content = UNMutableNotificationContent()
                        content.title = request.op.title!
                        content.subtitle = request.op.subtitle!
                        content.body = request.op.body!
                        content.sound = nil // #222 is always silent; no sound/click metadata.
                        let notification = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
                        self.center.add(notification) { error in
                            reply.finish(error == nil ? .submitted : .failed)
                            self.endOperation()
                        }
                    }
                    if !performed { self.endOperation() }
                } else {
                    reply.finish(current)
                    self.endOperation()
                }
            }
        }
        return BridgeReply(reply.wait()).data()
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .list])
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void) {
        completionHandler() // Click navigation/terminal activation belong to later slices.
    }
}

func launchBridge(_ deadline: Deadline) -> Bool {
    let url = Bundle.main.bundleURL
    guard url.pathExtension == "app", let bundle = Bundle(url: url),
          bundle.bundleIdentifier == Bundle.main.bundleIdentifier,
          (bundle.object(forInfoDictionaryKey: "OVRCRBridgeSchema") as? NSNumber)?.uint32Value == bridgeSchema,
          (bundle.object(forInfoDictionaryKey: "OVRCRServerWire") as? NSNumber)?.uint32Value == bridgeServerWire else { return false }
    let config = NSWorkspace.OpenConfiguration()
    config.activates = false
    config.hides = true
    // The caller is a transient process from this same bundle. Always launch a
    // separate application; its port admission rejects duplicate native owners.
    config.createsNewApplicationInstance = true
    let result = BoundedReply(seconds: deadline.remaining)
    NSWorkspace.shared.openApplication(at: url, configuration: config) { app, error in
        result.finish(app != nil && error == nil ? .available : .failed)
    }
    while !result.isFinished && deadline.remaining > 0 {
        RunLoop.current.run(until: Date(timeIntervalSinceNow: min(0.01, deadline.remaining)))
    }
    return result.wait() == .available
}
