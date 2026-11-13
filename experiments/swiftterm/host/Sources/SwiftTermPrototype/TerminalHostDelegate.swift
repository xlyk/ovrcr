import AppKit
import SwiftTerm

@MainActor
final class TerminalHostDelegate: TerminalViewDelegate {
    weak var terminalView: LocalProcessTerminalView?
    weak var window: NSWindow?

    func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        guard let terminalView else { return }
        terminalView.sizeChanged(source: terminalView, newCols: newCols, newRows: newRows)
    }

    func setTerminalTitle(source: TerminalView, title: String) {
        window?.title = title
    }

    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

    func send(source: TerminalView, data: ArraySlice<UInt8>) {
        guard let terminalView else { return }
        terminalView.send(source: terminalView, data: data)
    }

    func scrolled(source: TerminalView, position: Double) {}

    func clipboardCopy(source: TerminalView, content: Data) {
        guard content.count <= 64 * 1024,
              let string = String(data: content, encoding: .utf8) else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(string, forType: .string)
    }

    func clipboardRead(source: TerminalView) -> Data? { nil }

    func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
}
