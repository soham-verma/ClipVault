import AppKit
import SwiftUI

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var statusItem: NSStatusItem!
    private var popover: NSPopover!
    private var preferencesWindow: NSWindow?
    private var localKeyMonitor: Any?

    private let db = Database(path: AppDelegate.databasePath())
    private lazy var blobStore = BlobStore()
    private lazy var monitor = ClipboardMonitor(db: db, blobStore: blobStore)
    private lazy var store = HistoryStore(db: db, blobStore: blobStore)
    private let hotKeyManager = HotKeyManager()

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            button.image = NSImage(systemSymbolName: "clipboard", accessibilityDescription: "ClipVault")
            button.action = #selector(togglePopover)
            button.target = self
        }

        popover = NSPopover()
        popover.behavior = .transient
        popover.contentSize = NSSize(width: 380, height: 480)
        popover.contentViewController = NSHostingController(rootView: makePopoverView())

        monitor.onNewItem = { [weak self] in
            self?.store.reload()
        }
        monitor.start()

        hotKeyManager.onHotKey = { [weak self] in
            self?.showPopover()
        }
        hotKeyManager.register()

        if Settings.shared.launchAtLogin {
            LoginItem.setEnabled(true)
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        monitor.stop()
        hotKeyManager.unregister()
    }

    private func makePopoverView() -> some View {
        PopoverView(
            store: store,
            blobStore: blobStore,
            monitor: monitor,
            onCopyItem: { [weak self] item, closePopover in
                self?.paste(item, closePopover: closePopover)
            },
            onOpenPreferences: { [weak self] in
                self?.openPreferences()
            },
            onQuit: {
                NSApp.terminate(nil)
            }
        )
    }

    private func paste(_ item: ClipItem, closePopover: Bool) {
        if Settings.shared.pasteOnSelect {
            popover.performClose(nil)
            ClipboardWriter.copyAndPaste(item, blobStore: blobStore, monitor: monitor)
        } else {
            ClipboardWriter.copy(item, blobStore: blobStore, monitor: monitor)
            if closePopover { popover.performClose(nil) }
        }
    }

    @objc private func togglePopover() {
        if popover.isShown {
            popover.performClose(nil)
        } else {
            showPopover()
        }
    }

    private func showPopover() {
        guard let button = statusItem.button else { return }
        store.searchText = ""
        store.reload()
        popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        popover.contentViewController?.view.window?.makeKey()
        installLocalKeyMonitor()
    }

    /// Lets you press 1-9 while the popover is open to instantly paste that row, and Escape to dismiss.
    private func installLocalKeyMonitor() {
        removeLocalKeyMonitor()
        localKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self else { return event }
            if event.keyCode == 53 { // Escape
                self.popover.performClose(nil)
                return nil
            }
            if let chars = event.charactersIgnoringModifiers, let digit = Int(chars), digit >= 1, digit <= 9 {
                let index = digit - 1
                if index < self.store.items.count {
                    self.paste(self.store.items[index], closePopover: true)
                    return nil
                }
            }
            return event
        }
    }

    private func removeLocalKeyMonitor() {
        if let localKeyMonitor {
            NSEvent.removeMonitor(localKeyMonitor)
        }
        localKeyMonitor = nil
    }

    private func openPreferences() {
        popover.performClose(nil)
        if preferencesWindow == nil {
            let window = NSWindow(contentViewController: NSHostingController(rootView: PreferencesView(settings: Settings.shared)))
            window.title = "ClipVault Preferences"
            window.styleMask = [.titled, .closable]
            window.isReleasedWhenClosed = false
            preferencesWindow = window
        }
        NSApp.activate(ignoringOtherApps: true)
        preferencesWindow?.makeKeyAndOrderFront(nil)
    }

    private static func databasePath() -> String {
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let dir = appSupport.appendingPathComponent("ClipVault", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("clipvault.sqlite3").path
    }
}
