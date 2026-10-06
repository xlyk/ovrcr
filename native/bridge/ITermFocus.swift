import AppKit
import CoreServices
import Darwin
import Foundation

// Source-only #225 adapter. It has no callers in the accepted #222 bundle.
// The caller supplies the CURRENT owner's applied-ack target, runs off the main
// thread, and keeps its external deadline/cancellation boundary. See the dated
// research contract for sender purpose/entitlement and pending native acceptance.
struct ITermFocusTarget {
    let dashboardPID: Int32
    let dashboardStartSeconds: UInt64
    let dashboardStartMicroseconds: UInt64
    let itermSessionID: String
}

enum ITermFocusResult: Equatable {
    case authorized
    case selectionRequested // Accepted requests and matching metadata, not observed focus.
    case notEnabled
    case unavailable
    case invalidIdentity
    case ownerChanged
    case authorizationRequired
    case denied
    case missingSession
    case timedOut
    case failed
}

enum ITermFocus {
    // iTerm captures w/t/p positions at launch; those can become stale after a
    // move. Only the bounded UUID suffix is used for live metadata matching.
    static func sessionGUID(_ value: String) -> String? {
        let bytes = Array(value.utf8)
        guard bytes.count <= 128, let colon = bytes.firstIndex(of: 58),
              bytes.lastIndex(of: 58) == colon else { return nil }
        let prefix = Array(bytes[..<colon])
        var index = 0
        for marker in [UInt8(119), UInt8(116), UInt8(112)] { // w, t, p
            guard index < prefix.count, prefix[index] == marker else { return nil }
            index += 1
            let start = index
            while index < prefix.count, (48...57).contains(prefix[index]) { index += 1 }
            guard index > start else { return nil }
        }
        guard index == prefix.count else { return nil }
        let guid = String(decoding: bytes[(colon + 1)...], as: UTF8.self)
        return normalizedGUID(guid)
    }

    // Explicit future setup only. Never call from notification enabling, a
    // banner callback, a status-poll loop, or an automatic retry. No AE is sent.
    static func requestAuthorization(for target: ITermFocusTarget,
                                     ownerIsCurrent: @escaping () -> Bool,
                                     admission: () -> Bool) -> ITermFocusResult {
        guard !Thread.isMainThread else { return .unavailable }
        do {
            let context = try Context(target, ownerIsCurrent: ownerIsCurrent)
            try context.validate()
            let result = permission(context, ask: true, admission: admission)
            try context.validate()
            return result
        } catch let error as Failure { return error.result }
        catch { return .failed }
    }

    // A prior OS grant is not implicit product opt-in. All actual AE sends,
    // including read-only queries, refuse consent prompts independently of the
    // preflight so a later permission change cannot initiate authorization.
    static func selectExistingSession(for target: ITermFocusTarget,
                                      explicitlyEnabled: Bool,
                                      ownerIsCurrent: @escaping () -> Bool) -> ITermFocusResult {
        guard explicitlyEnabled else { return .notEnabled }
        guard !Thread.isMainThread else { return .unavailable }
        do {
            let context = try Context(target, ownerIsCurrent: ownerIsCurrent)
            let deadline = ProcessInfo.processInfo.systemUptime + 2
            let preflight = permission(context, ask: false)
            guard preflight == .authorized else { return preflight }
            let sender = Sender(context: context, deadline: deadline)
            let match = try findSession(sender)
            let session = try object(sessionClass, in: match.tab, form: OSType(formUniqueID),
                                     key: NSAppleEventDescriptor(string: match.guid))
            try requireMatchingSession(session, sender: sender)
            try sender.select(session)
            try requireCurrentSession(in: match.tab, sender: sender)
            try sender.select(match.tab)
            try requireCurrentSession(in: match.window, sender: sender)
            try sender.select(match.window)
            try requireCurrentSession(in: match.window, sender: sender)
            try context.validate()
            guard sender.remaining > 0 else { throw Failure(.timedOut) }
            guard context.application.activate(options: []) else { return .failed }
            return .selectionRequested
        } catch let error as Failure { return error.result }
        catch { return .failed }
    }

