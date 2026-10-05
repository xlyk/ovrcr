// The callback forwarding entry is also exercised by headless tests, without
// AppKit or UserNotifications. No Server startup, shell, provider or input APIs.
import CryptoKit
import Darwin
import Foundation
import Security

let navigationUserInfoKey = "ovrcr_navigation"

struct BridgeNavigationTicket: Codable, Equatable {
    let schema: UInt32
    let server_wire: UInt32
    let server_socket: String
    let callback_executable: String
    let callback_executable_sha256: String
    let server_lifetime: String
    let session: UInt64
    let run: UInt64

    var valid: Bool {
        schema == bridgeSchema && server_wire == bridgeServerWire
            && canonicalNavigationUUID(server_lifetime)
            && session > 0 && run > 0
            && navigationPath(server_socket) && navigationPath(callback_executable)
            && callback_executable_sha256.utf8.count == 64
            && callback_executable_sha256.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) })
            && URL(fileURLWithPath: callback_executable).lastPathComponent == "ovrcr"
    }

    var data: Data? { try? JSONEncoder().encode(self) }
    var userInfo: [AnyHashable: Any] {
        guard let data = data else { return [:] }
        return [navigationUserInfoKey: data]
    }

    static func decode(userInfo: [AnyHashable: Any]) -> BridgeNavigationTicket? {
        guard let data = userInfo[navigationUserInfoKey] as? Data,
              !data.isEmpty, data.count <= bridgeMaxBytes,
              let ticket = try? JSONDecoder().decode(Self.self, from: data), ticket.valid else { return nil }
        return ticket
    }
}

func canonicalNavigationUUID(_ text: String) -> Bool {
    guard text.utf8.count == 36 else { return false }
    return text.utf8.enumerated().allSatisfy { index, byte in
        [8, 13, 18, 23].contains(index) ? byte == 45 : (48...57).contains(byte) || (97...102).contains(byte)
    }
}

func navigationPath(_ text: String) -> Bool {
    text.hasPrefix("/") && text.utf8.count <= 4096
        && !text.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
        && !text.split(separator: "/").contains("..")
}

struct BridgeActivationTarget: Codable, Equatable {
    let dashboard_pid: UInt32
    let dashboard_start_seconds: UInt64
    let dashboard_start_microseconds: UInt64
    let iterm_session_id: String?
    let iterm_focus: Bool
    let owner: BridgeOwnerTicket?
}

struct BridgeNavigationResult: Decodable {
    let schema: UInt32
    let server_wire: UInt32
    let applied: Bool
    let activation: BridgeActivationTarget?

    static func decode(_ data: Data) -> BridgeNavigationResult? {
        guard !data.isEmpty, data.count <= bridgeMaxBytes,
              let result = try? JSONDecoder().decode(Self.self, from: data),
              result.schema == bridgeSchema, result.server_wire == bridgeServerWire,
              result.applied || result.activation == nil else { return nil }
        return result
    }
}

struct TrustedNavigationHelper {
    let executable: String
    let sha256: String
}

// Info.plist and the fixed helper are covered by the enclosing app signature.
// No source-machine path is pinned; the package may be installed or relocated.
func trustedNavigationHelper() -> TrustedNavigationHelper? {
    let bundle = Bundle.main
    let url = bundle.bundleURL
    guard url.pathExtension == "app", url.isFileURL,
          let hash = bundle.object(forInfoDictionaryKey: "OVRCRCallbackSHA256") as? String,
          hash.utf8.count == 64, hash.utf8.allSatisfy({ (48...57).contains($0) || (97...102).contains($0) }) else { return nil }
    var code: SecStaticCode?
    guard SecStaticCodeCreateWithPath(url as CFURL, [], &code) == errSecSuccess,
          let code = code,
          SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate | kSecCSCheckAllArchitectures | kSecCSCheckNestedCode), nil) == errSecSuccess else { return nil }
    return TrustedNavigationHelper(executable: url.appendingPathComponent("Contents/MacOS/ovrcr").path, sha256: hash)
}

