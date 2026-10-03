import CoreFoundation
import Darwin
import Foundation

let bridgePortName = "\(Bundle.main.bundleIdentifier ?? bridgeBundleID).user.\(getuid())"

final class LocalBridgePort {
    private var port: CFMessagePort?
    private let queue = DispatchQueue(label: "com.ovrcr.bridge.requests")
    private let handler: (Data) -> Data

    init?(name: String, handler: @escaping (Data) -> Data) {
        self.handler = handler
        var context = CFMessagePortContext(version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(), retain: nil, release: nil, copyDescription: nil)
        var reused: DarwinBoolean = false
        let candidate = CFMessagePortCreateLocal(nil, name as CFString, { _, message, payload, info in
            guard message == 1, let payload = payload, let info = info,
                  CFDataGetLength(payload) <= bridgeMaxBytes else {
                return Unmanaged.passRetained(BridgeReply(.failed).data() as CFData)
            }
            let owner = Unmanaged<LocalBridgePort>.fromOpaque(info).takeUnretainedValue()
            // Copy before any asynchronous native work; CF owns callback data.
            let data = Data(bytes: CFDataGetBytePtr(payload), count: CFDataGetLength(payload))
            return Unmanaged.passRetained(owner.handler(data) as CFData)
        }, &context, &reused)
        // A reused local port is somebody else's sender instance; do not invalidate it.
        guard let candidate = candidate, !reused.boolValue else { return nil }
        port = candidate
        CFMessagePortSetDispatchQueue(candidate, queue)
    }

    deinit { if let port = port { CFMessagePortInvalidate(port) } }
}

func remoteBridgePort(_ name: String) -> CFMessagePort? {
    CFMessagePortCreateRemote(nil, name as CFString)
}

func exchange(_ remote: CFMessagePort, data: Data, deadline: Deadline) -> BridgeStatus {
    guard data.count <= bridgeMaxBytes, deadline.remaining > 0 else { return .failed }
    let remaining = deadline.remaining
    let sendTimeout = min(0.1, remaining)
    var reply: Unmanaged<CFData>?
    let result = CFMessagePortSendRequest(remote, 1, data as CFData,
        sendTimeout, max(0, remaining - sendTimeout), CFRunLoopMode.defaultMode.rawValue, &reply)
    guard result == kCFMessagePortSuccess, let response = reply?.takeRetainedValue() else { return .failed }
    return receivedStatus(response as Data)
}

func boundedStdin(_ deadline: Deadline) -> Data? {
    var data = Data()
    var buffer = [UInt8](repeating: 0, count: 4096)
    while deadline.remaining > 0 {
        var descriptor = pollfd(fd: STDIN_FILENO, events: Int16(POLLIN), revents: 0)
        let result = poll(&descriptor, 1, Int32(max(1, deadline.remaining * 1000)))
        if result < 0 && errno == EINTR { continue }
        guard result > 0 else { return nil }
        let count = Darwin.read(STDIN_FILENO, &buffer, buffer.count)
        if count < 0 && errno == EINTR { continue }
        guard count >= 0 else { return nil }
        if count == 0 { return data }
        guard data.count + count <= bridgeMaxBytes else { return nil }
        data.append(contentsOf: buffer.prefix(count))
    }
    return nil
}
