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
    request(["type": "deliver", "title": "OVRCR · response ready", "subtitle": subtitle, "body": body])
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
for status in [BridgeStatus.available, .notDetermined, .permissionPending, .denied, .submitted, .settingsOpened, .incompatible, .failed] {
    check(receivedStatus(BridgeReply(status).data()) == status, "typed reply \(status.rawValue)")
}
check(receivedStatus(Data("{\"schema\":1,\"server_wire\":\(bridgeServerWire + 1),\"status\":\"available\"}".utf8)) == .incompatible, "wrong reply wire")
check(receivedStatus(Data("{\"schema\":1,\"server_wire\":\(bridgeServerWire),\"status\":\"raw error\"}".utf8)) == .failed, "untyped reply")
let expired = BoundedReply(seconds: 0.005)
check(expired.wait() == .failed, "bounded operation deadline")
check(!expired.perform { fatalError("late side effect") }, "late callback fenced")
let late = BoundedReply(seconds: 0)
late.finish(.available)
check(late.wait() == .failed, "late completion rejected")
let completed = BoundedReply()
completed.finish(.available)
completed.finish(.denied)
check(completed.wait() == .available, "completion exactly once")

let name = "com.ovrcr.bridge.headless.\(getuid()).\(UUID().uuidString)"
guard let local = LocalBridgePort(name: name, handler: { data in
    switch admit(data) {
    case .request: return BridgeReply(.available).data()
    case .rejected(let failure): return BridgeReply(failure).data()
    }
}) else { fatalError("Cannot create owned local headless endpoint") }
guard let remote = remoteBridgePort(name) else { fatalError("Cannot connect owned remote headless endpoint") }
check(LocalBridgePort(name: name, handler: { _ in BridgeReply(.failed).data() }) == nil, "unique endpoint owner")
check(exchange(remote, data: request(["type": "status"]), deadline: Deadline(seconds: 0.5)) == .available, "real CFMessagePort round trip")
for data in [Data(), Data("{}".utf8), Data("not JSON".utf8), request(["type": "invalid"])] {
    check(exchange(remote, data: data, deadline: Deadline(seconds: 0.5)) == .failed, "real transport malformed admission")
}
check(exchange(remote, data: request(["type": "status"], wire: bridgeServerWire + 1), deadline: Deadline(seconds: 0.5)) == .incompatible, "transport admission mismatch")
check(exchange(remote, data: Data(repeating: 0, count: bridgeMaxBytes + 1), deadline: Deadline(seconds: 0.5)) == .failed, "transport request cap")
check(remoteBridgePort(name + ".missing") == nil, "absent endpoint")
let entered = DispatchSemaphore(value: 0)
let release = DispatchSemaphore(value: 0)
guard let slow = LocalBridgePort(name: name + ".slow", handler: { _ in
    entered.signal()
    _ = release.wait(timeout: .now() + 0.5)
    return BridgeReply(.available).data()
}), let slowRemote = remoteBridgePort(name + ".slow") else { fatalError("Cannot create owned timeout endpoint") }
let start = DispatchTime.now().uptimeNanoseconds
check(exchange(slowRemote, data: request(["type": "status"]), deadline: Deadline(seconds: 0.03)) == .failed, "bounded receive timeout")
check(entered.wait(timeout: .now() + 0.1) == .success, "timeout request reached real endpoint")
release.signal()
check(Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000_000 < 0.5, "timeout wall bound")
withExtendedLifetime((local, slow)) {}
print("\(checks) headless checks passed; schema \(bridgeSchema), wire \(bridgeServerWire); no AppKit/UserNotifications lifecycle.")