func navigationExecutableHash(_ path: String, deadline: Deadline) -> String? {
    guard navigationPath(path) else { return nil }
    let fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
    guard fd >= 0 else { return nil }
    defer { close(fd) }
    var before = stat(), after = stat(), named = stat()
    guard fstat(fd, &before) == 0, before.st_uid == getuid(),
          (before.st_mode & S_IFMT) == S_IFREG, before.st_size > 0,
          before.st_size <= 512 * 1024 * 1024, (before.st_mode & 0o111) != 0 else { return nil }
    var digest = SHA256()
    var chunk = [UInt8](repeating: 0, count: 64 * 1024)
    var count: Int64 = 0
    while deadline.remaining > 0 {
        let readCount = read(fd, &chunk, chunk.count)
        if readCount < 0 { if errno == EINTR { continue }; return nil }
        if readCount == 0 { break }
        count += Int64(readCount)
        guard count <= before.st_size else { return nil }
        digest.update(data: Data(chunk.prefix(readCount)))
    }
    guard deadline.remaining > 0, count == before.st_size,
          fstat(fd, &after) == 0, lstat(path, &named) == 0,
          before.st_dev == after.st_dev, before.st_ino == after.st_ino,
          before.st_size == after.st_size,
          before.st_mtimespec.tv_sec == after.st_mtimespec.tv_sec,
          before.st_mtimespec.tv_nsec == after.st_mtimespec.tv_nsec,
          before.st_ctimespec.tv_sec == after.st_ctimespec.tv_sec,
          before.st_ctimespec.tv_nsec == after.st_ctimespec.tv_nsec,
          before.st_dev == named.st_dev, before.st_ino == named.st_ino else { return nil }
    return digest.finalize().map { String(format: "%02x", $0) }.joined()
}

// The only argv admitted by a callback. Ticket paths provide compatibility
// evidence only. Launch authority is the fixed, sealed enclosing-bundle helper.
func forwardNavigation(_ ticket: BridgeNavigationTicket, deadline: Deadline,
                       helper: TrustedNavigationHelper? = trustedNavigationHelper()) -> BridgeNavigationResult? {
    guard ticket.valid, let input = ticket.data else { return nil }
    let context = BridgeOwnerContext(server_socket: ticket.server_socket,
        callback_executable: ticket.callback_executable,
        callback_executable_sha256: ticket.callback_executable_sha256,
        server_lifetime: ticket.server_lifetime)
    guard let output = invokeBridgeHelper(input, context: context, command: "navigate", deadline: deadline, helper: helper) else { return nil }
    return BridgeNavigationResult.decode(output)
}

