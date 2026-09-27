use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, PhysicalPosition, Rect};

static LAST_DISMISS: Mutex<Option<Instant>> = Mutex::new(None);
static WORKSPACE_VIEW: Mutex<Option<String>> = Mutex::new(None);

pub fn dismiss(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("quick") {
        if window.is_visible().unwrap_or(false) {
            *LAST_DISMISS.lock().unwrap() = Some(Instant::now());
            let _ = window.hide();
        }
    }
}

pub fn open_main(app: &tauri::AppHandle, view: &str) {
    *WORKSPACE_VIEW.lock().unwrap() = Some(view.into());
    dismiss(app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("navigate", view);
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
pub fn workspace_view() -> String {
    WORKSPACE_VIEW
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "history".into())
}

pub fn quick(app: &tauri::AppHandle, anchor: Option<Rect>, toggle: bool) {
    let Some(window) = app.get_webview_window("quick") else {
        return;
    };
    let recently_dismissed = LAST_DISMISS
        .lock()
        .unwrap()
        .is_some_and(|time| time.elapsed() < Duration::from_millis(250));
    if toggle && (window.is_visible().unwrap_or(false) || recently_dismissed) {
        dismiss(app);
        return;
    }
    // Wayland chooses window placement itself. On Mac/X11 keep the picker on the
    // pointer's display, anchored below/above the tray when its geometry is available.
    let cursor = window.cursor_position().ok();
    let monitor = cursor
        .and_then(|p| window.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        let work = monitor.work_area();
        let size = window.outer_size().unwrap_or(tauri::PhysicalSize::new(
            (380.0 * scale) as u32,
            (480.0 * scale) as u32,
        ));
        let left = work.position.x as f64 + 8.0 * scale;
        let top = work.position.y as f64 + 8.0 * scale;
        let right =
            (work.position.x as f64 + work.size.width as f64 - size.width as f64 - 8.0 * scale)
                .max(left);
        let bottom =
            (work.position.y as f64 + work.size.height as f64 - size.height as f64 - 8.0 * scale)
                .max(top);
        let (x, y) = if let Some(anchor) = anchor {
            let position = anchor.position.to_physical::<f64>(scale);
            let tray_size = anchor.size.to_physical::<f64>(scale);
            let below = position.y + tray_size.height + 6.0 * scale;
            (
                position.x + tray_size.width / 2.0 - size.width as f64 / 2.0,
                if below <= bottom {
                    below
                } else {
                    position.y - size.height as f64 - 6.0 * scale
                },
            )
        } else {
            ((left + right) / 2.0, (top + bottom) / 2.0)
        };
        let _ = window.set_position(PhysicalPosition::new(
            x.clamp(left, right),
            y.clamp(top, bottom),
        ));
    } else {
        let _ = window.center();
    }
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    let _ = window.emit("quick-opened", ());
}

#[tauri::command]
pub fn open_workspace(app: tauri::AppHandle, view: String) -> Result<(), String> {
    if !["history", "sharing", "settings"].contains(&view.as_str()) {
        return Err("Unknown ClipVault view".into());
    }
    open_main(&app, &view);
    Ok(())
}
