// No AppKit/UserNotifications imports; all endpoints are fresh and task-owned.
import Darwin
import Foundation

var checks = 0
func check(_ condition: @autoclosure () -> Bool, _ name: String) {
    guard condition() else { fatalError("FAILED: \(name)") }
    checks += 1
}
func request(_ op: [String: Any], schema: UInt32 = bridgeSchema, wire: UInt32 = bridgeServerWire) -> Data {
    try! JSONSerialization.data(withJSONObject: ["schema": schema, "server_wire": wire, "op": op])
}
func rejected(_ data: Data, _ expected: BridgeStatus) -> Bool {
    if case .rejected(let status) = admit(data) { return status == expected }
    return false
}
func accepted(_ data: Data) -> Bool {
    if case .request = admit(data) { return true }
    return false
}
func deliver(subtitle: String = "manual label", body: String = "project / workspace / terminal (#1)") -> Data {
    request(["type": "deliver", "title": "OVRCR · response ready", "subtitle": subtitle, "body": body,
             "sound": NSNull(), "navigation": navigationFixture()])
}

func navigationFixture() -> [String: Any] {
    ["schema": bridgeSchema, "server_wire": bridgeServerWire, "server_socket": "/tmp/owned-server.sock",
     "callback_executable": "/tmp/ovrcr", "callback_executable_sha256": String(repeating: "0", count: 64), "server_lifetime": "10000000-0000-4000-8000-000000000001",
     "session": 1, "run": 2]
}

for kind in ["status", "authorize", "settings"] {
    check(accepted(request(["type": kind])), "control operation \(kind)")
}
check(accepted(deliver()), "silent identity payload")
check(accepted(request(["type": "status", "title": 42, "unknown": "ignored"])), "control extras ignored")
check(rejected(request(["type": "unknown"]), .failed), "unknown operation")
check(rejected(request(["type": "deliver", "title": "OVRCR · response ready"]), .failed), "missing display field")
check(rejected(request(["type": "deliver", "title": "private prompt", "subtitle": "x", "body": "x"]), .failed), "title allowlist")
check(rejected(request(["type": "invalid"], schema: bridgeSchema + 1), .incompatible), "schema before operation")
check(rejected(request(["type": "status"], wire: bridgeServerWire + 1), .incompatible), "wire mismatch")
for bytes in [Data(), Data("{}".utf8), Data("not JSON".utf8), Data(repeating: 32, count: bridgeMaxBytes + 1)] {
    check(rejected(bytes, .failed), "malformed/oversized input")
}
for invalid in ["-1", "4294967296", "true", "\"1\"", "1.5"] {
    let data = Data("{\"schema\":\(invalid),\"server_wire\":\(bridgeServerWire),\"op\":{\"type\":\"status\"}}".utf8)
    check(rejected(data, .failed), "invalid schema scalar \(invalid)")
}
check(accepted(deliver(subtitle: String(repeating: "🧑", count: 80))), "Unicode max subtitle")
check(accepted(deliver(subtitle: "👩‍💻", body: String(repeating: "x", count: 1024))), "ZWJ emoji/body bound")
check(rejected(deliver(subtitle: String(repeating: "x", count: 321)), .failed), "subtitle overflow")
check(rejected(deliver(body: String(repeating: "x", count: 1025)), .failed), "body overflow")
for value: UInt32 in [0, 9, 10, 31, 127, 159, 0x061C, 0x200E, 0x200F, 0x2028, 0x2029, 0x202A, 0x202E, 0x2066, 0x2069] {
    let text = "label" + String(UnicodeScalar(value)!)
    check(rejected(deliver(subtitle: text), .failed), "unsafe Unicode scalar \(value)")
}
for status in [BridgeStatus.available, .notDetermined, .permissionPending, .denied, .submitted, .settingsOpened, .incompatible, .failed, .itermPending, .itermAuthorized, .itermAuthorizationRequired, .itermDenied, .itermUnavailable] {
    check(receivedReply(BridgeReply(status).data()).status == status, "typed reply \(status.rawValue)")
}
check(receivedReply(Data("{\"schema\":\(bridgeSchema),\"server_wire\":\(bridgeServerWire + 1),\"status\":\"available\"}".utf8)).status == .incompatible, "wrong reply wire")
check(receivedReply(Data("{\"schema\":\(bridgeSchema),\"server_wire\":\(bridgeServerWire),\"status\":\"raw error\"}".utf8)).status == .failed, "untyped reply")