func invokeBridgeHelper(_ input: Data, context: BridgeOwnerContext, command: String, deadline: Deadline,
                        helper: TrustedNavigationHelper? = trustedNavigationHelper()) -> Data? {
    guard context.valid, input.count <= bridgeMaxBytes, ["navigate", "owner"].contains(command),
          let helper = helper,
          context.callback_executable_sha256 == helper.sha256,
          navigationExecutableHash(helper.executable, deadline: deadline) == helper.sha256,
          navigationExecutableHash(context.callback_executable, deadline: deadline) == helper.sha256 else { return nil }
    var incoming: [Int32] = [0, 0], outgoing: [Int32] = [0, 0]
    guard pipe(&incoming) == 0 else { return nil }
    guard pipe(&outgoing) == 0 else { close(incoming[0]); close(incoming[1]); return nil }
    defer { for fd in incoming + outgoing where fd >= 0 { close(fd) } }
    var actions: posix_spawn_file_actions_t?
    var attributes: posix_spawnattr_t?
    guard posix_spawn_file_actions_init(&actions) == 0 else { return nil }
    defer { posix_spawn_file_actions_destroy(&actions) }
    guard posix_spawnattr_init(&attributes) == 0 else { return nil }
    defer { posix_spawnattr_destroy(&attributes) }
    posix_spawn_file_actions_adddup2(&actions, incoming[0], STDIN_FILENO)
    posix_spawn_file_actions_adddup2(&actions, outgoing[1], STDOUT_FILENO)
    posix_spawn_file_actions_addopen(&actions, STDERR_FILENO, "/dev/null", O_WRONLY, 0)
    for fd in incoming + outgoing { posix_spawn_file_actions_addclose(&actions, fd) }
    posix_spawnattr_setflags(&attributes, Int16(POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT))
    posix_spawnattr_setpgroup(&attributes, 0)
    let arguments = [helper.executable, "bridge", command, "--stdin"]
    var argv = arguments.map { strdup($0) } + [nil]
    defer { for value in argv { free(value) } }
    // Avoid arbitrary launch/environment settings inherited from the Bridge.
    var environment = [strdup("PATH=/usr/bin:/bin"), nil]
    defer { for value in environment { free(value) } }
    var child: pid_t = 0
    guard deadline.remaining > 0,
          posix_spawn(&child, helper.executable, &actions, &attributes, &argv, &environment) == 0 else { return nil }
    close(incoming[0]); incoming[0] = -1
    close(outgoing[1]); outgoing[1] = -1
    _ = fcntl(incoming[1], F_SETFL, O_NONBLOCK)
    _ = fcntl(outgoing[0], F_SETFL, O_NONBLOCK)
    // Pipe writes must report EPIPE instead of terminating the persistent app.
    _ = fcntl(incoming[1], F_SETNOSIGPIPE, 1)
    var reaped = false
    defer {
        // A numeric PGID ceases to prove ownership after waitpid reaps the leader.
        if !reaped { _ = kill(-child, SIGKILL); while waitpid(child, nil, 0) < 0 && errno == EINTR {} }
    }
    var sent = 0, output = Data(), eof = false, status: Int32 = 0
    while deadline.remaining > 0 {
        if incoming[1] >= 0 {
            let written = input.withUnsafeBytes { buffer in
                write(incoming[1], buffer.baseAddress!.advanced(by: sent), input.count - sent)
            }
            if written > 0 { sent += written }
            else if written < 0 && errno != EAGAIN && errno != EINTR { return nil }
            if sent == input.count { close(incoming[1]); incoming[1] = -1 }
        }
        var chunk = [UInt8](repeating: 0, count: 4096)
        let count = read(outgoing[0], &chunk, chunk.count)
        if count > 0 {
            guard output.count + count <= bridgeMaxBytes else { return nil }
            output.append(contentsOf: chunk.prefix(count))
        } else if count == 0 { eof = true }
        else if errno != EAGAIN && errno != EINTR { return nil }
        if !reaped {
            let ended = waitpid(child, &status, WNOHANG)
            if ended == child { reaped = true }
            else if ended < 0 && errno != EINTR {
                if errno == ECHILD { reaped = true }
                return nil
            }
        }
        if reaped && eof {
            guard status == 0, sent == input.count else { return nil }
            return output
        }
        var pollFD = pollfd(fd: outgoing[0], events: Int16(POLLIN), revents: 0)
        _ = poll(&pollFD, 1, Int32(min(10, deadline.remaining * 1000)))
    }
    return nil
}

func currentDashboardProcess(_ target: BridgeActivationTarget) -> proc_bsdinfo? {
    guard target.dashboard_pid > 0, target.dashboard_pid <= UInt32(Int32.max) else { return nil }
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    let read = withUnsafeMutablePointer(to: &info) { pointer in
        proc_pidinfo(Int32(target.dashboard_pid), PROC_PIDTBSDINFO, 0, pointer, size)
    }
    guard read == size, info.pbi_uid == getuid(), info.pbi_pid == target.dashboard_pid,
          info.pbi_start_tvsec == target.dashboard_start_seconds,
          info.pbi_start_tvusec == target.dashboard_start_microseconds else { return nil }
    return info
}

func currentDashboardHasAncestor(_ target: BridgeActivationTarget, ancestor: proc_bsdinfo) -> Bool {
    guard var current = currentDashboardProcess(target) else { return false }
    var seen = Set<UInt32>()
    for _ in 0..<32 {
        guard current.pbi_uid == getuid(), seen.insert(current.pbi_pid).inserted else { return false }
        if current.pbi_pid == ancestor.pbi_pid {
            return current.pbi_start_tvsec == ancestor.pbi_start_tvsec
                && current.pbi_start_tvusec == ancestor.pbi_start_tvusec
        }
        guard current.pbi_ppid > 1, current.pbi_ppid <= UInt32(Int32.max) else { return false }
        var parent = proc_bsdinfo()
        let size = Int32(MemoryLayout<proc_bsdinfo>.size)
        let read = withUnsafeMutablePointer(to: &parent) {
            proc_pidinfo(Int32(current.pbi_ppid), PROC_PIDTBSDINFO, 0, $0, size)
        }
        guard read == size, parent.pbi_pid == current.pbi_ppid else { return false }
        current = parent
    }
    return false
}
