import Foundation

let bridgeBundleID = "com.ovrcr.bridge"
let bridgeMaxBytes = 1_048_576

enum BridgeStatus: String, Codable {
    case available, notDetermined = "not_determined", permissionPending = "permission_pending"
    case denied, submitted, settingsOpened = "settings_opened", incompatible, failed
}

struct BridgeReply: Codable {
    let schema: UInt32
    let server_wire: UInt32
    let status: BridgeStatus

    init(_ status: BridgeStatus) {
        schema = bridgeSchema
        server_wire = bridgeServerWire
        self.status = status
    }

    func data() -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = .sortedKeys
        return try! encoder.encode(self)
    }
}

struct BridgeVersions: Decodable {
    let schema: UInt32
    let server_wire: UInt32
}

struct BridgeOperation: Decodable {
    enum Kind: String, Decodable { case status, authorize, settings, deliver }
    let type: Kind
    let title: String?
    let subtitle: String?
    let body: String?

    enum CodingKeys: String, CodingKey { case type, title, subtitle, body }
    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        type = try values.decode(Kind.self, forKey: .type)
        title = type == .deliver ? try values.decode(String.self, forKey: .title) : nil
        subtitle = type == .deliver ? try values.decode(String.self, forKey: .subtitle) : nil
        body = type == .deliver ? try values.decode(String.self, forKey: .body) : nil
    }
}

struct BridgeRequest: Decodable {
    let schema: UInt32
    let server_wire: UInt32
    let op: BridgeOperation
}

enum Admission {
    case request(BridgeRequest)
    case rejected(BridgeStatus)
}

func safeBannerText(_ text: String, bytes: Int) -> Bool {
    guard text.utf8.count <= bytes else { return false }
    return text.unicodeScalars.allSatisfy { scalar in
        let value = scalar.value
        return value >= 32 && !(127...159).contains(value)
            && ![0x061C, 0x200E, 0x200F, 0x2028, 0x2029].contains(value)
            && !(0x202A...0x202E).contains(value) && !(0x2066...0x2069).contains(value)
    }
}

func admit(_ data: Data) -> Admission {
    guard !data.isEmpty, data.count <= bridgeMaxBytes,
          let versions = try? JSONDecoder().decode(BridgeVersions.self, from: data) else {
        return .rejected(.failed)
    }
    guard versions.schema == bridgeSchema, versions.server_wire == bridgeServerWire else {
        return .rejected(.incompatible)
    }
    guard let request = try? JSONDecoder().decode(BridgeRequest.self, from: data) else {
        return .rejected(.failed)
    }
    if request.op.type == .deliver {
        guard let title = request.op.title, let subtitle = request.op.subtitle,
              let body = request.op.body,
              ["OVRCR · response ready", "OVRCR · input needed"].contains(title),
              safeBannerText(title, bytes: 96), safeBannerText(subtitle, bytes: 320),
              safeBannerText(body, bytes: 1024) else { return .rejected(.failed) }
    }
    return .request(request)
}

func receivedStatus(_ data: Data) -> BridgeStatus {
    guard data.count <= bridgeMaxBytes,
          let reply = try? JSONDecoder().decode(BridgeReply.self, from: data) else { return .failed }
    guard reply.schema == bridgeSchema, reply.server_wire == bridgeServerWire else { return .incompatible }
    return reply.status
}

struct Deadline {
    let end: UInt64
    init(seconds: Double) { end = DispatchTime.now().uptimeNanoseconds + UInt64(seconds * 1_000_000_000) }
    var remaining: Double {
        let now = DispatchTime.now().uptimeNanoseconds
        return now < end ? Double(end - now) / 1_000_000_000 : 0
    }
}

// One bounded operation. A late OS callback cannot start a new side effect.
final class BoundedReply {
    private let lock = NSLock()
    private let ready = DispatchSemaphore(value: 0)
    private let deadline: Deadline
    private var active = true
    private var result: BridgeStatus = .failed

    init(seconds: Double = 0.65) { deadline = Deadline(seconds: seconds) }

    func perform(_ effect: () -> Void) -> Bool {
        lock.lock()
        let allowed = active && deadline.remaining > 0
        lock.unlock()
        guard allowed else { return false }
        // Admission is the side-effect start; never hold a lock across an OS API.
        effect()
        return true
    }

    var isFinished: Bool {
        lock.lock(); defer { lock.unlock() }
        return !active
    }

    func finish(_ status: BridgeStatus) {
        lock.lock()
        guard active else { lock.unlock(); return }
        result = deadline.remaining > 0 ? status : .failed
        active = false
        lock.unlock()
        ready.signal()
    }

    func wait() -> BridgeStatus {
        _ = ready.wait(timeout: .now() + deadline.remaining)
        lock.lock()
        active = false
        let status = result
        lock.unlock()
        return status
    }
}
