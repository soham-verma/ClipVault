# Verification

## Local results — 26 September 2026

- macOS ARM64: production `.app` and `.dmg` built successfully.
- 12 Rust core tests and 9 frontend/helper tests passed; the desktop shortcut
  conversion test also passed. Rust Clippy, formatting, and TypeScript passed.
- Native Mac smoke test with isolated history: the global shortcut opens the
  picker; Enter copies and dismisses it; ordinary paste returns the chosen text;
  a new normal copy is captured. Sharing/preferences navigation and dark-mode
  picker appearance were checked. Automatic paste requires Accessibility access.
- Linux adapters were type-checked from the Mac host. Real Linux runtime checks
  and the other architecture builds remain to be run by CI/on target machines.

## Automated checks

Run `npm ci`, `npm run build`, `npm test`, `cargo test -p clipvault-core`, and
`cargo clippy --workspace --all-targets -- -D warnings`.

The core suite creates isolated data directories and real loopback connections.
It never reads or writes your desktop clipboard. It exercises legacy SQLite
migration, pinned retention, shared blobs, Noise verification, rejected pairing,
three-device text and image sharing, simultaneous events, room/participant
pause, reconnection, and revocation. Allow localhost socket access if a development
sandbox blocks the network tests.

On macOS, `cargo check -p clipvault --features linux-backend-check` also compiles
the pure-Rust Linux clipboard and portal adapters. This catches API/type errors;
it does **not** certify behavior under a real Linux compositor.

Linux CI runs the X11 adapter test under its own Xvfb display. Never run the
ignored clipboard integration test against a desktop clipboard containing work:
it deliberately replaces clipboard contents with test fixtures.

## Desktop acceptance matrix

Before publishing a release, run these checks on macOS 13+, X11, KDE Plasma
Wayland, Sway/Hyprland, and GNOME 46–51 with the helper, on both architectures.
The repository workflow produces installers; it does not establish a passed
GNOME/KDE/Hyprland runtime matrix by compiling alone.

- [ ] Copy Unicode/multiline text, URLs, and screenshots between every Mac/Linux
  pairing; paste into a browser, editor, image-capable app, and terminal.
- [ ] On Linux test native Wayland sources and destinations, not only XWayland.
- [ ] Receive a large PNG spanning several frames; verify pixels and dimensions.
- [ ] Approve matching codes on both devices; reject mismatched codes; ensure
  no contents arrive before both approvals.
- [ ] Sleep/wake, disconnect Wi-Fi, restart host and participants; ensure no
  offline or pre-existing clipboard is replayed.
- [ ] Pause each role and forget a device while connected and while offline.
- [ ] Verify sensitive-marker filtering and platform-supported app exclusions.
- [ ] Test global shortcut, tray/launcher access, hide/restore, and launch at login.
- [ ] Open the compact picker from the Mac tray, Linux tray menu, GNOME panel,
  and launcher. Verify single-click selection, arrows/Enter, Cmd/Ctrl+1–9, Escape,
  outside-click dismissal, and the full-library/sharing/settings buttons.
- [ ] Record a custom shortcut, save, restart, and verify the new binding.
  Test a reserved/conflicting binding and ensure the previous shortcut still works.
- [ ] GNOME: install/enable helper, lock/unlock, disable/re-enable extension,
  and change the shortcut. Confirm capability status recovers after Recheck.
- [ ] Upgrade a copy of a legacy Mac data directory and check every clipboard kind.

Use `CLIPVAULT_DATA_DIR=/absolute/test/directory npm run tauri -- dev` for an
isolated history during manual app tests. It still uses the actual system
clipboard. The explicit override also isolates the identity key. Do not run the legacy and new apps together.
