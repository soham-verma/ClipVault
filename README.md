# ClipVault

A lightweight, native macOS clipboard manager. Menu-bar only, built with
Swift/AppKit/SwiftUI — no Electron, minimal memory footprint, and designed to
hold an effectively unlimited clipboard history without the wheels coming off.

## Features

- **Menu-bar app** — no dock icon, lives quietly in the status bar
- **Global hotkey** — `⌘⇧V` from anywhere, no Accessibility permission required
- **Massive history, small footprint** — clipboard text is indexed in SQLite
  with full-text search (FTS5); images and files are stored as content-hashed
  blobs on disk, not in memory or the database, so history size doesn't
  degrade UI performance
- **Instant search** across your entire history, with type filters (Text /
  Images / Links / Files)
- **Quick paste** — press `1`–`9` while the popover is open to paste that row
  instantly
- **Pin** items to keep them forever, exempt from any cleanup
- **Configurable retention** — unlimited by item count by default, capped only
  by total disk usage (default 2 GB, adjustable); pinned items are never
  auto-removed
- **Privacy-aware** — automatically ignores copies from password managers
  (1Password, Bitwarden, etc.) via the `org.nspasteboard.ConcealedType`
  convention; per-app exclusion list in Preferences
- **Dedupes** consecutive identical copies
- **Launch at login** (optional, via `SMAppService`)

## Requirements

- macOS 13 (Ventura) or later
- Xcode Command Line Tools (for `swift`/`swiftc`)

## Building

Preferred:

```sh
swift build -c release
```

If your local toolchain's SwiftPM is broken (mismatched Command Line Tools
install — a system issue unrelated to this code, shows up as a `dyld: Symbol
not found` error from `swift-package`), use the bundled fallback build script,
which invokes `swiftc` directly and assembles the `.app` bundle by hand:

```sh
./build.sh
```

Either way, the resulting app can be run directly or copied into
`/Applications`:

```sh
open build/ClipVault.app
cp -R build/ClipVault.app /Applications/
```

The app is ad-hoc code-signed (`codesign --sign -`) for local use. It is not
notarized, so if you move it to another Mac, Gatekeeper will block it on first
launch — right-click → Open to bypass.

## Usage

- Click the clipboard icon in the menu bar, or press `⌘⇧V`, to open the
  history popover
- Type to search; use the filter chips to narrow by content type
- Double-click a row, or press its number key (`1`–`9`), to paste it
- Right-click a row for Copy / Pin / Delete
- `Esc` closes the popover
- Gear icon opens Preferences (launch at login, paste behavior, disk cap,
  excluded apps)

## Architecture

- `Core/ClipboardMonitor.swift` — polls `NSPasteboard.general.changeCount`
  (macOS has no push notification for pasteboard changes); this is the
  standard, App-Store-safe technique every clipboard manager uses
- `Core/Database.swift` — thin wrapper over the system `sqlite3` C library;
  schema includes an FTS5 virtual table kept in sync via triggers
- `Core/BlobStore.swift` — content-addressed storage for images/files under
  `~/Library/Application Support/ClipVault/Blobs`, sharded by hash prefix
- `Core/HistoryStore.swift` — paginated view model backing the SwiftUI list,
  so the UI never has to hold the full history in memory
- `Core/HotKeyManager.swift` — Carbon `RegisterEventHotKey`-based global
  hotkey (no Accessibility permission needed)
- `UI/` — SwiftUI views for the popover and preferences window

Data lives entirely locally in `~/Library/Application Support/ClipVault/` —
nothing leaves your machine.

## License

MIT — see [LICENSE](LICENSE).