let ticket = try! JSONDecoder().decode(BridgeNavigationTicket.self,
    from: JSONSerialization.data(withJSONObject: navigationFixture()))
check(ticket.valid, "valid original lifetime/run ticket")
check(BridgeNavigationTicket.decode(userInfo: ticket.userInfo) == ticket, "native userInfo ticket round trip")
for (field, invalid) in [("server_socket", "relative"), ("callback_executable", "/bin/sh"),
                         ("callback_executable", "/tmp/../ovrcr"), ("server_lifetime", "other lifetime")] {
    var fixture = navigationFixture(); fixture[field] = invalid
    let value = try! JSONDecoder().decode(BridgeNavigationTicket.self,
        from: JSONSerialization.data(withJSONObject: fixture))
    check(!value.valid, "invalid callback field \(field)")
}
check(BridgeNavigationTicket.decode(userInfo: [navigationUserInfoKey: Data(repeating: 32, count: bridgeMaxBytes + 1)]) == nil,
      "callback userInfo frame bound")

// The test double is the native host executable boundary only. The actual
// forwarding function validates bytes and performs the real fixed-argv spawn.
func checkFixedCallbackHelper() {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("ovrcr-navigation-\(UUID().uuidString)")
    try! FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    defer { try! FileManager.default.removeItem(at: root) }
    let packaged = root.appendingPathComponent("packaged")
    let source = root.appendingPathComponent("original")
    let arbitrary = root.appendingPathComponent("arbitrary")
    for directory in [packaged, source, arbitrary] {
        try! FileManager.default.createDirectory(at: directory, withIntermediateDirectories: false)
    }
    let helper = packaged.appendingPathComponent("ovrcr")
    let original = source.appendingPathComponent("ovrcr")
    let unreviewed = arbitrary.appendingPathComponent("ovrcr")
    let script = """
    #!/bin/sh
    [ "$#" = 3 ] && [ "$1" = bridge ] && [ "$2" = navigate ] && [ "$3" = --stdin ] || exit 99
    printf '%s\\n' "$@" > "$0.argv"
    printf invoked > "$0.started"
    cat > "$0.input"
    printf '{"schema":\(bridgeSchema),"server_wire":\(bridgeServerWire),"applied":false,"activation":null}\\n'
    """
    try! Data(script.utf8).write(to: helper)
    try! FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)
    try! FileManager.default.copyItem(at: helper, to: original)
    try! Data("#!/bin/sh\nprintf arbitrary > \"$0.started\"\n".utf8).write(to: unreviewed)
    try! FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: unreviewed.path)
    let hash = navigationExecutableHash(helper.path, deadline: Deadline(seconds: 0.5))!
    let authority = TrustedNavigationHelper(executable: helper.path, sha256: hash)
    func value(_ path: String) -> BridgeNavigationTicket {
        BridgeNavigationTicket(schema: bridgeSchema, server_wire: bridgeServerWire,
            server_socket: root.appendingPathComponent("server.sock").path,
            callback_executable: path, callback_executable_sha256: hash, server_lifetime: ticket.server_lifetime, session: 1, run: 2)
    }
    check(forwardNavigation(value(unreviewed.path), deadline: Deadline(seconds: 0.5), helper: authority) == nil,
          "arbitrary same-UID executable named ovrcr rejected before any launch")
    check(!FileManager.default.fileExists(atPath: unreviewed.path + ".started")
          && !FileManager.default.fileExists(atPath: helper.path + ".started"), "rejected program launch witness absent")
    let result = forwardNavigation(value(original.path), deadline: Deadline(seconds: 0.5), helper: authority)
    check(result?.applied == false, "same reviewed bytes accepted after helper relocation")
    check(FileManager.default.fileExists(atPath: helper.path + ".started")
          && !FileManager.default.fileExists(atPath: original.path + ".started"), "only fixed packaged helper was launched")
    check(try! String(contentsOfFile: helper.path + ".argv", encoding: .utf8) == "bridge\nnavigate\n--stdin\n", "fixed callback argv")
    let received = try! JSONDecoder().decode(BridgeNavigationTicket.self, from: Data(contentsOf: URL(fileURLWithPath: helper.path + ".input")))
    check(received == value(original.path), "callback ticket travels only through bounded stdin")
    try! FileManager.default.removeItem(atPath: helper.path + ".started")
    try! Data("#!/bin/sh\nprintf replaced > \"$0.started\"\n".utf8).write(to: helper)
    check(forwardNavigation(value(original.path), deadline: Deadline(seconds: 0.5), helper: authority) == nil,
          "changed packaged helper fails sealed hash without fallback")
    check(!FileManager.default.fileExists(atPath: helper.path + ".started"), "replacement executable never launches")
}
checkFixedCallbackHelper()
let expired = BoundedReply(seconds: 0.005)
check(expired.wait().status == .failed, "bounded operation deadline")
check(!expired.perform { fatalError("late side effect") }, "late callback fenced")
let late = BoundedReply(seconds: 0)
late.finish(.available)
check(late.wait().status == .failed, "late completion rejected")
let completed = BoundedReply()
completed.finish(.available)
completed.finish(.denied)
check(completed.wait().status == .available, "completion exactly once")

