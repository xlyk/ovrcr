// Production protected-login reader and ancestry admission with injected
// metadata only. Every native lookup is replaced: no sysctl/proc_pidinfo,
// AppKit process discovery, permission, AppleEvent, focus or signal call.
// Root compiles/runs through scripts/build-bridge.sh --check.
// Optional --legacy-full-bsd-only deliberately denies the measured protected
// hop at the old reader boundary and must FAIL the required positive assertion.
// This is a counterfactual of the captured EPERM/same-UID boundary, not an
// execution of the original source or proof of native iTerm selection.
import Foundation

var checks = 0
func check(_ condition: @autoclosure () -> Bool, _ name: String) {
    guard condition() else { fatalError("iTerm ancestry check failed: \(name)") }
    checks += 1
}

let userID: UInt32 = 501
let dashboardPID: UInt32 = 71001, shellPID: UInt32 = 71000
let loginPID: UInt32 = 69807, serverPID: UInt32 = 40192, itermPID: UInt32 = 40100
let tty: UInt32 = 268435463
let serverDirectory = "/Users/fixture/Library/Application Support/iTerm2"
let itermImage = "/Applications/iTerm.app/Contents/MacOS/iTerm2"

func process(_ pid: UInt32, parent: UInt32, image: String, uid: UInt32 = userID,
             realUID: UInt32 = userID, group: UInt32? = nil, device: UInt32 = tty,
             seconds: UInt64 = 1791318905, microseconds: UInt64 = 225383) -> BridgeAncestryProcess {
    BridgeAncestryProcess(pid: pid, uid: uid, realUID: realUID, parentPID: parent,
        processGroup: group ?? pid, ttyDevice: device, startSeconds: seconds,
        startMicroseconds: microseconds, executable: image)
}
func changed(_ original: BridgeAncestryProcess, pid: UInt32? = nil, uid: UInt32? = nil,
             realUID: UInt32? = nil, parent: UInt32? = nil, group: UInt32? = nil,
             device: UInt32? = nil, seconds: UInt64? = nil, microseconds: UInt64? = nil,
             image: String? = nil) -> BridgeAncestryProcess {
    BridgeAncestryProcess(pid: pid ?? original.pid, uid: uid ?? original.uid,
        realUID: realUID ?? original.realUID, parentPID: parent ?? original.parentPID,
        processGroup: group ?? original.processGroup, ttyDevice: device ?? original.ttyDevice,
        startSeconds: seconds ?? original.startSeconds,
        startMicroseconds: microseconds ?? original.startMicroseconds,
        executable: image ?? original.executable)
}
func application(_ pid: UInt32, image: String = itermImage,
                 bundle: String? = "com.googlecode.iterm2", terminated: Bool = false,
                 prohibited: Bool = false) -> BridgeAncestryApplication {
    BridgeAncestryApplication(pid: pid, bundleID: bundle, executable: image,
        isTerminated: terminated, isProhibited: prohibited)
}

// Login scalar values reproduce Root199's actual stable public metadata.
// Adjacent same-user nodes are synthetic; no existing account file is read.
let measuredLogin = process(loginPID, parent: serverPID, image: "/usr/bin/login",
    uid: 0, seconds: 1791318904, microseconds: 225383)
func brief(_ p: BridgeAncestryProcess, status: UInt32 = 2) -> BridgeAncestryShort {
    BridgeAncestryShort(pid: p.pid, uid: p.uid, realUID: p.realUID,
        parentPID: p.parentPID, processGroup: p.processGroup, status: status)
}
final class RawReads {
    var first: BridgeAncestryKernel?
    var second: BridgeAncestryKernel?
    var short: BridgeAncestryShort?
    var firstImage: String?
    var secondImage: String?
    var image: String? {
        get { firstImage }
        set { firstImage = newValue; secondImage = newValue }
    }
    var calls: [String] = []
    init(_ value: BridgeAncestryProcess = measuredLogin) {
        // KERN_PROC_PID does not provide the image; the path reader supplies it.
        let kernel = BridgeAncestryKernel(process: value.image(""), status: 2)
        first = kernel; second = kernel; short = brief(value)
        firstImage = value.executable; secondImage = value.executable
    }
    func read(pid: UInt32 = loginPID, user: UInt32 = userID) -> BridgeAncestryProcess? {
        var kernelCalls = 0, pathCalls = 0
        return BridgeProcessAncestry.protectedLogin(pid, userID: user,
            kernel: { requested in
                precondition(requested == pid, "reader must keep the exact requested PID")
                self.calls.append("kernel"); kernelCalls += 1
                precondition(kernelCalls <= 2, "reader must not retry")
                return kernelCalls == 1 ? self.first : self.second
            }, short: { requested in
                precondition(requested == pid); self.calls.append("short"); return self.short
            }, path: { requested in
                precondition(requested == pid); self.calls.append("path"); pathCalls += 1
                precondition(pathCalls <= 2, "image reader must not retry")
                return pathCalls == 1 ? self.firstImage : self.secondImage
            })
    }
}
let stable = RawReads()
check(stable.read() == measuredLogin, "stable UID0/RUID501 login preserves exact birth, links, group, TTY and image")
check(stable.calls == ["kernel", "short", "path", "kernel", "path"], "exact bounded independent reads, without retry")

