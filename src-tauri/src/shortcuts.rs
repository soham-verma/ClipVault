use anyhow::Result;

pub async fn register(app: &tauri::AppHandle, shortcut: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return wayland(app, shortcut).await;
    }
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};
    static BINDING: std::sync::Mutex<Option<Shortcut>> = std::sync::Mutex::new(None);
    let parsed: Shortcut = shortcut.parse()?;
    let mut binding = BINDING.lock().unwrap();
    if *binding == Some(parsed) {
        return Ok(());
    }
    app.global_shortcut().register(parsed)?;
    if let Some(old) = *binding {
        if let Err(error) = app.global_shortcut().unregister(old) {
            let _ = app.global_shortcut().unregister(parsed);
            return Err(error.into());
        }
    }
    *binding = Some(parsed);
    Ok(())
}

#[cfg(any(target_os = "linux", feature = "linux-backend-check"))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
async fn wayland(app: &tauri::AppHandle, shortcut: &str) -> Result<()> {
    use futures_util::StreamExt;
    static LISTENER: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>> =
        std::sync::Mutex::new(None);
    let handle = app.clone();
    if std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("gnome")
    {
        let connection = zbus::Connection::session().await?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.gnome.Shell.Extensions.ClipVault",
            "/org/gnome/Shell/Extensions/ClipVault",
            "org.gnome.Shell.Extensions.ClipVault1",
        )
        .await?;
        // The clipboard bridge owns the authorized bus name. SetShortcut uses that connection;
        // users can also edit the binding in GNOME's extension settings with gsettings.
        let _ = shortcut;
        let mut stream = proxy.receive_signal("ToggleRequested").await?;
        let task = tauri::async_runtime::spawn(async move {
            let _connection = connection;
            while stream.next().await.is_some() {
                crate::windows::quick(&handle, None, true);
            }
        });
        if let Some(old) = LISTENER.lock().unwrap().replace(task) {
            old.abort();
        }
        return Ok(());
    }
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session().await?;
    let trigger = linux_trigger(shortcut, false)?;
    let shortcuts = [
        NewShortcut::new("show-clipvault", "Open ClipVault clipboard history")
            .preferred_trigger(Some(trigger.as_str())),
    ];
    portal
        .bind_shortcuts(&session, &shortcuts, None)
        .await?
        .response()?;
    let mut signals = portal.receive_activated().await?;
    let task = tauri::async_runtime::spawn(async move {
        let _keep_session = session;
        let _keep_portal = portal;
        while signals.next().await.is_some() {
            crate::windows::quick(&handle, None, true);
        }
    });
    if let Some(old) = LISTENER.lock().unwrap().replace(task) {
        old.abort();
    }
    Ok(())
}

/// Convert the app's key-code syntax to XKB names shared by GTK and the portal.
#[cfg(any(target_os = "linux", feature = "linux-backend-check"))]
pub fn linux_trigger(shortcut: &str, gtk: bool) -> Result<String> {
    let parts: Vec<_> = shortcut.split('+').collect();
    let (key, modifiers) = parts
        .split_last()
        .ok_or_else(|| anyhow::anyhow!("Invalid shortcut"))?;
    let mut result = String::new();
    for modifier in modifiers {
        result.push_str(match (*modifier, gtk) {
            ("Super" | "Meta" | "Command", true) => "<Super>",
            ("Control" | "Ctrl", true) => "<Control>",
            ("Alt", true) => "<Alt>",
            ("Shift", true) => "<Shift>",
            ("Super" | "Meta" | "Command", false) => "LOGO+",
            ("Control" | "Ctrl", false) => "CTRL+",
            ("Alt", false) => "ALT+",
            ("Shift", false) => "SHIFT+",
            _ => anyhow::bail!("Unsupported shortcut modifier"),
        });
    }
    let mapped = match *key {
        "Enter" => "Return",
        "Space" => "space",
        "Backspace" => "BackSpace",
        "ArrowDown" => "Down",
        "ArrowUp" => "Up",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "PageUp" => "Page_Up",
        "PageDown" => "Page_Down",
        "Comma" => "comma",
        "Period" => "period",
        "Slash" => "slash",
        "Backslash" => "backslash",
        "Semicolon" => "semicolon",
        "Quote" => "apostrophe",
        "Backquote" => "grave",
        "BracketLeft" => "bracketleft",
        "BracketRight" => "bracketright",
        "Minus" => "minus",
        "Equal" => "equal",
        other => other,
    };
    if mapped.len() == 1 {
        result.push_str(&mapped.to_lowercase());
    } else {
        result.push_str(mapped);
    }
    Ok(result)
}

#[cfg(all(test, any(target_os = "linux", feature = "linux-backend-check")))]
mod tests {
    #[test]
    fn shortcuts_use_desktop_specific_syntax() {
        assert_eq!(
            super::linux_trigger("Super+Shift+V", false).unwrap(),
            "LOGO+SHIFT+v"
        );
        assert_eq!(
            super::linux_trigger("Control+F8", true).unwrap(),
            "<Control>F8"
        );
        assert_eq!(
            super::linux_trigger("Alt+ArrowDown", true).unwrap(),
            "<Alt>Down"
        );
    }
}