    private static let bundleID = "com.googlecode.iterm2"
    private static let windowClass: OSType = 0x6377696e // cwin
    private static let tabClass: OSType = 0x54726d74 // Trmt
    private static let sessionClass: OSType = 0x54726d73 // Trms
    private static let sessionIDProperty: OSType = 0x556e6971 // Uniq -> guid
    private static let ttyProperty: OSType = 0x53747479 // Stty
    private static let currentSessionProperty: OSType = 0x5763736e // Wcsn
    private static let selectClass: OSType = 0x4974726d // Itrm
    private static let selectEvent: OSType = 0x736c6374 // slct

    private struct Failure: Error {
        let result: ITermFocusResult
        init(_ result: ITermFocusResult) { self.result = result }
    }

    private static func normalizedGUID(_ value: String) -> String? {
        let bytes = Array(value.utf8)
        guard bytes.count == 36 else { return nil }
        for (index, byte) in bytes.enumerated() {
            if [8, 13, 18, 23].contains(index) {
                guard byte == 45 else { return nil }
            } else {
                guard (48...57).contains(byte) || (65...70).contains(byte)
                    || (97...102).contains(byte) else { return nil }
            }
        }
        guard UUID(uuidString: value) != nil else { return nil }
        return value.lowercased()
    }

    private static func process(_ pid: Int32) throws -> proc_bsdinfo {
        guard pid > 1 else { throw Failure(.invalidIdentity) }
        var info = proc_bsdinfo()
        let size = Int32(MemoryLayout<proc_bsdinfo>.size)
        guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
              info.pbi_pid == UInt32(pid), info.pbi_uid == getuid(),
              info.pbi_status != 5 else { throw Failure(.ownerChanged) } // SZOMB
        return info
    }

    private static func parentApplication(_ target: ITermFocusTarget)
        throws -> (application: NSRunningApplication, ancestry: BridgeParentAncestry) {
        let dashboard = try process(target.dashboardPID)
        let lookup = BridgeProcessAncestry.liveLookup(userID: getuid())
        guard let start = lookup.owned(dashboard.pbi_pid),
              start.startSeconds == target.dashboardStartSeconds,
              start.startMicroseconds == target.dashboardStartMicroseconds,
              let ancestry = BridgeProcessAncestry.itermApplication(from: start, userID: getuid(),
                  maximumDepth: 64, lookup: lookup),
              let app = NSRunningApplication(processIdentifier: Int32(ancestry.application.pid)),
              !app.isTerminated, app.activationPolicy != .prohibited,
              app.bundleIdentifier == bundleID,
              app.executableURL?.path == ancestry.application.executable else { throw Failure(.unavailable) }
        return (app, ancestry)
    }

    private struct Context {
        let target: ITermFocusTarget
        let guid: String
        let application: NSRunningApplication
        let address: NSAppleEventDescriptor
        let applicationStartSeconds: UInt64
        let applicationStartMicroseconds: UInt64
        let ttyDevice: UInt32
        let ancestry: BridgeParentAncestry
        let ownerIsCurrent: () -> Bool

        init(_ target: ITermFocusTarget, ownerIsCurrent: @escaping () -> Bool) throws {
            guard ownerIsCurrent() else { throw Failure(.ownerChanged) }
            self.ownerIsCurrent = ownerIsCurrent
            guard let guid = sessionGUID(target.itermSessionID),
                  target.dashboardStartSeconds > 0,
                  target.dashboardStartMicroseconds < 1_000_000 else {
                throw Failure(.invalidIdentity)
            }
            self.target = target
            self.guid = guid
            let dashboard = try process(target.dashboardPID)
            guard dashboard.pbi_start_tvsec == target.dashboardStartSeconds,
                  dashboard.pbi_start_tvusec == target.dashboardStartMicroseconds,
                  dashboard.e_tdev != UInt32.max else { throw Failure(.ownerChanged) }
            ttyDevice = dashboard.e_tdev
            let parent = try parentApplication(target)
            application = parent.application
            ancestry = parent.ancestry
            let app = try process(application.processIdentifier)
            applicationStartSeconds = app.pbi_start_tvsec
            applicationStartMicroseconds = app.pbi_start_tvusec
            // PID addressing cannot launch a missing app or choose a different
            // concurrently running iTerm instance by bundle identifier.
            address = NSAppleEventDescriptor(processIdentifier: application.processIdentifier)
        }

