import Foundation

// This runner exercises pure parsing and entry guards only. Enabled selection
// is called ONLY on the main thread, where it must stop before Context, process
// lookup, permission preflight, Apple Events or application activation. The setup
// entry is also exercised only through its main-thread early guard. Compile/run only after the coordinator grants the queued slot.
var checks = 0

func check(_ condition: @autoclosure () -> Bool, _ name: String) {
    guard condition() else { fatalError("iTerm headless check failed: \(name)") }
    checks += 1
}

let guid = "01234567-89ab-cdef-0123-456789abcdef"
let otherGUID = "fedcba98-7654-3210-fedc-ba9876543210"
let boundaryIdentity = "w" + String(repeating: "9", count: 86) + "t0p0:" + guid
let oversizedIdentity = "w" + String(repeating: "9", count: 87) + "t0p0:" + guid

check(boundaryIdentity.utf8.count == 128, "identity fixture at byte limit")
check(oversizedIdentity.utf8.count == 129, "identity fixture beyond byte limit")

let accepted: [(name: String, identity: String, expected: String)] = [
    ("launch positions", "w0t0p0:" + guid, guid),
    ("moved positions", "w81t42p7:" + guid, guid),
    ("leading zero positions", "w000t001p002:" + guid, guid),
    ("uppercase GUID", "w0t0p0:" + guid.uppercased(), guid),
    ("mixed case GUID", "w0t0p0:01234567-89Ab-cDeF-0123-456789aBcDeF", guid),
    ("different GUID", "w0t0p0:" + otherGUID, otherGUID),
    ("128-byte identity", boundaryIdentity, guid),
]

for item in accepted {
    check(ITermFocus.sessionGUID(item.identity) == item.expected, item.name)
}

let rejected: [(name: String, identity: String)] = [
    ("empty", ""),
    ("missing colon", "w0t0p0" + guid),
    ("missing positions", ":" + guid),
    ("extra separator", "w0t0p0::" + guid),
    ("separator inside positions", "w0:t0p0:" + guid),
    ("trailing separator", "w0t0p0:" + guid + ":"),
    ("missing window marker", "0t0p0:" + guid),
    ("missing tab marker", "w00p0:" + guid),
    ("missing pane marker", "w0t00:" + guid),
    ("missing window digits", "wt0p0:" + guid),
    ("missing tab digits", "w0tp0:" + guid),
    ("missing pane digits", "w0t0p:" + guid),
    ("negative position", "w-1t0p0:" + guid),
    ("signed position", "w+1t0p0:" + guid),
    ("uppercase marker", "W0t0p0:" + guid),
    ("reordered markers", "w0p0t0:" + guid),
    ("trailing position text", "w0t0p0x:" + guid),
    ("non-ASCII position", "w０t0p0:" + guid),
    ("position whitespace", "w0 t0p0:" + guid),
    ("position newline", "w0\nt0p0:" + guid),
    ("position NUL", "w0t0p0\0:" + guid),
    ("empty GUID", "w0t0p0:"),
    ("compact GUID", "w0t0p0:" + guid.replacingOccurrences(of: "-", with: "")),
    ("braced GUID", "w0t0p0:{" + guid + "}"),
    ("GUID missing byte", "w0t0p0:" + String(guid.dropLast())),
    ("GUID extra byte", "w0t0p0:" + guid + "0"),
    ("GUID wrong hyphen", "w0t0p0:01234567_89ab-cdef-0123-456789abcdef"),
    ("GUID non-hex", "w0t0p0:01234567-89ab-cdef-0123-456789abcdeg"),
    ("GUID non-ASCII hex", "w0t0p0:０1234567-89ab-cdef-0123-456789abcdef"),
    ("GUID leading whitespace", "w0t0p0: " + guid),
    ("GUID trailing newline", "w0t0p0:" + guid + "\n"),
    ("GUID trailing tab", "w0t0p0:" + guid + "\t"),
    ("GUID NUL", "w0t0p0:" + String(guid.dropLast()) + "\0"),
    ("GUID control byte", "w0t0p0:" + String(guid.dropLast()) + "\u{1B}"),
    ("GUID bidi control", "w0t0p0:" + guid + "\u{202E}"),
    ("129-byte identity", oversizedIdentity),
    ("unbounded identity", "w" + String(repeating: "1", count: 4096) + "t0p0:" + guid),
]

for item in rejected {
    check(ITermFocus.sessionGUID(item.identity) == nil, item.name)
}

// A move changes indices; those indices must never become the destination key.
let original = ITermFocus.sessionGUID("w1t2p3:" + guid)
let moved = ITermFocus.sessionGUID("w9t8p7:" + guid.uppercased())
let replaced = ITermFocus.sessionGUID("w1t2p3:" + otherGUID)
check(original == guid && moved == original, "GUID survives mutable positions")
check(replaced != original && replaced == otherGUID, "same positions do not preserve another GUID")

// This assertion precedes EVERY selection-entry call. With opt-in enabled, the
// main-thread guard must return unavailable before even rejecting the deliberately
// invalid identities. With opt-in disabled, notEnabled must precede that guard.
check(Thread.isMainThread, "selection guard cases run on the main thread")
let guardedTargets = [
    ITermFocusTarget(dashboardPID: 0, dashboardStartSeconds: 0,
                     dashboardStartMicroseconds: 0, itermSessionID: "malformed"),
    ITermFocusTarget(dashboardPID: Int32.max, dashboardStartSeconds: UInt64.max,
                     dashboardStartMicroseconds: UInt64.max, itermSessionID: oversizedIdentity),
    ITermFocusTarget(dashboardPID: 1, dashboardStartSeconds: 1,
                     dashboardStartMicroseconds: 0, itermSessionID: "w0t0p0:" + guid),
]
for (index, target) in guardedTargets.enumerated() {
    check(ITermFocus.requestAuthorization(for: target, ownerIsCurrent: { true }, admission: { true }) == .unavailable,
          "explicit authorization main-thread guard before context \(index)")
    check(ITermFocus.selectExistingSession(for: target, explicitlyEnabled: false, ownerIsCurrent: { true }) == .notEnabled,
          "opt-in guard before context and main-thread guard \(index)")
    check(ITermFocus.selectExistingSession(for: target, explicitlyEnabled: true, ownerIsCurrent: { true }) == .unavailable,
          "main-thread guard before context \(index)")
}

print("iTerm pure headless checks: \(checks)")
