import Foundation

// One explicit authorization task. Notification delivery and ordinary clicks
// never call begin; polling only reads this result and cannot request consent.
final class ITermSetup {
    private let lock = NSLock()
    private var target: BridgeActivationTarget?
    private var result: BridgeStatus = .itermUnavailable
    private var busy = false

    func begin(_ requested: BridgeActivationTarget,
               admission: @escaping () -> Bool,
               current: @escaping (BridgeActivationTarget) -> Bool,
               authorize: @escaping (BridgeActivationTarget, @escaping () -> Bool) -> ITermFocusResult) -> BridgeStatus {
        guard admission(), requested.iterm_focus, let identity = requested.iterm_session_id,
              ITermFocus.sessionGUID(identity) != nil, current(requested), admission() else {
            return .itermUnavailable
        }
        lock.lock()
        guard !busy else {
            let status: BridgeStatus = target == requested ? .itermPending : .itermUnavailable
            lock.unlock()
            return status
        }
        target = requested
        result = .itermPending
        busy = true
        lock.unlock()
        DispatchQueue.global(qos: .userInitiated).async {
            // The OS may wait for an answer. There is at most one such worker;
            // expiration retires its result, and never starts a second prompt.
            let deadline = Deadline(seconds: 120)
            let valid = { deadline.remaining > 0 && current(requested) }
            let outcome = admission() && valid() && admission() ? authorize(requested, valid) : .ownerChanged
            let status = valid() ? itermBridgeStatus(outcome) : .itermUnavailable
            self.lock.lock()
            self.result = status
            self.busy = false
            self.lock.unlock()
        }
        return .itermPending
    }

    func status(_ requested: BridgeActivationTarget,
                current: (BridgeActivationTarget) -> Bool) -> BridgeStatus {
        guard requested.iterm_focus, current(requested) else { return .itermUnavailable }
        lock.lock(); defer { lock.unlock() }
        guard target == requested else { return .itermUnavailable }
        return result
    }
}

func itermBridgeStatus(_ result: ITermFocusResult) -> BridgeStatus {
    switch result {
    case .authorized, .selectionRequested: return .itermAuthorized
    case .authorizationRequired: return .itermAuthorizationRequired
    case .denied: return .itermDenied
    default: return .itermUnavailable
    }
}
