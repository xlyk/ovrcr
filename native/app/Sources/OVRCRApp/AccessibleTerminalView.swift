import AppKit
import SwiftTerm

@MainActor
final class AccessibleTerminalView: LocalProcessTerminalView {
    override func accessibilityIsIgnored() -> Bool { false }
    override func accessibilityRole() -> NSAccessibility.Role? { .textArea }
    override func accessibilityLabel() -> String? { "OVRCR Dashboard terminal" }

    override func accessibilityValue() -> Any? {
        let rows = terminalDimensions.rows
        return visibleRowsText(0..<rows).joined(separator: "\n")
    }
}
