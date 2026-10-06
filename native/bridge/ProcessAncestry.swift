import Darwin
import Foundation
import AppKit

// Identity metadata only: no argv, environment, terminal contents or signals.
struct BridgeAncestryProcess: Equatable {
    let pid: UInt32
    let uid: UInt32
    let realUID: UInt32
    let parentPID: UInt32
    let processGroup: UInt32
    let ttyDevice: UInt32
    let startSeconds: UInt64
    let startMicroseconds: UInt64
    let executable: String

    func image(_ path: String) -> Self {
        Self(pid: pid, uid: uid, realUID: realUID, parentPID: parentPID,
             processGroup: processGroup, ttyDevice: ttyDevice,
             startSeconds: startSeconds, startMicroseconds: startMicroseconds,
             executable: path)
    }
}

struct BridgeAncestryApplication: Equatable {
    let pid: UInt32
    let bundleID: String?
    let executable: String?
    let isTerminated: Bool
    let isProhibited: Bool
}

struct BridgeParentAncestry: Equatable {
    let application: BridgeAncestryProcess
    let chain: [BridgeAncestryProcess]
}

struct BridgeAncestryKernel: Equatable {
    let process: BridgeAncestryProcess
    let status: UInt32
}

struct BridgeAncestryShort: Equatable {
    let pid: UInt32
    let uid: UInt32
    let realUID: UInt32
    let parentPID: UInt32
    let processGroup: UInt32
    let status: UInt32
}

enum BridgeProcessAncestry {
    struct Lookup {
        let owned: (UInt32) -> BridgeAncestryProcess?
        let protectedLogin: (UInt32) -> BridgeAncestryProcess?
        let application: (UInt32) -> BridgeAncestryApplication?
        let itermServerDirectory: String?
    }

    static func itermApplication(from dashboard: BridgeAncestryProcess, userID: UInt32,
                                 maximumDepth: Int, lookup: Lookup,
                                 continuing: () -> Bool = { true }) -> BridgeParentAncestry? {
        guard let proof = parentApplication(from: dashboard, userID: userID,
            maximumDepth: maximumDepth, lookup: lookup, continuing: continuing),
              let app = lookup.application(proof.application.pid),
              app.pid == proof.application.pid, app.bundleID == "com.googlecode.iterm2",
              !app.isTerminated, !app.isProhibited,
              app.executable == proof.application.executable,
              lookup.owned(proof.application.pid) == proof.application,
              continuing() else { return nil }
        return proof
    }

    static func parentApplication(from dashboard: BridgeAncestryProcess, userID: UInt32,
                                  maximumDepth: Int, lookup: Lookup,
                                  continuing: () -> Bool = { true }) -> BridgeParentAncestry? {
        guard maximumDepth > 0, maximumDepth <= 64, dashboard.uid == userID,
              lookup.owned(dashboard.pid) == dashboard else { return nil }
        var current = dashboard, chain: [BridgeAncestryProcess] = [], seen = Set<UInt32>()
        while chain.count < maximumDepth {
            guard continuing(), current.uid == userID, current.pid > 1,
                  seen.insert(current.pid).inserted else { return nil }
            chain.append(current)
            if let app = lookup.application(current.pid), !app.isProhibited {
                // Never walk beyond the first GUI, even if iTerm is above it.
                guard app.pid == current.pid, !app.isTerminated,
                      app.executable == current.executable,
                      lookup.owned(current.pid) == current, continuing() else { return nil }
                return BridgeParentAncestry(application: current, chain: chain)
            }
            guard current.parentPID > 1, current.parentPID <= UInt32(Int32.max) else { return nil }
            if let parent = lookup.owned(current.parentPID) {
                guard parent.pid == current.parentPID else { return nil }
                current = parent
                continue
            }
            // The only cross-UID edge admitted is login -> iTermServer -> iTerm.
            // Every endpoint is independently read; a denied hop is never skipped.
            guard chain.count + 3 <= maximumDepth, current.ttyDevice == dashboard.ttyDevice,
                  dashboard.ttyDevice != UInt32.max, lookup.owned(current.pid) == current,
                  let login = lookup.protectedLogin(current.parentPID), login.pid == current.parentPID,
                  login.uid == 0, login.realUID == userID, login.executable == "/usr/bin/login",
                  login.ttyDevice == dashboard.ttyDevice, !seen.contains(login.pid),
                  lookup.application(login.pid)?.isProhibited != false,
                  let server = lookup.owned(login.parentPID), server.pid == login.parentPID,
                  server.uid == userID, !seen.contains(server.pid), server.pid != login.pid,
                  let directory = lookup.itermServerDirectory,
                  URL(fileURLWithPath: server.executable).deletingLastPathComponent().path == directory,
                  serverName(URL(fileURLWithPath: server.executable).lastPathComponent),
                  lookup.application(server.pid)?.isProhibited != false,
                  let application = lookup.owned(server.parentPID), application.pid == server.parentPID,
                  application.uid == userID, !seen.contains(application.pid),
                  application.pid != login.pid, application.pid != server.pid,
                  let app = lookup.application(application.pid), app.pid == application.pid,
                  !app.isTerminated, !app.isProhibited, app.bundleID == "com.googlecode.iterm2",
                  app.executable == application.executable,
                  lookup.protectedLogin(login.pid) == login,
                  lookup.owned(server.pid) == server, lookup.owned(application.pid) == application,
                  lookup.owned(current.pid) == current, continuing() else { return nil }
            chain.append(contentsOf: [login, server, application])
            return BridgeParentAncestry(application: application, chain: chain)
        }
        return nil
    }

