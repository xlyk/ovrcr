import AppKit

@MainActor
struct TerminalFontSettings {
    static let sizes = 8...32
    var family: String?
    var size: Int

    init(defaults: UserDefaults = .standard) {
        family = defaults.string(forKey: "terminalFontFamily")
        let savedSize = defaults.integer(forKey: "terminalFontSize")
        size = Self.sizes.contains(savedSize) ? savedSize : Int(NSFont.systemFontSize)
        if let family, Self.fixedPitchFont(family: family, size: size) == nil {
            self.family = nil
        }
    }

    static func fixedPitchFont(family: String, size: Int) -> NSFont? {
        let font = NSFontManager.shared.font(withFamily: family, traits: [], weight: 5, size: CGFloat(size))
        guard let font, font.isFixedPitch else { return nil }
        return font
    }

    func font(at pointSize: Int) -> NSFont {
        family.flatMap { Self.fixedPitchFont(family: $0, size: pointSize) }
            ?? .monospacedSystemFont(ofSize: CGFloat(pointSize), weight: .regular)
    }

    func save(to defaults: UserDefaults = .standard) {
        defaults.set(family, forKey: "terminalFontFamily")
        defaults.set(size, forKey: "terminalFontSize")
    }
}