checkSounds(URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true))

let name = "com.ovrcr.bridge.headless.\(getuid()).\(UUID().uuidString)"
guard let local = LocalBridgePort(name: name, handler: { data in
    switch admit(data) {
    case .request: return BridgeReply(.available).data()
    case .rejected(let failure): return BridgeReply(failure).data()
    }
}) else { fatalError("Cannot create owned local headless endpoint") }
guard let remote = remoteBridgePort(name) else { fatalError("Cannot connect owned remote headless endpoint") }
check(LocalBridgePort(name: name, handler: { _ in BridgeReply(.failed).data() }) == nil, "unique endpoint owner")
check(exchange(remote, data: request(["type": "status"]), deadline: Deadline(seconds: 0.5)).status == .available, "real CFMessagePort round trip")
for data in [Data(), Data("{}".utf8), Data("not JSON".utf8), request(["type": "invalid"])] {
    check(exchange(remote, data: data, deadline: Deadline(seconds: 0.5)).status == .failed, "real transport malformed admission")
}
check(exchange(remote, data: request(["type": "status"], wire: bridgeServerWire + 1), deadline: Deadline(seconds: 0.5)).status == .incompatible, "transport admission mismatch")
check(exchange(remote, data: Data(repeating: 0, count: bridgeMaxBytes + 1), deadline: Deadline(seconds: 0.5)).status == .failed, "transport request cap")
check(remoteBridgePort(name + ".missing") == nil, "absent endpoint")
let entered = DispatchSemaphore(value: 0)
let release = DispatchSemaphore(value: 0)
guard let slow = LocalBridgePort(name: name + ".slow", handler: { _ in
    entered.signal()
    _ = release.wait(timeout: .now() + 0.5)
    return BridgeReply(.available).data()
}), let slowRemote = remoteBridgePort(name + ".slow") else { fatalError("Cannot create owned timeout endpoint") }
let start = DispatchTime.now().uptimeNanoseconds
check(exchange(slowRemote, data: request(["type": "status"]), deadline: Deadline(seconds: 0.03)).status == .failed, "bounded receive timeout")
check(entered.wait(timeout: .now() + 0.1) == .success, "timeout request reached real endpoint")
release.signal()
check(Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000_000 < 0.5, "timeout wall bound")
withExtendedLifetime((local, slow)) {}
print("\(checks) headless checks passed; schema \(bridgeSchema), wire \(bridgeServerWire); no AppKit/UserNotifications lifecycle.")
