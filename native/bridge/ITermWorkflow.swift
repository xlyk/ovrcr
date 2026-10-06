import Foundation

struct BridgeOwnerContext: Codable, Equatable {
    let server_socket: String
    let callback_executable: String
    let callback_executable_sha256: String
    let server_lifetime: String
    var valid: Bool {
        navigationPath(server_socket) && navigationPath(callback_executable)
            && canonicalNavigationUUID(server_lifetime)
            && callback_executable_sha256.utf8.count == 64
            && callback_executable_sha256.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
}
struct BridgeOwnerTicket: Codable, Equatable {
    let schema: UInt32
    let server_wire: UInt32
    let context: BridgeOwnerContext
    let dashboard_owner: String
    var valid: Bool {
        schema == bridgeSchema && server_wire == bridgeServerWire
            && context.valid && canonicalNavigationUUID(dashboard_owner)
    }
}
struct BridgeOwnerCall: Encodable {
    let owner: BridgeOwnerTicket
    let outcome: String?
}
struct BridgeOwnerResult: Decodable {
    let schema: UInt32
    let server_wire: UInt32
    let target: BridgeActivationTarget?
}

// Same sealed helper/argv authority as notification navigation. No live-server
// discovery, connect-or-start, environment lookup or alternate transport.
func currentBridgeOwner(_ target: BridgeActivationTarget, deadline: Deadline,
                        outcome: String? = nil) -> BridgeActivationTarget? {
    guard let owner = target.owner, owner.valid,
          let input = try? JSONEncoder().encode(BridgeOwnerCall(owner: owner, outcome: outcome)),
          let bytes = invokeBridgeHelper(input, context: owner.context, command: "owner", deadline: deadline),
          let result = try? JSONDecoder().decode(BridgeOwnerResult.self, from: bytes),
          result.schema == bridgeSchema, result.server_wire == bridgeServerWire,
          let current = result.target, sameDashboardOwner(target, current) else { return nil }
    return current
}

func sameDashboardOwner(_ first: BridgeActivationTarget, _ second: BridgeActivationTarget) -> Bool {
    first.owner != nil && first.owner == second.owner
        && first.dashboard_pid == second.dashboard_pid
        && first.dashboard_start_seconds == second.dashboard_start_seconds
        && first.dashboard_start_microseconds == second.dashboard_start_microseconds
        && first.iterm_session_id == second.iterm_session_id
}

func itermFocusTarget(_ target: BridgeActivationTarget) -> ITermFocusTarget? {
    guard target.dashboard_pid > 1, target.dashboard_pid <= UInt32(Int32.max),
          let identity = target.iterm_session_id, ITermFocus.sessionGUID(identity) != nil else { return nil }
    return ITermFocusTarget(dashboardPID: Int32(target.dashboard_pid),
        dashboardStartSeconds: target.dashboard_start_seconds,
        dashboardStartMicroseconds: target.dashboard_start_microseconds,
        itermSessionID: identity)
}

func requestITermAuthorization(_ target: BridgeActivationTarget,
                              ownerIsCurrent: @escaping () -> Bool,
                              admission: () -> Bool) -> ITermFocusResult {
    guard target.iterm_focus, let native = itermFocusTarget(target) else { return .notEnabled }
    return ITermFocus.requestAuthorization(for: native, ownerIsCurrent: ownerIsCurrent, admission: admission)
}

// Runs off the main thread. Ordinary click never calls requestAuthorization.
// Returns only a freshly validated current-owner destination for safe parent
// activation when exact-session control is unavailable. Navigation stays applied.
func focusITermOrParent(_ target: BridgeActivationTarget, deadline: Deadline,
                       current: @escaping (BridgeActivationTarget, Deadline, String?) -> BridgeActivationTarget? = currentBridgeOwner,
                       select: (ITermFocusTarget, Bool, @escaping () -> Bool) -> ITermFocusResult = {
                           ITermFocus.selectExistingSession(for: $0, explicitlyEnabled: $1, ownerIsCurrent: $2)
                       }) -> BridgeActivationTarget? {
    guard !Thread.isMainThread, deadline.remaining > 0,
          let accepted = current(target, deadline, nil) else { return nil }
    if accepted.iterm_focus {
        let guardCurrent = { deadline.remaining > 0 && current(accepted, deadline, nil)?.iterm_focus == true }
        let result = itermFocusTarget(accepted).map { select($0, true, guardCurrent) } ?? .invalidIdentity
        let outcome: String
        switch result {
        case .selectionRequested: outcome = "selection_requested"
        case .authorized: outcome = "authorized"
        case .denied: outcome = "denied"
        case .authorizationRequired: outcome = "authorization_required"
        default: outcome = "unavailable"
        }
        _ = current(accepted, deadline, outcome)
        if result == .selectionRequested { return nil }
    }
    return deadline.remaining > 0 ? current(accepted, deadline, nil) : nil
}

// The final activation boundary stays on the worker that performs the bounded
// Server check. Never queue an accepted owner snapshot for later activation.
func performCurrentParentActivation(_ target: BridgeActivationTarget, deadline: Deadline,
    current: (BridgeActivationTarget, Deadline, String?) -> BridgeActivationTarget? = currentBridgeOwner,
    activate: (BridgeActivationTarget) -> Bool) -> Bool {
    guard !Thread.isMainThread, deadline.remaining > 0,
          let accepted = current(target, deadline, nil),
          sameDashboardOwner(target, accepted), deadline.remaining > 0 else { return false }
    return activate(accepted)
}
