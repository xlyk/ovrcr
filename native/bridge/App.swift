import AppKit
import Darwin
import UserNotifications

final class BridgeApp: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    private let center = UNUserNotificationCenter.current()
    private let state = NSLock()
    private var authorizationPending = false
    private let itermSetup = ITermSetup()
    private var operationBusy = false
    private var navigationBusy = false
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
        // Independent explicit setup: notification denial/pending cannot turn
        // it into a notification authorization operation or disable navigation.
        if let target = request.op.target {
            let current = { (requested: BridgeActivationTarget) in
                currentBridgeOwner(requested, deadline: Deadline(seconds: 1.5))?.iterm_focus == true
            }
            let status: BridgeStatus
            if request.op.type == .itermSetup {
                status = itermSetup.begin(target, admission: { deadline.remaining > 0 }, current: current, authorize: { requested, valid in
                    requestITermAuthorization(requested, ownerIsCurrent: valid, admission: { deadline.remaining > 0 })
                })
            } else {
                status = itermSetup.status(target, current: current)
            }
            return BridgeReply(status).data()
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
                    guard deadline.remaining > 0 else { self.endOperation(); return }
                    let sound = prepareSound(request.op.sound, bundle: Bundle.main.bundleURL,
                        lookupDirectories: soundLookupDirectories())
                    let performed = reply.perform {
                        let content = UNMutableNotificationContent()
                        content.title = request.op.title!
                        content.subtitle = request.op.subtitle!
                        content.body = request.op.body!
                        switch sound.plan {
                        case .silent: content.sound = nil
                        case .systemDefault: content.sound = .default
                        case .named(let file): content.sound = UNNotificationSound(named: UNNotificationSoundName(rawValue: file))
                        }
                        content.userInfo = request.op.navigation!.userInfo
                        let notification = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
                        self.center.add(notification) { error in
                            reply.finish(error == nil ? .submitted : .failed, soundUnavailable: sound.unavailable)
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
        return reply.wait().data()
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        var options: UNNotificationPresentationOptions = [.banner, .list]
        if notification.request.content.sound != nil { options.insert(.sound) }
        completionHandler(options)
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void) {
        guard response.actionIdentifier == UNNotificationDefaultActionIdentifier,
              let ticket = BridgeNavigationTicket.decode(userInfo: response.notification.request.content.userInfo) else {
            completionHandler(); return
        }
        state.lock()
        guard !navigationBusy else { state.unlock(); completionHandler(); return }
        navigationBusy = true
        state.unlock()
        let deadline = Deadline(seconds: 3.8)
        DispatchQueue.global(qos: .userInitiated).async {
            let result = forwardNavigation(ticket, deadline: Deadline(seconds: min(1.8, deadline.remaining)))
            let parent = result.flatMap { result in
                result.applied ? result.activation.flatMap { focusITermOrParent($0, deadline: deadline) } : nil
            }
            if deadline.remaining > 0, let target = parent {
                activateDashboardParent(target, deadline: deadline)
            }
            DispatchQueue.main.async {
                self.state.lock(); self.navigationBusy = false; self.state.unlock()
                completionHandler()
            }
        }
    }
}

// Parent-app fallback only. AppKit activation sends no Apple Event and does
// not launch a terminal, prompt for permission or synthesize terminal input.
func activateDashboardParent(_ target: BridgeActivationTarget, deadline: Deadline) {
    // NSRunningApplication is SDK-declared Sendable/thread safe; activation has
    // no main-actor annotation. Keep owner validation and its effect together
    // off the main thread, with only notification completion queued afterward.
    guard !Thread.isMainThread,
          let ancestry = currentDashboardParentAncestry(target, continuing: { deadline.remaining > 0 }),
          let app = NSRunningApplication(processIdentifier: Int32(ancestry.application.pid)),
          !app.isTerminated, app.bundleIdentifier != bridgeBundleID else { return }
    var current = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    let read = withUnsafeMutablePointer(to: &current) {
        proc_pidinfo(Int32(ancestry.application.pid), PROC_PIDTBSDINFO, 0, $0, size)
    }
    guard read == size, current.pbi_uid == getuid(),
          current.pbi_pid == ancestry.application.pid,
          current.pbi_start_tvsec == ancestry.application.startSeconds,
          current.pbi_start_tvusec == ancestry.application.startMicroseconds,
          currentDashboardHasAncestor(target, ancestor: current, expectedAncestry: ancestry,
              continuing: { deadline.remaining > 0 }), deadline.remaining > 0 else { return }
    _ = performCurrentParentActivation(target, deadline: deadline) { accepted in
        guard !app.isTerminated,
              currentDashboardHasAncestor(accepted, ancestor: current, expectedAncestry: ancestry,
                  continuing: { deadline.remaining > 0 }),
              deadline.remaining > 0 else { return false }
        return app.activate(options: [])
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
    return result.wait().status == .available
}