    private static func serverName(_ value: String) -> Bool {
        let prefix = "iTermServer-"
        guard value.hasPrefix(prefix) else { return false }
        let suffix = value.utf8.dropFirst(prefix.utf8.count)
        return !suffix.isEmpty && suffix.count <= 64 && suffix.allSatisfy {
            (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0)
                || [UInt8(45), 46, 95].contains($0)
        }
    }

    static func protectedLogin(_ pid: UInt32, userID: UInt32) -> BridgeAncestryProcess? {
        protectedLogin(pid, userID: userID, kernel: publicKernel, short: publicShort, path: processPath)
    }

    static func protectedLogin(_ pid: UInt32, userID: UInt32,
                               kernel: (UInt32) -> BridgeAncestryKernel?,
                               short: (UInt32) -> BridgeAncestryShort?,
                               path: (UInt32) -> String?) -> BridgeAncestryProcess? {
        guard pid > 1, pid <= UInt32(Int32.max), userID != 0,
              let first = kernel(pid), first.process.pid == pid, first.process.uid == 0,
              first.process.realUID == userID, first.status > 0, first.status != 5,
              first.process.startSeconds > 0, first.process.startMicroseconds < 1_000_000,
              first.process.parentPID > 1, first.process.parentPID <= UInt32(Int32.max),
              first.process.processGroup > 0, first.process.ttyDevice != UInt32.max,
              let brief = short(pid), brief.pid == pid, brief.uid == first.process.uid,
              brief.realUID == first.process.realUID, brief.parentPID == first.process.parentPID,
              brief.processGroup == first.process.processGroup, brief.status == first.status,
              let image = path(pid), image == "/usr/bin/login",
              let second = kernel(pid), second == first,
              path(pid) == image else { return nil }
        return first.process.image(image)
    }

    static func ownedProcess(_ pid: UInt32, userID: UInt32) -> BridgeAncestryProcess? {
        guard pid > 1, pid <= UInt32(Int32.max) else { return nil }
        func read() -> BridgeAncestryKernel? {
            var info = proc_bsdinfo()
            let size = Int32(MemoryLayout<proc_bsdinfo>.size)
            guard size == 136, proc_pidinfo(Int32(pid), PROC_PIDTBSDINFO, 0, &info, size) == size,
                  info.pbi_pid == pid, info.pbi_uid == userID, info.pbi_status > 0, info.pbi_status != 5,
                  info.pbi_start_tvsec > 0, info.pbi_start_tvusec < 1_000_000 else { return nil }
            return BridgeAncestryKernel(process: BridgeAncestryProcess(pid: pid, uid: info.pbi_uid,
                realUID: info.pbi_ruid, parentPID: info.pbi_ppid, processGroup: info.pbi_pgid,
                ttyDevice: info.e_tdev, startSeconds: info.pbi_start_tvsec,
                startMicroseconds: info.pbi_start_tvusec, executable: ""), status: info.pbi_status)
        }
        guard let first = read(), let path = processPath(pid), let second = read(),
              first.process == second.process, processPath(pid) == path else { return nil }
        return first.process.image(path)
    }

