import AppKit
import SwiftTerm

@main
struct SwiftTermPrototype {
    @MainActor
    static func main() {
        let application = NSApplication.shared
        let delegate = AppDelegate()
        application.delegate = delegate
        withExtendedLifetime(delegate) {
            application.run()
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate, LocalProcessTerminalViewDelegate {
    private var window: NSWindow?
    private var terminalView: LocalProcessTerminalView?
    private var terminalHostDelegate: TerminalHostDelegate?
    private var configuration: HostConfiguration?
    private var dashboardStartRequested = false
    private var dashboardStarted = false
    private var terminationRequested = false
    private var pendingRuntimeAlert: (title: String, message: String)?
    private var fontSettings = TerminalFontSettings()

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)

        let hostExecutable = Bundle.main.executableURL
            ?? URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL
        do {
            configuration = try HostConfiguration.load(
                environment: ProcessInfo.processInfo.environment,
                hostExecutable: hostExecutable,
                currentDirectory: FileManager.default.currentDirectoryPath
            )
        } catch {
            showLaunchError(error)
            return
        }

        let view = AccessibleTerminalView(frame: .zero, font: fontSettings.font(at: fontSettings.size), options: .default)
        view.optionAsMetaKey = false
        view.optionAsMetaKeyForFunctionalKeys = true
        view.pressAndHoldShowsAccentMenu = false
        view.allowMouseReporting = true
        terminalView = view

        let hostDelegate = TerminalHostDelegate()
        hostDelegate.terminalView = view
        terminalHostDelegate = hostDelegate
        view.terminalDelegate = hostDelegate
        view.processDelegate = self

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1000, height: 700),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "SwiftTerm Prototype"
        window.center()
        window.delegate = self
        window.contentView = view
        view.frame = window.contentView?.bounds ?? .zero
        view.autoresizingMask = [.width, .height]
        hostDelegate.window = window
        self.window = window

        installMenu(for: view, window: window)
        showTerminalWindow()
        DispatchQueue.main.async { [weak self] in
            self?.startDashboard()
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showTerminalWindow()
        presentPendingRuntimeAlert()
        return true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        terminationRequested = true
        guard dashboardStarted, let terminalView,
              terminalView.process.running || terminalView.process.windingDown else {
            return .terminateNow
        }
        terminalView.terminate()
        return .terminateLater
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        guard !terminationRequested else { return true }
        sender.orderOut(nil)
        return false
    }

    func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}

