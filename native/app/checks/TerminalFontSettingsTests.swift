import AppKit

@main
struct TerminalFontSettingsCheck {
    @MainActor
    static func main() {
        let suite = "dev.ovrcr.FontSettingsCheck." + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }

        var settings = TerminalFontSettings(defaults: defaults)
        precondition(settings.family == nil && settings.size == Int(NSFont.systemFontSize))
        settings.family = "Menlo"
        settings.size = 18
        settings.save(to: defaults)

        let saved = TerminalFontSettings(defaults: defaults)
        precondition(saved.family == "Menlo" && saved.size == 18)
        let zoomed = saved.font(at: 20)
        precondition(zoomed.familyName == "Menlo" && zoomed.pointSize == 20)
        for invalidFamily in ["Unavailable-Font-OVRCR", "Helvetica"] {
            defaults.set(invalidFamily, forKey: "terminalFontFamily")
            let fallback = TerminalFontSettings(defaults: defaults)
            precondition(fallback.family == nil && fallback.size == 18)
            precondition(fallback.font(at: 18) == NSFont.monospacedSystemFont(ofSize: 18, weight: .regular))
        }

        defaults.set(90, forKey: "terminalFontSize")
        precondition(TerminalFontSettings(defaults: defaults).size == Int(NSFont.systemFontSize))
    }
}
