# ClipVault

Clipboard history and encrypted local-network sharing for **macOS and Linux**,
on **ARM64 and AMD64/x86_64**. Built with Rust, Tauri 2, React, and TypeScript.

Search, pin, and reuse text, links, screenshots, and local file references. Host
a server on one computer, join from another, and new copies become available
to paste on every connected device. There are no accounts or cloud relays.

## Quick clipboard picker

ClipVault opens a compact **380 × 480** clipboard picker for everyday use.
Click the Mac menu-bar icon, the GNOME helper's top-bar button, or choose
**Quick clipboard** from a Linux tray menu. Press **Cmd+Shift+V** on Mac or
**Super+Shift+V** on Linux to toggle the same picker. On desktops without a tray,
the application launcher and `clipvault --show` also open it.

Search immediately, use ↑/↓ and Enter, or double-click an item to copy it and
close the picker. The number keys 1–9 copy the matching item while focus is
outside the search field; Escape or clicking outside closes the picker. Optional automatic paste works on Mac/X11 where permitted.
On Wayland, paste normally after selecting an item.

Change the binding in **Preferences → Quick clipboard shortcut**: record your
own keys or type a shortcut, then save. Reserved system combinations may be
unavailable, and the Wayland portal can ask you to confirm or choose a binding.
The picker has footer buttons for sharing and preferences; the larger window
remains available for those tasks. Wayland compositors control
popup placement, so the picker may be centered rather than anchored to the tray.

## Build and run

Install current stable Rust (minimum 1.88), Node.js 24, and the
[Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/).
macOS also requires Xcode Command Line Tools. On Arch Linux:

```sh
sudo pacman -S --needed base-devel rust nodejs npm webkit2gtk-4.1 \
  libappindicator-gtk3 openssl librsvg xdotool
```

On Ubuntu 24.04 / Debian with WebKitGTK 4.1:

```sh
sudo apt-get install build-essential libwebkit2gtk-4.1-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf
```

```sh
npm ci
npm run tauri -- dev
```

Create installers for the current platform and architecture:

```sh
./build.sh
```

Outputs are under `target/release/bundle/`: `.app` and `.dmg` on Mac; AppImage,
`.deb`, and `.rpm` on Linux. AppImages may need FUSE, or can be started with
`APPIMAGE_EXTRACT_AND_RUN=1`. A `v*` Git tag builds both architectures on both
platforms and publishes a GitHub release containing DMG, macOS app archive,
AppImage, Debian, RPM, Arch recipe, GNOME helper, and SHA-256 checksum downloads.
Linux binaries are built on Ubuntu 24.04 (glibc 2.39+). Build from source for an
older compatible distribution. Arch and Arch Linux ARM can use the bundled
[packaging recipe](packaging/arch/PKGBUILD).

Mac bundles can be signed/notarized using the optional `APPLE_*` repository
secrets listed in [the build workflow](.github/workflows/build.yml). Without
Developer ID signing they are local development builds; macOS may require
explicit approval before opening a downloaded copy.

## Share a clipboard

1. Open **Device sharing → Host Server** on a Mac or Linux device.
2. Open ClipVault on another device on the same network. Enter the six-digit
   server code. If discovery is blocked, enter the displayed host IP and port.
3. Compare the complete verification code shown on **both** devices, then choose
   **Codes match** on each. No clipboard contents are exchanged before this.
4. Copy text, links, or images normally, then paste normally on any participant.

The short server code expires in five minutes and is only a discovery locator.
The separate verification authenticates the connection. Verified devices are
remembered and reconnect after brief outages or app restarts. **Disconnect** or
**Stop server** disables automatic reconnection until you explicitly join/host
again. **Forget** removes a device's authorization.

The host must remain running. Its pause button pauses the whole room; a guest's
pause button pauses that device. Only new copies made while connected are
shared: no old history, offline queue, or pre-existing clipboard is uploaded.
The host orders simultaneous copies, and the latest accepted copy wins.

Text is limited to 1 MiB; PNG images to 20 MiB and 32 megapixels. Rich text shares
its plain text; copied files and their paths remain local. Received copies are
saved in local history and follow the same retention rules.

## Linux desktop integration

| Desktop                                                     | Background clipboard                                          | Shortcut / paste behavior                                      |
| ----------------------------------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------- |
| macOS 13+                                                   | Native pasteboard                                             | Cmd+Shift+V; automatic paste requires Accessibility permission |
| Linux X11                                                   | Native XFixes/selection adapter, including incremental images | Super+Shift+V; optional automatic paste                        |
| KDE / Sway / Hyprland / other supported Wayland compositors | `ext-data-control` or `wlr-data-control`                      | Global Shortcuts portal where available; normal manual paste   |
| GNOME Wayland                                               | Bundled GNOME Shell helper                                    | Helper shortcut; normal manual paste                           |

On GNOME, use **Preferences → Install GNOME helper**. You may need to sign out
and back in, enable **ClipVault Clipboard Bridge** in GNOME Extensions, then
click **Recheck clipboard access**. The helper targets GNOME 46–51. It only
provides clipboard/shortcut access over a versioned session D-Bus interface;
networking and history remain in the Rust app. Its tests mock the shell APIs;
real desktop acceptance is tracked in [TESTING.md](docs/TESTING.md).

If a Wayland compositor has no Global Shortcuts portal, bind `clipvault --show`
in its settings. For example, in Sway use
`bindsym Mod4+Shift+v exec clipvault --show`; in Hyprland use
`bind = SUPER SHIFT, V, exec, clipvault --show`.
The regular application launcher always opens the picker, including desktops
without a system tray. Use **Open library** for the full window, or `clipvault --settings`.
The app reports missing clipboard capabilities instead
of falling back silently to an incomplete XWayland clipboard.

Password-manager sensitive markers are always excluded. Per-app exclusions use
Mac bundle IDs, X11 WM_CLASS, or GNOME desktop IDs. Generic Wayland does not
provide source-app identity, so its exclusion control is disabled. Source
attribution describes the active application and is best-effort, not a security
boundary for other applications copying in the background.

## Existing Mac history

The app keeps `com.soham.clipvault` and
`~/Library/Application Support/ClipVault/`. It backs up the old SQLite database
before migrating, retains blobs and pinned items, and imports legacy
preferences. Close the old Swift app before starting this version. The Swift
implementation remains available in Git history.

Linux data lives in `$XDG_DATA_HOME/clipvault` (normally
`~/.local/share/clipvault`). Large payloads are content-addressed files in
`Blobs/`; the database indexes text using SQLite FTS5. Only unpinned items are
pruned. A blob is deleted only after its last history reference is gone.

For isolated development history and identity, set
`CLIPVAULT_DATA_DIR=/absolute/test/directory`. The app still uses the system
clipboard. Never put real clipboard data in the repository.

## Development and verification

```sh
npm run build
npm test
cargo test -p clipvault-core
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
# Also type-check the pure-Rust Linux integrations from a Mac:
cargo check -p clipvault --features linux-backend-check
```

See [the acceptance checklist](docs/TESTING.md) and
[the protocol description](docs/PROTOCOL.md). The cross-platform build matrix
and Linux runtime checks run in CI; a successful Mac build alone does not
establish compatibility with every Linux desktop/version.

MIT — see [LICENSE](LICENSE).