    func setTerminalTitle(source: LocalProcessTerminalView, title: String) {
        window?.title = title
    }

    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}

    func processTerminated(source: TerminalView, exitCode: Int32?) {
        if terminationRequested {
            NSApp.reply(toApplicationShouldTerminate: true)
            return
        }

        let status = exitCode.map(String.init) ?? "terminated by a signal"
        pendingRuntimeAlert = (
            title: "Dashboard exited",
            message: "The OVRCR Dashboard process exited with status \(status). No replacement Dashboard was started."
        )
        presentPendingRuntimeAlert()
    }

    func processFailedToStart(source: TerminalView, error: LocalProcessError) {
        pendingRuntimeAlert = (
            title: "Dashboard could not start",
            message: "SwiftTerm could not start the OVRCR Dashboard process: \(error)"
        )
        presentPendingRuntimeAlert()
    }

    @objc private func showSettings(_ sender: Any?) {
        guard let window else { return }
        showTerminalWindow()
        guard window.attachedSheet == nil else { return }

        let family = NSPopUpButton()
        family.addItem(withTitle: "System Monospaced")
        let families = NSFontManager.shared.availableFontFamilies.filter {
            TerminalFontSettings.fixedPitchFont(family: $0, size: fontSettings.size) != nil
        }.sorted { $0.localizedStandardCompare($1) == .orderedAscending }
        family.addItems(withTitles: families)
        if let selected = fontSettings.family { family.selectItem(withTitle: selected) }
        family.setAccessibilityLabel("Font family")

        let size = NSPopUpButton()
        for points in TerminalFontSettings.sizes {
            size.addItem(withTitle: "\(points) pt")
            size.lastItem?.tag = points
        }
        size.selectItem(withTag: fontSettings.size)
        size.setAccessibilityLabel("Font size")

        let grid = NSGridView(views: [
            [NSTextField(labelWithString: "Font"), family],
            [NSTextField(labelWithString: "Size"), size],
        ])
        grid.column(at: 0).xPlacement = .trailing
        grid.setFrameSize(grid.fittingSize)
        let sheet = NSAlert()
        sheet.messageText = "Terminal Settings"
        sheet.accessoryView = grid
        sheet.addButton(withTitle: "Apply")
        sheet.addButton(withTitle: "Cancel")
        sheet.beginSheetModal(for: window) { [self] response in
            guard response == .alertFirstButtonReturn else { return }
            fontSettings.family = family.indexOfSelectedItem == 0 ? nil : family.titleOfSelectedItem
            fontSettings.size = size.selectedTag()
            fontSettings.save()
            setFontSize(fontSettings.size)
        }
    }

    @objc private func zoomIn(_ sender: Any?) {
        guard let terminalView else { return }
        setFontSize(min(Int(terminalView.font.pointSize) + 1, TerminalFontSettings.sizes.upperBound))
    }

    @objc private func zoomOut(_ sender: Any?) {
        guard let terminalView else { return }
        setFontSize(max(Int(terminalView.font.pointSize) - 1, TerminalFontSettings.sizes.lowerBound))
    }

    @objc private func zoomReset(_ sender: Any?) {
        setFontSize(fontSettings.size)
    }

    private func installMenu(for terminalView: LocalProcessTerminalView, window: NSWindow) {
        let mainMenu = NSMenu()

        let appMenuItem = NSMenuItem()
        let appMenu = NSMenu(title: "SwiftTerm Prototype")
        appMenu.addItem(NSMenuItem(title: "Settings…", action: #selector(showSettings(_:)), keyEquivalent: ",").targeted(self))
        appMenu.addItem(.separator())
        let quit = NSMenuItem(
            title: "Quit SwiftTerm Prototype",
            action: #selector(NSApplication.terminate(_:)),
            keyEquivalent: "q"
        )
        quit.target = NSApp
        appMenu.addItem(quit)
        appMenuItem.submenu = appMenu
        mainMenu.addItem(appMenuItem)

        let editMenuItem = NSMenuItem()
        let editMenu = NSMenu(title: "Edit")
        editMenu.addItem(NSMenuItem(title: "Copy", action: #selector(TerminalView.copy(_:)), keyEquivalent: "c").targeted(terminalView))
        editMenu.addItem(NSMenuItem(title: "Paste", action: #selector(TerminalView.paste(_:)), keyEquivalent: "v").targeted(terminalView))
        editMenu.addItem(.separator())
        editMenu.addItem(NSMenuItem(title: "Select All", action: #selector(TerminalView.selectAll(_:)), keyEquivalent: "a").targeted(terminalView))
        editMenuItem.submenu = editMenu
        mainMenu.addItem(editMenuItem)

        let viewMenuItem = NSMenuItem()
        let viewMenu = NSMenu(title: "View")
        let zoomIn = NSMenuItem(title: "Zoom In", action: #selector(zoomIn(_:)), keyEquivalent: "+")
        zoomIn.keyEquivalentModifierMask = [.command, .shift]
        zoomIn.target = self
        viewMenu.addItem(zoomIn)
        let zoomOut = NSMenuItem(title: "Zoom Out", action: #selector(zoomOut(_:)), keyEquivalent: "-")
        zoomOut.keyEquivalentModifierMask = [.command]
        zoomOut.target = self
        viewMenu.addItem(zoomOut)
        let zoomReset = NSMenuItem(title: "Actual Size", action: #selector(zoomReset(_:)), keyEquivalent: "0")
        zoomReset.keyEquivalentModifierMask = [.command]
        zoomReset.target = self
        viewMenu.addItem(zoomReset)
        viewMenuItem.submenu = viewMenu
        mainMenu.addItem(viewMenuItem)

        let windowMenuItem = NSMenuItem()
        let windowMenu = NSMenu(title: "Window")
        let close = NSMenuItem(title: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        close.target = window
        windowMenu.addItem(close)
        windowMenuItem.submenu = windowMenu
        mainMenu.addItem(windowMenuItem)
        NSApp.windowsMenu = windowMenu
        NSApp.mainMenu = mainMenu
    }

    private func showTerminalWindow() {
        guard let window, let terminalView else { return }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        window.makeFirstResponder(terminalView)
    }

    private func startDashboard() {
        guard !terminationRequested, !dashboardStartRequested,
              let configuration, let terminalView else { return }
        dashboardStartRequested = true
        terminalView.startProcess(
            executable: configuration.dashboardExecutable.path,
            environment: configuration.environment,
            execName: configuration.dashboardExecutable.lastPathComponent,
            currentDirectory: configuration.currentDirectory
        )
        dashboardStarted = true
    }

    private func setFontSize(_ size: Int) {
        let font = fontSettings.font(at: size)
        if terminalView?.font != font { terminalView?.font = font }
    }

    private func showLaunchError(_ error: Error) {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.alertStyle = .critical
        alert.messageText = "SwiftTerm Prototype refused to launch"
        alert.informativeText = error.localizedDescription
        alert.runModal()
        NSApp.terminate(nil)
    }

    private func presentPendingRuntimeAlert() {
        guard let pendingRuntimeAlert, let window, window.isVisible else { return }
        self.pendingRuntimeAlert = nil
        let alert = NSAlert()
        alert.alertStyle = .warning
        alert.messageText = pendingRuntimeAlert.title
        alert.informativeText = pendingRuntimeAlert.message
        alert.beginSheetModal(for: window)
    }
}

private extension NSMenuItem {
    func targeted(_ target: AnyObject) -> NSMenuItem {
        self.target = target
        return self
    }
}
