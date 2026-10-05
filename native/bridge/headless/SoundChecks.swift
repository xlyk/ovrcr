import Foundation

func checkSounds(_ repo: URL) {
    let manager = FileManager.default
    let temporary = manager.temporaryDirectory.appendingPathComponent("ovrcr-sound-checks-\(UUID().uuidString)", isDirectory: true)
    defer { try! manager.removeItem(at: temporary) }
    let bundle = temporary.appendingPathComponent("Fixture.app", isDirectory: true)
    let resources = bundle.appendingPathComponent("Contents/Resources", isDirectory: true)
    let lookup = temporary.appendingPathComponent("Library/Sounds", isDirectory: true)
    try! manager.createDirectory(at: resources, withIntermediateDirectories: true)
    try! manager.createDirectory(at: lookup, withIntermediateDirectories: true)
    func prepare(_ choice: ReadySoundChoice?) -> PreparedSound {
        prepareSound(choice, bundle: bundle, lookupDirectories: [lookup])
    }
    let display: [String: Any] = ["type": "deliver", "title": "OVRCR · response ready", "subtitle": "label", "body": "p / w / n (#1)", "navigation": navigationFixture()]
    let titles = ["OVRCR · response ready", "OVRCR · input needed"]
    for title in titles {
        for choice in ReadySoundChoice.allCases {
            var op = display; op["title"] = title; op["sound"] = choice.rawValue
            check(accepted(request(op)), "sound ID admitted \(choice.rawValue) for \(title)")
        }
    }
    var nullSound = display; nullSound["sound"] = NSNull()
    check(accepted(request(nullSound)), "null sound means silent")
    for value: Any in ["Glass", "/tmp/tap.wav", "ovrcr-tap-v1.wav", true, 7, ["tap"]] {
        var op = display; op["sound"] = value
        check(rejected(request(op), .failed), "non-enum sound rejected")
    }
    check(accepted(request(["type": "status", "sound": "/tmp/ignored"])), "control sound ignored")
    check(prepare(nil) == PreparedSound(plan: .silent, unavailable: nil), "absent sound stays silent")
    check(prepare(.default) == PreparedSound(plan: .systemDefault, unavailable: nil), "system default needs no resource")

    for choice in [ReadySoundChoice.tap, .chime, .rise] {
        let resource = choice.resource!
        let file = resources.appendingPathComponent(resource.file)
        let source = repo.appendingPathComponent("research/notification-bridge/sounds/\(resource.file)")
        let data = try! Data(contentsOf: source)
        func restore() { try! data.write(to: file); try! manager.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path) }
        check(prepare(choice).unavailable == .missingResource, "missing \(choice.rawValue)")
        restore()
        check(validPCM(data, frames: resource.frames), "approved PCM header/count \(choice.rawValue)")
        check(prepare(choice) == PreparedSound(plan: .named(resource.file), unavailable: nil), "exact regular hash resource \(choice.rawValue)")
        try! manager.setAttributes([.posixPermissions: 0o000], ofItemAtPath: file.path)
        check(prepare(choice) == PreparedSound(plan: .silent, unavailable: .unreadableResource), "unreadable \(choice.rawValue)")
        try! manager.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        var changed = data; changed[44] ^= 1; try! changed.write(to: file)
        check(validPCM(changed, frames: resource.frames), "changed sample retains PCM shape")
        check(prepare(choice).unavailable == .invalidResource, "changed approved hash \(choice.rawValue)")
        var corrupt = data; corrupt[20] = 3; try! corrupt.write(to: file)
        check(!validPCM(corrupt, frames: resource.frames), "invalid codec rejected")
        check(prepare(choice).unavailable == .invalidResource, "invalid PCM \(choice.rawValue)")
        try! Data([0]).write(to: file)
        check(prepare(choice).unavailable == .invalidResource, "invalid size bounded \(choice.rawValue)")
        try! manager.removeItem(at: file)
        try! manager.createSymbolicLink(at: file, withDestinationURL: source)
        check(prepare(choice).unavailable == .invalidResource, "linked resource rejected \(choice.rawValue)")
        try! manager.removeItem(at: file); restore()
        let competing = lookup.appendingPathComponent(resource.file)
        try! Data("conflict".utf8).write(to: competing)
        check(prepare(choice) == PreparedSound(plan: .silent, unavailable: .lookupConflict), "higher priority conflict \(choice.rawValue)")
        try! manager.removeItem(at: competing)
        check(prepare(.default).plan == .systemDefault && prepare(nil).plan == .silent, "failure never changes explicit default/silent")
    }
    let outside = temporary.appendingPathComponent("outside", isDirectory: true)
    try! manager.moveItem(at: resources, to: outside)
    try! manager.createSymbolicLink(at: resources, withDestinationURL: outside)
    check(prepare(.tap).unavailable == .invalidResource, "resource directory link cannot escape bundle")
    try! manager.removeItem(at: resources); try! manager.moveItem(at: outside, to: resources)

    for failure in BridgeSoundFailure.allCases {
        let submitted = BridgeReply(.submitted, soundUnavailable: failure)
        let decoded = receivedReply(submitted.data())
        check(decoded.status == .submitted && decoded.sound_unavailable == failure, "silent submitted reason \(failure.rawValue)")
        check(BridgeReply(.failed, soundUnavailable: failure).sound_unavailable == nil, "failed add not reported as submitted")
        let bounded = BoundedReply(); bounded.finish(.submitted, soundUnavailable: failure)
        check(bounded.wait().sound_unavailable == failure, "bounded adapter preserves reason")
        let expired = BoundedReply(seconds: 0); expired.finish(.submitted, soundUnavailable: failure)
        let late = expired.wait()
        check(late.status == .failed && late.sound_unavailable == nil, "expired add reports no submitted sound reason")
    }
    let encoded = String(data: BridgeReply(.submitted).data(), encoding: .utf8)!
    check(!encoded.contains("sound_unavailable") && !encoded.contains("heard"), "no sound failure/audibility claim when absent")
    let invalidReply = request(["type": "status"])
    check(receivedReply(invalidReply).status == .failed, "request is not a delivery reply")

    for value in ["null", "\"unknown\"", "true"] {
        let encoded = Data("{\"schema\":\(bridgeSchema),\"server_wire\":\(bridgeServerWire),\"status\":\"submitted\",\"sound_unavailable\":\(value)}".utf8)
        let reply = receivedReply(encoded)
        check(value == "null" ? reply.status == .submitted && reply.sound_unavailable == nil : reply.status == .failed,
            "optional sound reason strictly typed")
    }

    // Real unique transport, with a pure handler; 'submitted' models add success.
    let name = "com.ovrcr.sound.headless.\(UUID().uuidString)"
    let local = LocalBridgePort(name: name, handler: { data in
        guard case .request(let request) = admit(data), request.op.type == .deliver else { return BridgeReply(.failed).data() }
        let sound = prepare(request.op.sound)
        return BridgeReply(.submitted, soundUnavailable: sound.unavailable).data()
    })!
    let remote = remoteBridgePort(name)!
    for title in titles {
        for choice in ReadySoundChoice.allCases {
            var op = display; op["title"] = title; op["sound"] = choice.rawValue
            let reply = exchange(remote, data: request(op), deadline: Deadline(seconds: 0.5))
            check(reply.status == .submitted && reply.sound_unavailable == nil, "sound selection survives real IPC for \(title)")
        }
    }
    try! manager.removeItem(at: resources.appendingPathComponent("ovrcr-tap-v1.wav"))
    var op = display; op["sound"] = "tap"
    let failedSound = exchange(remote, data: request(op), deadline: Deadline(seconds: 0.5))
    check(failedSound.status == .submitted && failedSound.sound_unavailable == .missingResource, "silent missing resource reason survives real IPC")
    let tap = resources.appendingPathComponent("ovrcr-tap-v1.wav")
    let tapData = try! Data(contentsOf: repo.appendingPathComponent("research/notification-bridge/sounds/ovrcr-tap-v1.wav"))
    try! tapData.write(to: tap)
    try! manager.setAttributes([.posixPermissions: 0o000], ofItemAtPath: tap.path)
    check(exchange(remote, data: request(op), deadline: Deadline(seconds: 0.5)).sound_unavailable == .unreadableResource, "unreadable reason survives real IPC")
    try! manager.setAttributes([.posixPermissions: 0o644], ofItemAtPath: tap.path)
    try! Data([0]).write(to: tap)
    check(exchange(remote, data: request(op), deadline: Deadline(seconds: 0.5)).sound_unavailable == .invalidResource, "invalid reason survives real IPC")
    try! tapData.write(to: tap)
    try! Data([0]).write(to: lookup.appendingPathComponent("ovrcr-tap-v1.wav"))
    check(exchange(remote, data: request(op), deadline: Deadline(seconds: 0.5)).sound_unavailable == .lookupConflict, "lookup conflict reason survives real IPC")
    withExtendedLifetime(local) {}
}
