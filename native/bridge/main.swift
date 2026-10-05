import AppKit
import Darwin
import Foundation

func emit(_ reply: BridgeReply) {
    var data = reply.data()
    data.append(10)
    FileHandle.standardOutput.write(data)
}

func emit(_ status: BridgeStatus) { emit(BridgeReply(status)) }

let arguments = CommandLine.arguments
// These guards run before NSApplication, UserNotifications, launch or IPC.
if arguments.dropFirst().first == "--check-contract" {
    guard arguments.count == 4, let schema = UInt32(arguments[2]), let wire = UInt32(arguments[3]) else {
        emit(.failed); exit(64)
    }
    let matches = schema == bridgeSchema && wire == bridgeServerWire
    emit(matches ? .available : .incompatible)
    exit(matches ? 0 : 78)
}

if arguments.dropFirst().first == "--client" {
    guard arguments.count == 2 else { emit(.failed); exit(0) }
    let deadline = Deadline(seconds: 1.8) // Leave room inside the Dashboard's two-second bound.
    guard let input = boundedStdin(deadline) else { emit(.failed); exit(0) }
    if case .rejected(let failure) = admit(input) { emit(failure); exit(0) }
    var remote = remoteBridgePort(bridgePortName)
    if remote == nil {
        guard launchBridge(deadline) else { emit(.failed); exit(0) }
        while remote == nil && deadline.remaining > 0 {
            remote = remoteBridgePort(bridgePortName)
            if remote == nil { RunLoop.current.run(until: Date(timeIntervalSinceNow: min(0.01, deadline.remaining))) }
        }
    }
    emit(remote.map { exchange($0, data: input, deadline: deadline) } ?? BridgeReply(.failed))
    exit(0)
}

guard arguments.count == 1, Bundle.main.bundleIdentifier != nil else { emit(.failed); exit(64) }
var host: BridgeApp?
let hostLock = NSLock()
let launchReady = DispatchGroup()
launchReady.enter()
guard let listener = LocalBridgePort(name: bridgePortName, handler: { data in
    let deadline = Deadline(seconds: 0.65)
    guard launchReady.wait(timeout: .now() + deadline.remaining) == .success else {
        return BridgeReply(.failed).data()
    }
    hostLock.lock(); let current = host; hostLock.unlock()
    return current?.handle(data, deadline: deadline) ?? BridgeReply(.failed).data()
}) else { exit(0) } // Existing sender owns the endpoint; no second native owner.
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = BridgeApp()
delegate.onLaunch = { launchReady.leave() }
hostLock.lock(); host = delegate; hostLock.unlock()
app.delegate = delegate
withExtendedLifetime(listener) { app.run() }