    static func liveLookup(userID: UInt32) -> Lookup {
        Lookup(owned: { ownedProcess($0, userID: userID) },
               protectedLogin: { protectedLogin($0, userID: userID) },
               application: { pid in
                   guard let app = NSRunningApplication(processIdentifier: Int32(pid)) else { return nil }
                   return BridgeAncestryApplication(pid: pid, bundleID: app.bundleIdentifier,
                       executable: app.executableURL?.path, isTerminated: app.isTerminated,
                       isProhibited: app.activationPolicy == .prohibited)
               }, itermServerDirectory: serverDirectory(userID))
    }

    private static func serverDirectory(_ userID: UInt32) -> String? {
        var entry = passwd(), result: UnsafeMutablePointer<passwd>?
        var buffer = [CChar](repeating: 0, count: 16_384)
        return buffer.withUnsafeMutableBufferPointer { storage in
            guard getpwuid_r(userID, &entry, storage.baseAddress, storage.count, &result) == 0,
                  result != nil, let pointer = entry.pw_dir else { return nil }
            let home = String(cString: pointer)
            guard home.hasPrefix("/"), URL(fileURLWithPath: home).resolvingSymlinksInPath().path == home else { return nil }
            return home + "/Library/Application Support/iTerm2"
        }
    }

    private static func processPath(_ pid: UInt32) -> String? {
        var buffer = [CChar](repeating: 0, count: 4096)
        guard buffer.withUnsafeMutableBufferPointer({ proc_pidpath(Int32(pid), $0.baseAddress, UInt32($0.count)) }) > 0 else { return nil }
        let value = String(cString: buffer)
        // This is the kernel's image, not an externally supplied launch path.
        // Foundation may spell an existing /private/tmp image as /tmp; do not
        // reject that same live process merely because of that presentation.
        guard value.hasPrefix("/"), value.utf8.count < 4096,
              !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
              !value.split(separator: "/").contains("..") else { return nil }
        return value
    }

    private static func publicKernel(_ pid: UInt32) -> BridgeAncestryKernel? {
        var info = kinfo_proc(), size = MemoryLayout<kinfo_proc>.size
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, Int32(pid)]
        guard size == 648, mib.withUnsafeMutableBufferPointer({
            sysctl($0.baseAddress, UInt32($0.count), &info, &size, nil, 0)
        }) == 0, size == 648, info.kp_proc.p_pid > 1, info.kp_proc.p_pid == Int32(pid),
              info.kp_proc.p_stat > 0, info.kp_eproc.e_ppid > 1, info.kp_eproc.e_pgid > 0 else { return nil }
        let birth = info.kp_proc.p_un.__p_starttime
        guard birth.tv_sec > 0, birth.tv_usec >= 0, birth.tv_usec < 1_000_000 else { return nil }
        return BridgeAncestryKernel(process: BridgeAncestryProcess(pid: pid,
            uid: info.kp_eproc.e_ucred.cr_uid, realUID: info.kp_eproc.e_pcred.p_ruid,
            parentPID: UInt32(info.kp_eproc.e_ppid), processGroup: UInt32(info.kp_eproc.e_pgid),
            ttyDevice: UInt32(bitPattern: info.kp_eproc.e_tdev), startSeconds: UInt64(birth.tv_sec),
            startMicroseconds: UInt64(birth.tv_usec), executable: ""), status: UInt32(info.kp_proc.p_stat))
    }

    private static func publicShort(_ pid: UInt32) -> BridgeAncestryShort? {
        var info = proc_bsdshortinfo()
        let size = Int32(MemoryLayout<proc_bsdshortinfo>.size)
        guard size == 64, proc_pidinfo(Int32(pid), PROC_PIDT_SHORTBSDINFO, 0, &info, size) == size else { return nil }
        return BridgeAncestryShort(pid: info.pbsi_pid, uid: info.pbsi_uid, realUID: info.pbsi_ruid,
            parentPID: info.pbsi_ppid, processGroup: info.pbsi_pgid, status: info.pbsi_status)
    }
}
