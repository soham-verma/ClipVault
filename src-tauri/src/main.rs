mod clipboard;
mod shortcuts;
mod windows;

use anyhow::{Context, Result};
use clipboard::{Capabilities, ClipboardBackend};
use clipvault_core::{
    model::*,
    store::Store,
    sync::{self, SyncService},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc},
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};
use tauri_plugin_autostart::ManagerExt;

struct AppState {
    store: Arc<Store>,
    sync: Arc<SyncService>,
    settings: Arc<Mutex<Settings>>,
    capabilities: Arc<Mutex<Capabilities>>,
    worker: mpsc::SyncSender<Work>,
}
enum Work {
    Copy(Clip, bool, mpsc::Sender<std::result::Result<(), String>>),
    Remote(Arc<clipvault_core::wire::Transfer>),
    Shortcut(String, mpsc::Sender<std::result::Result<(), String>>),
    Retry,
}
#[derive(Serialize)]
struct Snapshot {
    settings: Settings,
    capabilities: Capabilities,
    status: sync::Status,
    trusted: Vec<TrustedDevice>,
    nearby: Vec<sync::NearbyHost>,
    interfaces: Vec<(String, String)>,
}
type CommandResult<T> = std::result::Result<T, String>;
fn command_error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn snapshot(state: State<AppState>) -> CommandResult<Snapshot> {
    Ok(Snapshot {
        settings: state.settings.lock().clone(),
        capabilities: state.capabilities.lock().clone(),
        status: state.sync.status(),
        trusted: state.store.trusted().map_err(command_error)?,
        nearby: state.sync.nearby(),
        interfaces: SyncService::interfaces(),
    })
}
#[tauri::command]
fn history(
    state: State<AppState>,
    query: String,
    kind: Option<Kind>,
    offset: usize,
) -> CommandResult<Page> {
    state
        .store
        .page(&query, kind, offset, 60)
        .map_err(command_error)
}
#[tauri::command]
fn pin_item(
    app: tauri::AppHandle,
    state: State<AppState>,
    id: i64,
    pinned: bool,
) -> CommandResult<()> {
    state.store.pin(id, pinned).map_err(command_error)?;
    let _ = app.emit("history-changed", ());
    Ok(())
}
#[tauri::command]
fn delete_item(app: tauri::AppHandle, state: State<AppState>, id: i64) -> CommandResult<()> {
    state.store.delete(id).map_err(command_error)?;
    let _ = app.emit("history-changed", ());
    Ok(())
}
#[tauri::command]
fn clear_history(
    app: tauri::AppHandle,
    state: State<AppState>,
    keep_pinned: bool,
) -> CommandResult<()> {
    state.store.clear(keep_pinned).map_err(command_error)?;
    let _ = app.emit("history-changed", ());
    Ok(())
}
#[tauri::command]
fn image_path(state: State<AppState>, id: i64) -> CommandResult<String> {
    state
        .store
        .image_file(id)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(command_error)
}
#[tauri::command]
async fn copy_item(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    id: i64,
) -> CommandResult<()> {
    let clip = state.store.item_clip(id).map_err(command_error)?;
    let paste = state.settings.lock().paste_on_select && state.capabilities.lock().automatic_paste;
    if paste {
        window.hide().map_err(command_error)?;
    }
    let (send, receive) = mpsc::channel();
    state
        .worker
        .send(Work::Copy(clip, paste, send))
        .map_err(command_error)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        receive
            .recv_timeout(Duration::from_secs(10))
            .map_err(command_error)?
    })
    .await
    .map_err(command_error)?;
    if result.is_ok() && window.label() == "quick" {
        let _ = window.hide();
    } else if result.is_err() && paste {
        let _ = window.show();
        let _ = window.set_focus();
    }
    result
}
#[tauri::command]
async fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CommandResult<()> {
    settings.validate().map_err(command_error)?;
    let old = state.settings.lock().clone();
    if settings.launch_at_login != old.launch_at_login {
        let autostart = app.autolaunch();
        if settings.launch_at_login {
            autostart.enable()
        } else {
            autostart.disable()
        }
        .map_err(command_error)?;
    }
    if settings.shortcut != old.shortcut {
        let (send, receive) = mpsc::channel();
        state
            .worker
            .send(Work::Shortcut(settings.shortcut.clone(), send))
            .map_err(command_error)?;
        tauri::async_runtime::spawn_blocking(move || {
            receive
                .recv_timeout(Duration::from_secs(5))
                .map_err(command_error)?
        })
        .await
        .map_err(command_error)??;
        shortcuts::register(&app, &settings.shortcut)
            .await
            .map_err(command_error)?;
    }
    state
        .store
        .save_settings(&settings)
        .map_err(command_error)?;
    *state.settings.lock() = settings.clone();
    state
        .store
        .enforce_retention(&settings)
        .map_err(command_error)?;
    let _ = app.emit("history-changed", ());
    Ok(())
}
#[tauri::command]
async fn host_server(
    state: State<'_, AppState>,
    bind_address: Option<String>,
) -> CommandResult<()> {
    state
        .sync
        .start_host(bind_address, false)
        .await
        .map_err(command_error)
}
#[tauri::command]
fn join_server(state: State<AppState>, code: String, address: Option<String>) -> CommandResult<()> {
    state.sync.join(code, address).map_err(command_error)
}
#[tauri::command]
fn confirm_pairing(state: State<AppState>, id: String, accepted: bool) -> CommandResult<()> {
    state.sync.confirm(&id, accepted).map_err(command_error)
}
#[tauri::command]
fn pause_sharing(state: State<AppState>, paused: bool) -> CommandResult<()> {
    state.sync.pause(paused).map_err(command_error)
}
#[tauri::command]
fn disconnect(state: State<AppState>) -> CommandResult<()> {
    state.sync.stop(true).map_err(command_error)
}
#[tauri::command]
fn forget_device(state: State<AppState>, id: String) -> CommandResult<()> {
    state.sync.forget(&id).map_err(command_error)
}
#[tauri::command]
fn new_join_code(state: State<AppState>) -> CommandResult<()> {
    state.sync.rotate_code().map_err(command_error)
}
#[tauri::command]
async fn retry_clipboard(app: tauri::AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    state.worker.send(Work::Retry).map_err(command_error)?;
    let shortcut = state.settings.lock().shortcut.clone();
    shortcuts::register(&app, &shortcut)
        .await
        .map_err(command_error)
}
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn install_gnome_helper(app: tauri::AppHandle) -> CommandResult<String> {
    #[cfg(target_os = "linux")]
    {
        let source = app
            .path()
            .resource_dir()
            .map_err(command_error)?
            .join("gnome-extension");
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home().map_err(command_error)?.join(".local/share"));
        let target = base.join("gnome-shell/extensions/clipvault@com.soham.clipvault");
        std::fs::create_dir_all(target.join("schemas")).map_err(command_error)?;
        for file in [
            "metadata.json",
            "extension.js",
            "schemas/org.gnome.shell.extensions.clipvault.gschema.xml",
        ] {
            std::fs::copy(source.join(file), target.join(file)).map_err(command_error)?;
        }
        let compiled = std::process::Command::new("glib-compile-schemas")
            .arg(target.join("schemas"))
            .output()
            .map_err(command_error)?;
        if !compiled.status.success() {
            return Err("Could not compile GNOME helper settings. Install glib2 utilities.".into());
        }
        let enabled = std::process::Command::new("gnome-extensions")
            .args(["enable", "clipvault@com.soham.clipvault"])
            .output()
            .map_err(command_error)?;
        if enabled.status.success() {
            Ok("Helper enabled. Click Recheck clipboard access.".into())
        } else {
            Ok("Helper installed. Sign out of GNOME and back in, enable ClipVault Clipboard Bridge in Extensions, then reopen ClipVault.".into())
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Err("The helper is only required on GNOME Linux desktops".into())
    }
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("Home directory not found")
}
fn data_dir() -> Result<PathBuf> {
    // An explicit override keeps development/test sessions separate from a real history.
    if let Some(path) = std::env::var_os("CLIPVAULT_DATA_DIR") {
        return Ok(PathBuf::from(path));
    }
    #[cfg(target_os = "macos")]
    {
        Ok(home()?.join("Library/Application Support/ClipVault"))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".local/share"))
            .join("clipvault"))
    }
}
fn worker(
    app: tauri::AppHandle,
    store: Arc<Store>,
    sync: Arc<SyncService>,
    settings: Arc<Mutex<Settings>>,
    caps: Arc<Mutex<Capabilities>>,
    receiver: mpsc::Receiver<Work>,
    mut backend: Box<dyn ClipboardBackend>,
) {
    let mut baseline = false;
    let mut last_error = String::new();
    let mut retry_at = std::time::Instant::now();
    if let Err(error) = backend.configure_shortcut(&settings.lock().shortcut) {
        let _ = app.emit("app-error", error.to_string());
    }
    loop {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(Work::Copy(clip, paste, reply)) => {
                let result = (|| {
                    backend.write(&clip)?;
                    if let Some(payload) = clip.shared()? {
                        sync.publish(payload)?;
                    }
                    if paste {
                        std::thread::sleep(Duration::from_millis(100));
                        backend.paste()?;
                    }
                    Ok::<_, anyhow::Error>(())
                })();
                let _ = reply.send(result.map_err(command_error));
            }
            Ok(Work::Remote(transfer)) => {
                let clip = transfer.payload.to_clip(&transfer.envelope.origin.name);
                let result = (|| {
                    if !sync.should_record(&transfer) {
                        return Ok(());
                    }
                    store.insert(&clip, Some(&transfer.envelope.origin.id))?;
                    store.enforce_retention(&settings.lock())?;
                    let _ = app.emit("history-changed", ());
                    if sync.should_apply(&transfer) {
                        backend.write(&clip)?;
                    }
                    Ok::<_, anyhow::Error>(())
                })();
                if let Err(error) = result {
                    let _ = app.emit("app-error", error.to_string());
                }
            }
            Ok(Work::Shortcut(shortcut, reply)) => {
                let _ = reply.send(backend.configure_shortcut(&shortcut).map_err(command_error));
            }
            Ok(Work::Retry) => {
                if let Ok(next) = clipboard::create() {
                    backend = next;
                    baseline = false;
                    let _ = backend.configure_shortcut(&settings.lock().shortcut);
                    *caps.lock() = backend.capabilities();
                    let _ = app.emit("capabilities-changed", ());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if !backend.capabilities().monitoring && retry_at.elapsed() > Duration::from_secs(5) {
            retry_at = std::time::Instant::now();
            if let Ok(next) = clipboard::create() {
                backend = next;
                *caps.lock() = backend.capabilities();
                let _ = app.emit("capabilities-changed", ());
            }
        }
        let result = (|| {
            let Some(snapshot) = backend.snapshot()? else {
                return Ok(());
            };
            if !baseline {
                baseline = true;
                return Ok(());
            } // Never publish a pre-existing clipboard on launch/reconnect.
            if let Some(error) = snapshot.error {
                anyhow::bail!(error);
            }
            if snapshot.owned {
                return Ok(());
            }
            if let Some(clip) = snapshot.clip {
                let prefs = settings.lock().clone();
                if !prefs.permits(&clip) || clip.sensitive {
                    return Ok(());
                }
                if clip.kind != Kind::Image && clip.text.as_ref().is_none_or(|s| s.is_empty()) {
                    return Ok(());
                }
                store.insert(&clip, None)?;
                store.enforce_retention(&prefs)?;
                if let Some(payload) = clip.shared()? {
                    sync.publish(payload)?;
                }
                let _ = app.emit("history-changed", ());
            }
            Ok::<(), anyhow::Error>(())
        })();
        if let Err(error) = result {
            let message = error.to_string();
            if last_error != message {
                let _ = app.emit("app-error", &message);
                last_error = message;
            }
        }
    }
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let root = data_dir()?;
    let store = Arc::new(Store::open(&root)?);
    if store.get::<Settings>("settings")?.is_none() {
        store.save_settings(&clipboard::legacy_preferences().unwrap_or_default())?;
    }
    let prefs = store.settings()?;
    let settings = Arc::new(Mutex::new(prefs.clone()));
    if prefs.launch_at_login && std::env::var_os("CLIPVAULT_DATA_DIR").is_none() {
        app.autolaunch().enable()?;
    }
    let key = clipboard::identity(&root)?;
    let sync =
        tauri::async_runtime::block_on(async { SyncService::new(store.clone(), key, true) })?;
    let backend = clipboard::create()?;
    let capabilities = Arc::new(Mutex::new(backend.capabilities()));
    let (sender, receiver) = mpsc::sync_channel(8);
    let worker_app = app.handle().clone();
    let worker_store = store.clone();
    let worker_sync = sync.clone();
    let worker_settings = settings.clone();
    let worker_caps = capabilities.clone();
    std::thread::Builder::new()
        .name("clipvault-clipboard".into())
        .spawn(move || {
            worker(
                worker_app,
                worker_store,
                worker_sync,
                worker_settings,
                worker_caps,
                receiver,
                backend,
            )
        })?;
    let event_app = app.handle().clone();
    let event_sender = sender.clone();
    let mut events = sync.subscribe();
    let event_sync = sync.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(sync::Event::Remote(transfer)) => {
                    if event_sender.try_send(Work::Remote(transfer)).is_err() {
                        let _ = event_sync.pause(true);
                        let _ = event_app.emit("app-error", "Sharing paused because clipboard updates arrived faster than this device could apply them. Resume sharing and copy again.");
                    }
                }
                Ok(sync::Event::Status(status)) => {
                    let _ = event_app.emit("connection-changed", status);
                }
                Ok(sync::Event::Discovery) => {
                    let _ = event_app.emit("discovery-changed", ());
                }
                Ok(sync::Event::Error(error)) => {
                    let _ = event_app.emit("app-error", error);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _=event_app.emit("app-error","Some updates were skipped because the device was busy. Copy the item again.");
                }
                Err(_) => break,
            }
        }
    });
    app.asset_protocol_scope()
        .allow_directory(root.join("Blobs"), true)?;
    app.manage(AppState {
        store,
        sync: sync.clone(),
        settings,
        capabilities,
        worker: sender,
    });
    let quick = MenuItem::with_id(app, "quick", "Quick clipboard", true, None::<&str>)?;
    let open = MenuItem::with_id(
        app,
        "open",
        "History, sharing & preferences…",
        true,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(app, "pause", "Pause / resume sharing", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quick, &open, &pause, &quit])?;
    let tray = TrayIconBuilder::new()
        .icon(
            app.default_window_icon()
                .context("Missing app icon")?
                .clone(),
        )
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("ClipVault — Quick clipboard")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                windows::quick(tray.app_handle(), Some(rect), true);
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "quick" => windows::quick(app, None, false),
            "open" => windows::open_main(app, "history"),
            "pause" => {
                let state = app.state::<AppState>();
                if let Err(e) = state.sync.pause(!state.sync.status().paused) {
                    let _ = app.emit("app-error", e.to_string());
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app);
    if let Err(error) = tray {
        let _ = app.emit(
            "app-error",
            format!("Tray unavailable; use your shortcut or launcher: {error}"),
        );
    }
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = shortcuts::register(&handle, &prefs.shortcut).await {
            let _ = handle.emit("app-error", format!("Shortcut setup: {error}"));
        }
        if let Err(error) = sync.resume().await {
            let _ = handle.emit("app-error", error.to_string());
        }
    });
    if std::env::args().any(|a| a == "--settings") {
        windows::open_main(app.handle(), "settings");
    } else if !std::env::args().any(|a| a == "--background") {
        windows::quick(app.handle(), None, false);
    }
    Ok(())
}

fn run() -> Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if args.iter().any(|a| a == "--settings") {
                windows::open_main(app, "settings");
            } else {
                windows::quick(app, None, true);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        windows::quick(app, None, true)
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            snapshot,
            history,
            pin_item,
            delete_item,
            clear_history,
            image_path,
            copy_item,
            save_settings,
            host_server,
            join_server,
            confirm_pairing,
            pause_sharing,
            disconnect,
            forget_device,
            new_join_code,
            retry_clipboard,
            install_gnome_helper,
            quit,
            windows::open_workspace,
            windows::workspace_view
        ])
        .setup(setup)
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Focused(false) if window.label() == "quick" => {
                windows::dismiss(window.app_handle())
            }
            _ => {}
        })
        .build(tauri::generate_context!())?
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                windows::quick(app, None, false);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("ClipVault could not start: {error:#}");
        std::process::exit(1);
    }
}
