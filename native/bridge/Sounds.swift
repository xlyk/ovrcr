import CryptoKit
import Foundation

enum SoundPlan: Equatable { case silent, systemDefault, named(String) }
struct PreparedSound: Equatable {
    let plan: SoundPlan
    let unavailable: BridgeSoundFailure?
}

extension ReadySoundChoice {
    var resource: (file: String, frames: Int, sha256: String)? {
        switch self {
        case .default: return nil
        case .tap: return ("ovrcr-tap-v1.wav", 10560, "cfd6bec99cb836b6289501789d2624cf978145d036498374dea16013144e4624")
        case .chime: return ("ovrcr-chime-v1.wav", 40320, "c08d8450e8c74d026cf110889ac3ed12f62b60bd3d1a728632669c0f78f5ed53")
        case .rise: return ("ovrcr-rise-v1.wav", 30720, "fbfdb5a4e263a0569fff8aa8f8bb7f986bf18c90e7bfbf512b1f9267ded49bb1")
        }
    }
}

// Foundation supplies the sandbox home, or user home for a nonsandboxed Mac app.
// This app is built without app-group entitlements or additional sound containers.
func soundLookupDirectories() -> [URL] {
    [URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
        .appendingPathComponent("Library/Sounds", isDirectory: true)]
}

private func missing(_ error: Error) -> Bool {
    let error = error as NSError
    return error.domain == NSCocoaErrorDomain
        && [NSFileNoSuchFileError, NSFileReadNoSuchFileError].contains(error.code)
}

func validPCM(_ data: Data, frames: Int) -> Bool {
    guard data.count == 44 + frames * 2 else { return false }
    func text(_ offset: Int, _ count: Int) -> String? {
        String(data: data.subdata(in: offset..<(offset + count)), encoding: .ascii)
    }
    func word(_ offset: Int, _ count: Int) -> Int {
        (0..<count).reduce(0) { $0 | (Int(data[offset + $1]) << ($1 * 8)) }
    }
    return text(0, 4) == "RIFF" && word(4, 4) == data.count - 8 && text(8, 4) == "WAVE"
        && text(12, 4) == "fmt " && word(16, 4) == 16 && word(20, 2) == 1
        && word(22, 2) == 1 && word(24, 4) == 48000 && word(28, 4) == 96000
        && word(32, 2) == 2 && word(34, 2) == 16 && text(36, 4) == "data"
        && word(40, 4) == frames * 2 && Double(frames) / 48000 < 1.2
}

// No sound object, playback, native center or caller-provided path is involved.
func prepareSound(_ choice: ReadySoundChoice?, bundle: URL, lookupDirectories: [URL]) -> PreparedSound {
    guard let choice = choice else { return PreparedSound(plan: .silent, unavailable: nil) }
    guard let resource = choice.resource else { return PreparedSound(plan: .systemDefault, unavailable: nil) }
    let manager = FileManager.default
    let contents = bundle.appendingPathComponent("Contents", isDirectory: true)
    let resources = contents.appendingPathComponent("Resources", isDirectory: true)
    let file = resources.appendingPathComponent(resource.file)
    func unavailable(_ failure: BridgeSoundFailure) -> PreparedSound {
        PreparedSound(plan: .silent, unavailable: failure)
    }
    do {
        for directory in [bundle, contents, resources] {
            let attributes = try manager.attributesOfItem(atPath: directory.path)
            guard attributes[.type] as? FileAttributeType == .typeDirectory else { return unavailable(.invalidResource) }
        }
        let attributes = try manager.attributesOfItem(atPath: file.path)
        guard attributes[.type] as? FileAttributeType == .typeRegular else { return unavailable(.invalidResource) }
        guard manager.isReadableFile(atPath: file.path) else { return unavailable(.unreadableResource) }
        guard attributes[.size] as? Int == 44 + resource.frames * 2 else { return unavailable(.invalidResource) }
        let handle = try FileHandle(forReadingFrom: file)
        defer { try? handle.close() }
        var data = Data()
        let limit = 44 + resource.frames * 2
        while data.count <= limit {
            guard let part = try handle.read(upToCount: limit + 1 - data.count), !part.isEmpty else { break }
            data.append(part)
        }
        guard validPCM(data, frames: resource.frames),
              SHA256.hash(data: data).map({ String(format: "%02x", $0) }).joined() == resource.sha256 else {
            return unavailable(.invalidResource)
        }
    } catch { return unavailable(missing(error) ? .missingResource : .unreadableResource) }
    for directory in lookupDirectories {
        do {
            _ = try manager.attributesOfItem(atPath: directory.appendingPathComponent(resource.file).path)
            return unavailable(.lookupConflict)
        } catch {
            if !missing(error) { return unavailable(.unreadableResource) }
        }
    }
    return PreparedSound(plan: .named(resource.file), unavailable: nil)
}
