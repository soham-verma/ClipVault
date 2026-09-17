import AppKit
import Carbon.HIToolbox

@MainActor
enum ClipboardWriter {
    static func copy(_ item: ClipItem, blobStore: BlobStore, monitor: ClipboardMonitor) {
        let pb = NSPasteboard.general
        monitor.suppressNextChange()
        pb.clearContents()

        switch item.kind {
        case .text, .url:
            pb.setString(item.fullText ?? item.previewText, forType: .string)
        case .rtf:
            if let path = item.blobPath, let data = try? Data(contentsOf: blobStore.url(for: path)) {
                pb.setData(data, forType: .rtf)
            }
            if let plain = item.fullText { pb.setString(plain, forType: .string) }
        case .image:
            if let path = item.blobPath, let data = try? Data(contentsOf: blobStore.url(for: path)) {
                pb.setData(data, forType: .png)
            }
        case .fileList:
            let paths = (item.fullText ?? "").split(separator: "\n").map(String.init)
            let urls = paths.map { URL(fileURLWithPath: $0) as NSURL }
            pb.writeObjects(urls)
        }
    }

    static func copyAndPaste(_ item: ClipItem, blobStore: BlobStore, monitor: ClipboardMonitor) {
        copy(item, blobStore: blobStore, monitor: monitor)
        // Give the pasteboard a beat to settle, then synthesize Cmd+V into the frontmost app.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
            simulatePaste()
        }
    }

    private static func simulatePaste() {
        let source = CGEventSource(stateID: .combinedSessionState)
        let vKeyCode = CGKeyCode(kVK_ANSI_V)
        guard let keyDown = CGEvent(keyboardEventSource: source, virtualKey: vKeyCode, keyDown: true),
              let keyUp = CGEvent(keyboardEventSource: source, virtualKey: vKeyCode, keyDown: false) else { return }
        keyDown.flags = .maskCommand
        keyUp.flags = .maskCommand
        keyDown.post(tap: .cghidEventTap)
        keyUp.post(tap: .cghidEventTap)
    }
}
