// The production setup state with permission/current-owner doubles. No app
// lookup, Apple Event, notification API, signing, launch or permission call.
import Foundation

var checks = 0
func check(_ passed: Bool, _ label: String) {
    precondition(passed, label)
    checks += 1
}
let context = BridgeOwnerContext(server_socket: "/tmp/owned.sock", callback_executable: "/tmp/ovrcr",
    callback_executable_sha256: String(repeating: "0", count: 64),
    server_lifetime: "12345678-1234-4234-8234-123456789abc")
let owner = BridgeOwnerTicket(schema: bridgeSchema, server_wire: bridgeServerWire,
    context: context, dashboard_owner: "12345678-1234-4234-8234-123456789abd")
func target(enabled: Bool = true, identity: String? = "w0t0p0:12345678-1234-4234-8234-123456789abc") -> BridgeActivationTarget {
    BridgeActivationTarget(dashboard_pid: 12, dashboard_start_seconds: 1,
        dashboard_start_microseconds: 2, iterm_session_id: identity,
        iterm_focus: enabled, owner: owner)
}
let requested = target()
for disabled in [target(enabled: false), target(identity: nil), target(identity: "bad")] {
    let setup = ITermSetup()
    var calls = 0
    check(setup.begin(disabled, admission: { true }, current: { _ in true }, authorize: { _, _ in calls += 1; return .authorized }) == .itermUnavailable,
          "disabled or missing identity cannot authorize")
    check(calls == 0, "setup rejected before permission entry")
}
let expired = ITermSetup()
var expiredCalls = 0
check(expired.begin(requested, admission: { false }, current: { _ in true }, authorize: { _, _ in expiredCalls += 1; return .authorized }) == .itermUnavailable,
      "expired admission cannot start a late prompt")
check(expiredCalls == 0, "late permission entry not called")
let slowAdmission = ITermSetup()
var admitted = true, lateCalls = 0
check(slowAdmission.begin(requested, admission: { admitted }, current: { _ in admitted = false; return true }, authorize: { _, _ in lateCalls += 1; return .authorized }) == .itermUnavailable,
      "admission is rechecked after current-owner validation")
check(lateCalls == 0, "slow validation cannot start a late worker")
for outcome in [ITermFocusResult.authorized, .denied, .authorizationRequired, .unavailable] {
    let setup = ITermSetup()
    let reached = DispatchSemaphore(value: 0), release = DispatchSemaphore(value: 0)
    check(setup.begin(requested, admission: { true }, current: { _ in true }, authorize: { _, valid in
        precondition(valid(), "current owner supplied to permission adapter")
        reached.signal()
        _ = release.wait(timeout: .now() + 2)
        return outcome
    }) == .itermPending, "explicit setup is nonblocking pending")
    check(reached.wait(timeout: .now() + 2) == .success, "one explicit worker reached")
    for _ in 0..<3 {
        check(setup.status(requested, current: { _ in true }) == .itermPending, "status never authorizes")
        check(setup.begin(requested, admission: { true }, current: { _ in true }, authorize: { _, _ in
            preconditionFailure("pending setup must not initiate another authorization")
        }) == .itermPending, "pending setup does not retry")
    }
    release.signal()
    let deadline = Date().addingTimeInterval(2)
    while setup.status(requested, current: { _ in true }) == .itermPending && Date() < deadline {
        Thread.sleep(forTimeInterval: 0.001)
    }
    check(setup.status(requested, current: { _ in true }) == itermBridgeStatus(outcome), "typed completed outcome")
    check(setup.status(requested, current: { _ in false }) == .itermUnavailable, "replacement owner cannot read old result")
}
check(itermBridgeStatus(.selectionRequested) == .itermAuthorized, "request receipt is typed, not visible-focus proof")
print("\(checks) production iTerm setup-state checks passed with doubles; no native control or permission API executed.")