        func validate() throws {
            guard ownerIsCurrent() else { throw Failure(.ownerChanged) }
            let dashboard = try process(target.dashboardPID)
            let app = try process(application.processIdentifier)
            let parent = try parentApplication(target)
            guard dashboard.pbi_start_tvsec == target.dashboardStartSeconds,
                  dashboard.pbi_start_tvusec == target.dashboardStartMicroseconds,
                  dashboard.e_tdev == ttyDevice,
                  app.pbi_start_tvsec == applicationStartSeconds,
                  app.pbi_start_tvusec == applicationStartMicroseconds,
                  parent.application.processIdentifier == application.processIdentifier,
                  parent.ancestry == ancestry else {
                throw Failure(.ownerChanged)
            }
        }
    }

    private static func permission(_ context: Context, ask: Bool,
                                   admission: () -> Bool = { true }) -> ITermFocusResult {
        do { try context.validate() } catch { return .ownerChanged }
        guard #available(macOS 10.14, *), let address = context.address.aeDesc else {
            return .unavailable
        }
        // Context validation may itself be slow. Check client admission again
        // immediately before entering the only API that may initiate consent.
        guard admission() else { return .timedOut }
        let status = AEDeterminePermissionToAutomateTarget(address, typeWildCard, typeWildCard, ask)
        if status == noErr { return .authorized }
        return failure(status)
    }

    private static func failure(_ status: OSStatus) -> ITermFocusResult {
        switch status {
        case -1744: return .authorizationRequired
        case -1743: return .denied
        case -600, -1742: return .unavailable
        case -1728: return .missingSession
        case -1712: return .timedOut
        default: return .failed
        }
    }

    private struct Sender {
        let context: Context
        let deadline: TimeInterval
        var remaining: TimeInterval { deadline - ProcessInfo.processInfo.systemUptime }

        func send(_ eventClass: OSType, _ eventID: OSType,
                  object: NSAppleEventDescriptor) throws -> NSAppleEventDescriptor {
            try context.validate()
            guard #available(macOS 10.14, *), remaining > 0 else { throw Failure(.timedOut) }
            let event = NSAppleEventDescriptor(eventClass: eventClass, eventID: eventID,
                targetDescriptor: context.address, returnID: AEReturnID(kAutoGenerateReturnID),
                transactionID: AETransactionID(kAnyTransactionID))
            event.setParam(object, forKeyword: keyDirectObject)
            guard let descriptor = event.aeDesc else { throw Failure(.failed) }
            var reply = AppleEvent(descriptorType: typeNull, dataHandle: nil)
            let mode = AESendMode(kAEWaitReply) | AESendMode(kAENeverInteract)
                | AESendMode(kAEDoNotPromptForUserConsent)
            let ticks = Int(max(0, min(120, remaining * 60)))
            guard ticks > 0 else { throw Failure(.timedOut) }
            let status = AESendMessage(descriptor, &reply, mode, ticks)
            guard status == noErr else {
                _ = AEDisposeDesc(&reply)
                throw Failure(failure(status))
            }
            let size = AESizeOfFlattenedDesc(&reply)
            guard size > 0, size <= 65_536 else {
                _ = AEDisposeDesc(&reply)
                throw Failure(.failed)
            }
            // Ownership of reply's dataHandle passes to the Foundation wrapper.
            let result = NSAppleEventDescriptor(aeDescNoCopy: &reply)
            if let error = result.paramDescriptor(forKeyword: keyErrorNumber), error.int32Value != 0 {
                throw Failure(failure(error.int32Value))
            }
            guard remaining > 0 else { throw Failure(.timedOut) }
            return result
        }

        func get(_ object: NSAppleEventDescriptor) throws -> NSAppleEventDescriptor {
            let reply = try send(kAECoreSuite, kAEGetData, object: object)
            guard let data = reply.paramDescriptor(forKeyword: keyDirectObject) else {
                throw Failure(.failed)
            }
            return data
        }

        func select(_ object: NSAppleEventDescriptor) throws {
            _ = try send(selectClass, selectEvent, object: object)
        }
    }

    private static func object(_ desiredClass: OSType, in container: NSAppleEventDescriptor,
                               form: OSType, key: NSAppleEventDescriptor) throws -> NSAppleEventDescriptor {
        let record = NSAppleEventDescriptor.record()
        record.setDescriptor(NSAppleEventDescriptor(typeCode: desiredClass), forKeyword: AEKeyword(keyAEDesiredClass))
        record.setDescriptor(container, forKeyword: AEKeyword(keyAEContainer))
        record.setDescriptor(NSAppleEventDescriptor(enumCode: form), forKeyword: AEKeyword(keyAEKeyForm))
        record.setDescriptor(key, forKeyword: AEKeyword(keyAEKeyData))
        guard let specifier = record.coerce(toDescriptorType: typeObjectSpecifier) else {
            throw Failure(.failed)
        }
        return specifier
    }

    private static func all(_ desiredClass: OSType,
                            in container: NSAppleEventDescriptor) throws -> NSAppleEventDescriptor {
        try object(desiredClass, in: container, form: OSType(formAbsolutePosition),
                   key: NSAppleEventDescriptor(enumCode: OSType(kAEAll)))
    }

    private static func property(_ code: OSType,
                                 of container: NSAppleEventDescriptor) throws -> NSAppleEventDescriptor {
        try object(cProperty, in: container, form: OSType(formPropertyID),
                   key: NSAppleEventDescriptor(typeCode: code))
    }

    private static func list(_ data: NSAppleEventDescriptor, limit: Int) throws -> [NSAppleEventDescriptor] {
        guard data.descriptorType == typeAEList, data.numberOfItems <= limit else {
            throw Failure(.failed)
        }
        var items = [NSAppleEventDescriptor]()
        for index in 0..<data.numberOfItems {
            guard let item = data.atIndex(index + 1) else { throw Failure(.failed) }
            items.append(item)
        }
        return items
    }

    private struct Match {
        let window: NSAppleEventDescriptor
        let tab: NSAppleEventDescriptor
        let guid: String // Preserve the live property's case for iTerm's lookup.
    }

    private static func findSession(_ sender: Sender) throws -> Match {
        let windows = try list(sender.get(all(windowClass, in: .null())), limit: 64)
        var match: Match?
        var tabCount = 0
        var sessionCount = 0
        for window in windows {
            guard window.descriptorType == typeObjectSpecifier else { throw Failure(.failed) }
            for tab in try list(sender.get(all(tabClass, in: window)), limit: 256) {
                tabCount += 1
                guard tabCount <= 256, tab.descriptorType == typeObjectSpecifier else { throw Failure(.failed) }
                let ids = try list(sender.get(property(sessionIDProperty,
                    of: all(sessionClass, in: tab))), limit: 1024)
                sessionCount += ids.count
                guard sessionCount <= 1024 else { throw Failure(.failed) }
                for id in ids {
                    guard let value = id.stringValue, let guid = normalizedGUID(value) else {
                        throw Failure(.failed)
                    }
                    if guid == sender.context.guid {
                        guard match == nil else { throw Failure(.failed) }
                        match = Match(window: window, tab: tab, guid: value)
                    }
                }
            }
        }
        guard let result = match else { throw Failure(.missingSession) }
        return result
    }

    private static func requireMatchingSession(_ session: NSAppleEventDescriptor,
                                               sender: Sender) throws {
        let id = try sender.get(property(sessionIDProperty, of: session))
        guard let value = id.stringValue, normalizedGUID(value) == sender.context.guid else {
            throw Failure(.missingSession)
        }
        let tty = try sender.get(property(ttyProperty, of: session))
        guard let path = tty.stringValue, path.utf8.count <= 64, path.hasPrefix("/dev/ttys"),
              path.utf8.dropFirst(9).allSatisfy({ (48...57).contains($0) || (65...90).contains($0)
                  || (97...122).contains($0) }) else { throw Failure(.missingSession) }
        var device = stat()
        guard lstat(path, &device) == 0, device.st_mode & S_IFMT == S_IFCHR,
              UInt32(bitPattern: device.st_rdev) == sender.context.ttyDevice else {
            throw Failure(.missingSession)
        }
    }

    private static func requireCurrentSession(in container: NSAppleEventDescriptor,
                                              sender: Sender) throws {
        let current = try property(currentSessionProperty, of: container)
        try requireMatchingSession(current, sender: sender)
    }
}
