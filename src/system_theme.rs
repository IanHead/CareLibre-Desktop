//! Detects the desktop's light/dark preference.
//!
//! winit exposes the system theme on Windows and macOS, but not on Linux. The
//! freedesktop settings portal is the canonical source there; `GTK_THEME` and
//! `gsettings` provide fallbacks for desktops without a Settings portal. A
//! small background poll keeps **Follow System** in step when the desktop
//! switches appearance.

use std::sync::atomic::{AtomicU8, Ordering};

const UNKNOWN: u8 = 0;
const LIGHT: u8 = 1;
const DARK: u8 = 2;

static DETECTED: AtomicU8 = AtomicU8::new(UNKNOWN);

/// The most recently detected system appearance, if one was found.
pub fn detected() -> Option<eframe::egui::Theme> {
    match DETECTED.load(Ordering::Relaxed) {
        LIGHT => Some(eframe::egui::Theme::Light),
        DARK => Some(eframe::egui::Theme::Dark),
        _ => None,
    }
}

/// Start polling the desktop appearance in the background.
#[cfg(target_os = "linux")]
pub fn start(ctx: eframe::egui::Context) {
    use std::{sync::Once, thread, time::Duration};

    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        thread::Builder::new()
            .name("carelibre-system-theme".to_owned())
            .spawn(move || {
                let mut last = UNKNOWN;
                loop {
                    if let Some(dark) = detect() {
                        let next = if dark { DARK } else { LIGHT };
                        if next != last {
                            last = next;
                            DETECTED.store(next, Ordering::Relaxed);
                            ctx.request_repaint();
                        }
                    }
                    thread::sleep(Duration::from_secs(5));
                }
            })
            .ok();
    });
}

#[cfg(not(target_os = "linux"))]
pub fn start(_ctx: eframe::egui::Context) {}

#[cfg(target_os = "linux")]
fn detect() -> Option<bool> {
    if let Some(scheme) = portal_color_scheme() {
        // 0 = no preference, 1 = prefer dark, 2 = prefer light.
        match scheme {
            1 => return Some(true),
            2 => return Some(false),
            _ => {}
        }
    }

    if let Ok(theme) = std::env::var("GTK_THEME") {
        let theme = theme.to_ascii_lowercase();
        if theme.contains("dark") {
            return Some(true);
        }
        if theme.contains("light") {
            return Some(false);
        }
    }

    gsettings_color_scheme()
}

#[cfg(target_os = "linux")]
fn portal_color_scheme() -> Option<u32> {
    use std::collections::HashMap;

    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedValue;

    const DESTINATION: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const INTERFACE: &str = "org.freedesktop.portal.Settings";

    let connection = Connection::session().ok()?;

    // ReadOne is the current API; older portals only implement Read.
    if let Ok(reply) = connection.call_method(
        Some(DESTINATION),
        PATH,
        Some(INTERFACE),
        "ReadOne",
        &("org.freedesktop.appearance", "color-scheme"),
    ) && let Ok(value) = reply.body().deserialize::<OwnedValue>()
        && let Ok(scheme) = u32::try_from(&value)
    {
        return Some(scheme);
    }

    let reply = connection
        .call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "Read",
            &("org.freedesktop.appearance", vec!["color-scheme"]),
        )
        .ok()?;
    let values: HashMap<String, OwnedValue> = reply.body().deserialize().ok()?;
    u32::try_from(values.get("color-scheme")?).ok()
}

#[cfg(target_os = "linux")]
fn gsettings_color_scheme() -> Option<bool> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout);
    match value.trim().trim_matches('\'') {
        "prefer-dark" => Some(true),
        "prefer-light" | "default" => Some(false),
        _ => None,
    }
}
