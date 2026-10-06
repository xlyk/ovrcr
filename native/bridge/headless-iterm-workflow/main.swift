// Production click policy with current-owner and selector doubles. No native
// process lookup, Apple Event, permission preflight, notification or activation.
import Foundation

var checks = 0
func check(_ value: Bool, _ label: String) { precondition(value, label); checks += 1 }
final class Proof: @unchecked Sendable {
    private let lock = NSLock()
    private var fallback: BridgeActivationTarget?
    private var calls = 0
    private var reports = [String]()
    func finish(_ target: BridgeActivationTarget?) { lock.lock(); fallback = target; lock.unlock() }
    func selected() { lock.lock(); calls += 1; lock.unlock() }
    func reported(_ text: String) { lock.lock(); reports.append(text); lock.unlock() }
    var snapshot: (BridgeActivationTarget?, Int, [String]) { lock.lock(); defer { lock.unlock() }; return (fallback, calls, reports) }
}
let context = BridgeOwnerContext(server_socket: "/tmp/owned.sock", callback_executable: "/tmp/ovrcr",
    callback_executable_sha256: String(repeating: "0", count: 64), server_lifetime: "12345678-1234-4234-8234-123456789abc")
let owner = BridgeOwnerTicket(schema: bridgeSchema, server_wire: bridgeServerWire,
    context: context, dashboard_owner: "12345678-1234-4234-8234-123456789abd")
func target(_ enabled: Bool, identity: String? = "w0t0p0:12345678-1234-4234-8234-123456789abc") -> BridgeActivationTarget {
    BridgeActivationTarget(dashboard_pid: 12, dashboard_start_seconds: 1,
        dashboard_start_microseconds: 2, iterm_session_id: identity, iterm_focus: enabled, owner: owner)
}
for result in [ITermFocusResult.selectionRequested, .denied, .authorizationRequired, .missingSession, .unavailable, .timedOut] {
    let finished = DispatchSemaphore(value: 0)
    let proof = Proof()
    DispatchQueue.global().async {
        let fallback = focusITermOrParent(target(true), deadline: Deadline(seconds: 2), current: { current, _, outcome in
            if let outcome = outcome { proof.reported(outcome) }
            return current
        }, select: { native, enabled, current in
            precondition(enabled && current())
            precondition(native.itermSessionID == target(true).iterm_session_id)
            proof.selected()
            return result
        })
        proof.finish(fallback)
        finished.signal()
    }
    check(finished.wait(timeout: .now() + 3) == .success, "bounded click policy returned")
    let (fallback, selectionCount, reports) = proof.snapshot
    check(selectionCount == 1, "one no-prompt selector invocation")
    check((fallback == nil) == (result == .selectionRequested), "only accepted exact selection suppresses parent fallback")
    check(reports.count == 1, "one typed outcome; no retries")
}
for (enabled, identity) in [(false, target(false).iterm_session_id), (false, nil), (true, nil)] {
    let finished = DispatchSemaphore(value: 0)
    let proof = Proof()
    DispatchQueue.global().async {
        let fallback = focusITermOrParent(target(enabled, identity: identity), deadline: Deadline(seconds: 2), current: { current, _, _ in current }, select: { _, _, _ in proof.selected(); return .selectionRequested })
        proof.finish(fallback)
        finished.signal()
    }
    check(finished.wait(timeout: .now() + 3) == .success, "missing context remains bounded")
    let (fallback, calls, _) = proof.snapshot
    check(calls == 0 && fallback != nil, "disabled or missing context uses parent fallback without native control")
}
let finished = DispatchSemaphore(value: 0)
let staleProof = Proof()
DispatchQueue.global().async {
    let fallback = focusITermOrParent(target(true), deadline: Deadline(seconds: 2), current: { _, _, _ in nil }, select: { _, _, _ in staleProof.selected(); return .selectionRequested })
    staleProof.finish(fallback)
    finished.signal()
}
check(finished.wait(timeout: .now() + 3) == .success, "replaced owner is bounded")
let (staleFallback, staleCalls, _) = staleProof.snapshot
check(staleCalls == 0 && staleFallback == nil, "replacement does not select or activate old target")
check(sameDashboardOwner(target(true), target(false)), "accepted preference may change within same owner")

final class ActivationOwner: @unchecked Sendable {
    private let lock = NSLock()
    private var value: BridgeActivationTarget
    init(_ target: BridgeActivationTarget) { value = target }
    func replace(_ target: BridgeActivationTarget) { lock.lock(); value = target; lock.unlock() }
    var current: BridgeActivationTarget { lock.lock(); defer { lock.unlock() }; return value }
}
let replacementOwner = BridgeOwnerTicket(schema: bridgeSchema, server_wire: bridgeServerWire,
    context: context, dashboard_owner: "12345678-1234-4234-8234-123456789abe")
let replacement = BridgeActivationTarget(dashboard_pid: 12, dashboard_start_seconds: 1,
    dashboard_start_microseconds: 2, iterm_session_id: target(false).iterm_session_id,
    iterm_focus: false, owner: replacementOwner)
for mode in ["unchanged", "replaced", "expired"] {
    let prepared = DispatchSemaphore(value: 0)
    let resume = DispatchSemaphore(value: 0)
    let done = DispatchSemaphore(value: 0)
    let state = ActivationOwner(target(false))
    let proof = Proof()
    DispatchQueue.global().async {
        let deadline = Deadline(seconds: 2)
        let fallback = focusITermOrParent(target(false), deadline: deadline,
            current: { _, _, _ in state.current }, select: { _, _, _ in
                preconditionFailure("disabled focus must not select")
            })
        precondition(fallback != nil)
        prepared.signal()
        precondition(resume.wait(timeout: .now() + 2) == .success)
        let activationDeadline = mode == "expired" ? Deadline(seconds: 0) : deadline
        let performed = performCurrentParentActivation(fallback!, deadline: activationDeadline,
            current: { _, _, _ in state.current }, activate: { accepted in
                precondition(sameDashboardOwner(target(false), accepted))
                proof.selected()
                return true
            })
        proof.finish(performed ? fallback : nil)
        done.signal()
    }
    check(prepared.wait(timeout: .now() + 3) == .success, "fallback prepared before delayed activation")
    if mode == "replaced" { state.replace(replacement) }
    resume.signal()
    check(done.wait(timeout: .now() + 3) == .success, "delayed final activation boundary is bounded")
    let (activated, calls, _) = proof.snapshot
    check(calls == (mode == "unchanged" ? 1 : 0), "final owner/deadline check gates the activation effect")
    check((activated != nil) == (mode == "unchanged"), "same live PID/birth cannot defeat replaced owner nonce")
}
var mainActivationCalled = false
check(!performCurrentParentActivation(target(false), deadline: Deadline(seconds: 2),
    current: { current, _, _ in current }, activate: { _ in mainActivationCalled = true; return true }),
    "main thread cannot perform a bounded owner lookup or activation")
check(!mainActivationCalled, "main-thread guard admits no activation effect")
print("\(checks) production iTerm click-policy checks passed with doubles; no native control, consent or focus API executed.")