let rawRejections: [(String, (RawReads) -> Void)] = [
    ("denied/missing first kernel result", { $0.first = nil }),
    ("missing/short brief result", { $0.short = nil }),
    ("unreadable image", { $0.image = nil }),
    ("denied/missing second kernel result", { $0.second = nil }),
    ("unreadable image recheck", { $0.secondImage = nil }),
    ("image changes between independent reads", { $0.secondImage = "/usr/bin/su" }),
    ("wrong requested PID", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, pid: loginPID + 1), status: 2) }),
    ("non-root effective UID", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, uid: userID), status: 2) }),
    ("foreign real UID", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, realUID: userID + 1), status: 2) }),
    ("uninitialized status", { $0.first = BridgeAncestryKernel(process: $0.first!.process, status: 0) }),
    ("zombie", { $0.first = BridgeAncestryKernel(process: $0.first!.process, status: 5) }),
    ("zero birth seconds", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, seconds: 0), status: 2) }),
    ("invalid birth microseconds", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, microseconds: 1_000_000), status: 2) }),
    ("PID1 parent", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, parent: 1), status: 2) }),
    ("out-of-range parent", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, parent: UInt32.max), status: 2) }),
    ("missing group", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, group: 0), status: 2) }),
    ("missing controlling TTY", { $0.first = BridgeAncestryKernel(process: changed($0.first!.process, device: UInt32.max), status: 2) }),
    ("other root executable", { $0.image = "/usr/bin/su" }),
    ("lookalike login image", { $0.image = "/tmp/login" }),
    ("noncanonical image spelling", { $0.image = "/usr/bin/../bin/login" }),
    ("brief PID differs", { $0.short = brief(changed(measuredLogin, pid: loginPID + 1)) }),
    ("brief UID differs", { $0.short = brief(changed(measuredLogin, uid: userID)) }),
    ("brief real UID differs", { $0.short = brief(changed(measuredLogin, realUID: userID + 1)) }),
    ("brief parent differs", { $0.short = brief(changed(measuredLogin, parent: serverPID + 1)) }),
    ("brief group differs", { $0.short = brief(changed(measuredLogin, group: loginPID + 1)) }),
    ("brief status differs", { $0.short = brief(measuredLogin, status: 3) }),
    ("PID changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, pid: loginPID + 1), status: 2) }),
    ("UID changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, uid: userID), status: 2) }),
    ("real UID changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, realUID: userID + 1), status: 2) }),
    ("parent changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, parent: serverPID + 1), status: 2) }),
    ("group changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, group: loginPID + 1), status: 2) }),
    ("TTY changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, device: tty + 1), status: 2) }),
    ("birth seconds change between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, seconds: measuredLogin.startSeconds + 1), status: 2) }),
    ("microsecond birth changes between kernel reads", { $0.second = BridgeAncestryKernel(process: changed($0.second!.process, microseconds: measuredLogin.startMicroseconds + 1), status: 2) }),
    ("status changes between kernel reads", { $0.second = BridgeAncestryKernel(process: $0.second!.process, status: 3) }),
]
for (name, mutate) in rawRejections {
    let reads = RawReads(); mutate(reads)
    check(reads.read() == nil, name)
}
for pid in [UInt32(0), 1, UInt32.max] {
    let reads = RawReads()
    check(reads.read(pid: pid) == nil && reads.calls.isEmpty, "invalid PID refuses before metadata reads \(pid)")
}
let rootUser = RawReads()
check(rootUser.read(user: 0) == nil && rootUser.calls.isEmpty, "root is not the subject user")

final class Ancestry {
    var nodes: [UInt32: BridgeAncestryProcess] = [
        dashboardPID: process(dashboardPID, parent: shellPID, image: "/owned/ovrcr"),
        shellPID: process(shellPID, parent: loginPID, image: "/bin/zsh"),
        loginPID: measuredLogin,
        serverPID: process(serverPID, parent: itermPID, image: serverDirectory + "/iTermServer-3.7.2",
                           device: UInt32.max, seconds: 1789744262, microseconds: 330614),
        itermPID: process(itermPID, parent: 1, image: itermImage, device: UInt32.max,
                          seconds: 1789744260, microseconds: 1),
    ]
    var apps: [UInt32: BridgeAncestryApplication] = [itermPID: application(itermPID)]
    var directory: String? = serverDirectory
    var fullBSDOnly = false
    var denied = Set<UInt32>()
    var ownedReads: [UInt32] = [], protectedReads: [UInt32] = [], appReads: [UInt32] = []
    var mutateOwned: ((UInt32, Int, BridgeAncestryProcess) -> BridgeAncestryProcess?)?
    var mutateProtected: ((UInt32, Int, BridgeAncestryProcess) -> BridgeAncestryProcess?)?
    func owned(_ pid: UInt32) -> BridgeAncestryProcess? {
        ownedReads.append(pid)
        guard !denied.contains(pid), let p = nodes[pid], p.uid == userID else { return nil }
        if let mutate = mutateOwned { return mutate(pid, ownedReads.filter({ $0 == pid }).count, p) }
        return p
    }
    func protected(_ pid: UInt32) -> BridgeAncestryProcess? {
        protectedReads.append(pid)
        guard !fullBSDOnly, !denied.contains(pid), let p = nodes[pid] else { return nil }
        let value: BridgeAncestryProcess
        if let mutate = mutateProtected {
            guard let read = mutate(pid, protectedReads.filter({ $0 == pid }).count, p) else { return nil }
            value = read
        } else { value = p }
        // Exercise the same shipping stable reader for the walker, too. All
        // three callbacks are supplied; none of its live defaults can run.
        return RawReads(value).read(pid: pid)
    }
    var lookup: BridgeProcessAncestry.Lookup {
        BridgeProcessAncestry.Lookup(owned: { self.owned($0) }, protectedLogin: { self.protected($0) },
            application: { self.appReads.append($0); return self.apps[$0] }, itermServerDirectory: directory)
    }
    func proof(depth: Int = 64, continuing: () -> Bool = { true }) -> BridgeParentAncestry? {
        guard let dashboard = nodes[dashboardPID] else { return nil }
        return BridgeProcessAncestry.itermApplication(from: dashboard, userID: userID,
            maximumDepth: depth, lookup: lookup, continuing: continuing)
    }
    func update(_ pid: UInt32, _ mutation: (BridgeAncestryProcess) -> BridgeAncestryProcess) {
        nodes[pid] = mutation(nodes[pid]!)
    }
}

let normal = Ancestry()
normal.update(shellPID) { changed($0, parent: serverPID) }
let normalProof = normal.proof()
check(normalProof?.application == normal.nodes[itermPID], "unchanged all-same-UID ancestry still reaches exact iTerm")
check(normalProof?.chain.map({ $0.pid }) == [dashboardPID, shellPID, serverPID, itermPID]
    && normal.protectedReads.isEmpty, "ordinary route performs no protected reader fallback")

let legacy = Ancestry(); legacy.fullBSDOnly = true
check(legacy.proof() == nil, "captured full-BSD/same-UID reader refusal cannot skip the login hop")
let accepted = Ancestry()
accepted.fullBSDOnly = CommandLine.arguments.contains("--legacy-full-bsd-only")
let acceptedProof = accepted.proof()
check(acceptedProof?.application == accepted.nodes[itermPID], "default-login chain must succeed with the strict protected reader")
check(acceptedProof?.chain.map({ $0.pid }) == [dashboardPID, shellPID, loginPID, serverPID, itermPID],
      "proof retains every exact identity including the protected hop")
check(accepted.protectedReads == [loginPID, loginPID], "only one protected PID is admitted and reread, never skipped")
check(accepted.proof() == acceptedProof, "unchanged full chain revalidates equal to the cached proof")

let routeRejections: [(String, (Ancestry) -> Void)] = [
    ("missing Dashboard metadata", { $0.denied.insert(dashboardPID) }),
    ("foreign Dashboard UID", { $0.update(dashboardPID) { changed($0, uid: userID + 1) } }),
    ("missing protected metadata", { $0.denied.insert(loginPID) }),
    ("wrong root image", { $0.update(loginPID) { changed($0, image: "/usr/bin/su") } }),
    ("wrong login real UID", { $0.update(loginPID) { changed($0, realUID: userID + 1) } }),
    ("missing subject TTY", { $0.update(dashboardPID) { changed($0, device: UInt32.max) } }),
    ("child TTY differs", { $0.update(shellPID) { changed($0, device: tty + 1) } }),
    ("protected TTY differs", { $0.update(loginPID) { changed($0, device: tty + 1) } }),
    ("login parent unreadable", { $0.denied.insert(serverPID) }),
    ("login parent UID differs", { $0.update(serverPID) { changed($0, uid: userID + 1) } }),
    ("login parent outside account cache", { $0.update(serverPID) { changed($0, image: "/tmp/iTermServer-3.7.2") } }),
    ("lookalike account cache", { $0.update(serverPID) { changed($0, image: serverDirectory + "-other/iTermServer-3.7.2") } }),
    ("wrong login parent executable", { $0.update(serverPID) { changed($0, image: serverDirectory + "/zsh") } }),
    ("empty server version", { $0.update(serverPID) { changed($0, image: serverDirectory + "/iTermServer-") } }),
    ("unbounded server version", { $0.update(serverPID) { changed($0, image: serverDirectory + "/iTermServer-" + String(repeating: "x", count: 65)) } }),
    ("invalid server name", { $0.update(serverPID) { changed($0, image: serverDirectory + "/iTermServer-3.7.2!") } }),
    ("missing pinned account directory", { $0.directory = nil }),
    ("different account directory", { $0.directory = "/Users/other/Library/Application Support/iTerm2" }),
    ("server is an intervening GUI", { $0.apps[serverPID] = application(serverPID, image: $0.nodes[serverPID]!.executable, bundle: "dev.other.gui") }),
    ("server not directly under iTerm", { $0.update(serverPID) { changed($0, parent: shellPID) } }),
    ("iTerm process unreadable", { $0.denied.insert(itermPID) }),
    ("foreign iTerm UID", { $0.update(itermPID) { changed($0, uid: userID + 1) } }),
    ("missing iTerm application", { $0.apps.removeValue(forKey: itermPID) }),
    ("application PID differs", { $0.apps[itermPID] = application(itermPID + 1) }),
    ("wrong application bundle", { $0.apps[itermPID] = application(itermPID, bundle: "dev.other.gui") }),
    ("application executable differs", { $0.apps[itermPID] = application(itermPID, image: "/tmp/iTerm2") }),
    ("terminated iTerm", { $0.apps[itermPID] = application(itermPID, terminated: true) }),
    ("prohibited iTerm", { $0.apps[itermPID] = application(itermPID, prohibited: true) }),
    ("second protected hop", { fixture in
        let second: UInt32 = loginPID + 1
        fixture.nodes[second] = changed(measuredLogin, pid: second)
        fixture.update(loginPID) { changed($0, parent: second) }
    }),
    ("protected birth changes on recheck", { $0.mutateProtected = { pid, count, p in
        pid == loginPID && count > 1 ? changed(p, microseconds: p.startMicroseconds + 1) : p
    } }),
    ("child parent changes on recheck", { $0.mutateOwned = { pid, count, p in
        pid == shellPID && count > 1 ? changed(p, parent: serverPID) : p
    } }),
    ("server birth changes on recheck", { $0.mutateOwned = { pid, count, p in
        pid == serverPID && count > 1 ? changed(p, microseconds: p.startMicroseconds + 1) : p
    } }),
    ("server parent changes on recheck", { $0.mutateOwned = { pid, count, p in
        pid == serverPID && count > 1 ? changed(p, parent: shellPID) : p
    } }),
    ("iTerm birth changes on recheck", { $0.mutateOwned = { pid, count, p in
        pid == itermPID && count > 1 ? changed(p, microseconds: p.startMicroseconds + 1) : p
    } }),
    ("protected recheck becomes unreadable", { $0.mutateProtected = { pid, count, p in
        pid == loginPID && count > 1 ? nil : p
    } }),
    ("server recheck becomes unreadable", { $0.mutateOwned = { pid, count, p in
        pid == serverPID && count > 1 ? nil : p
    } }),
]
for (name, mutate) in routeRejections {
    let fixture = Ancestry(); mutate(fixture)
    check(fixture.proof() == nil, name)
}

let gui = Ancestry(), guiPID: UInt32 = 70000
gui.nodes[guiPID] = process(guiPID, parent: serverPID, image: "/owned/ovrcr-gui")
gui.apps[guiPID] = application(guiPID, image: "/owned/ovrcr-gui", bundle: "dev.ovrcr.gui")
gui.update(shellPID) { changed($0, parent: guiPID) }
check(gui.proof() == nil, "iTerm admission rejects an intervening GUI even with iTerm above it")
let guiParent = BridgeProcessAncestry.parentApplication(from: gui.nodes[dashboardPID]!, userID: userID,
    maximumDepth: 64, lookup: gui.lookup)
check(guiParent?.application.pid == guiPID && guiParent?.chain.map({ $0.pid }) == [dashboardPID, shellPID, guiPID],
      "generic activation stops at the first GUI and retains that exact route")
check(gui.protectedReads.isEmpty && !gui.appReads.contains(itermPID), "GUI route never probes the inherited iTerm destination")

let cycle = Ancestry(); cycle.update(shellPID) { changed($0, parent: dashboardPID) }
check(cycle.proof() == nil, "same-user cycle rejects instead of looping")
let deep = Ancestry()
for index in 0..<63 {
    let pid = UInt32(72000 + index)
    deep.nodes[pid] = process(pid, parent: index == 62 ? itermPID : pid + 1, image: "/bin/zsh")
}
deep.update(dashboardPID) { changed($0, parent: 72000) }
check(deep.proof() == nil, "65-node ordinary chain exceeds the existing 64-node bound")
let boundary = Ancestry()
check(boundary.proof(depth: 4) == nil, "protected route cannot exceed the caller's depth budget")
check(boundary.proof(depth: 5) != nil, "five identities fit the exact protected route budget")
check(boundary.proof(depth: 0) == nil && boundary.proof(depth: 65) == nil, "invalid depth budgets reject")
let stale = Ancestry()
check(stale.proof(continuing: { false }) == nil && stale.appReads.isEmpty && stale.protectedReads.isEmpty,
      "stale current-owner guard admits no ancestry or protected hop")
let replaced = Ancestry()
check(replaced.proof(continuing: { replaced.protectedReads.count < 2 }) == nil,
      "owner replacement during protected recheck refuses the prepared route")

// The production Context caches this exact Equatable proof and compares a
// freshly resolved proof before native operations. Exercise every cached
// identity, not merely the final iTerm PID or the Dashboard PID/birth pair.
let revalidationChanges: [(String, (Ancestry) -> Void)] = [
    ("Dashboard birth", { $0.update(dashboardPID) { changed($0, microseconds: $0.startMicroseconds + 1) } }),
    ("Dashboard TTY", { $0.update(dashboardPID) { changed($0, device: tty + 1) } }),
    ("shell parent", { $0.update(shellPID) { changed($0, parent: serverPID) } }),
    ("login birth", { $0.update(loginPID) { changed($0, microseconds: $0.startMicroseconds + 1) } }),
    ("login executable", { $0.update(loginPID) { changed($0, image: "/usr/bin/su") } }),
    ("login real UID", { $0.update(loginPID) { changed($0, realUID: userID + 1) } }),
    ("login parent", { $0.update(loginPID) { changed($0, parent: shellPID) } }),
    ("login TTY", { $0.update(loginPID) { changed($0, device: tty + 1) } }),
    ("server birth", { $0.update(serverPID) { changed($0, microseconds: $0.startMicroseconds + 1) } }),
    ("server image", { $0.update(serverPID) { changed($0, image: serverDirectory + "/iTermServer-3.7.3") } }),
    ("iTerm birth", { $0.update(itermPID) { changed($0, microseconds: $0.startMicroseconds + 1) } }),
]
for (name, mutate) in revalidationChanges {
    let fixture = Ancestry()
    let cached = fixture.proof()
    precondition(cached != nil)
    mutate(fixture)
    check(fixture.proof() != cached, "cached full proof is retired after \(name) changes")
}

print("\(checks) production protected-login reader and ancestry checks passed with injected metadata; no native lookup, permission, AppleEvent or activation executed.")
