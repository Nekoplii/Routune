#[tauri::command]
fn get_gui_status() -> &'static str {
    "Routune GUI ready. Decoupled from audio thread."
}

#[tauri::command]
fn app_minimize(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn app_toggle_maximize(window: tauri::Window) -> Result<bool, String> {
    if window.is_maximized().map_err(|e| e.to_string())? {
        window.unmaximize().map_err(|e| e.to_string())?;
        Ok(false)
    } else {
        window.maximize().map_err(|e| e.to_string())?;
        Ok(true)
    }
}

#[tauri::command]
fn app_close(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn app_is_maximized(window: tauri::Window) -> Result<bool, String> {
    window.is_maximized().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn resolve_icon_path(name: &str) -> Option<std::path::PathBuf> {
    if name.is_empty() {
        return None;
    }

    let direct_path = std::path::Path::new(name);
    if direct_path.is_absolute() && direct_path.exists() {
        return Some(direct_path.to_path_buf());
    }

    let clean = name.trim().trim_end_matches(".png").trim_end_matches(".svg").to_lowercase();
    let home = std::env::var("HOME").unwrap_or_default();

    let base_dirs = [
        format!("{}/.local/share/icons/hicolor", home),
        format!("{}/.local/share/icons", home),
        "/usr/share/icons/Papirus".to_string(),
        "/usr/share/icons/hicolor".to_string(),
        "/usr/share/icons/breeze".to_string(),
        "/usr/share/icons/Adwaita".to_string(),
        "/usr/share/pixmaps".to_string(),
        "/var/lib/flatpak/exports/share/icons/hicolor".to_string(),
    ];

    let subdirs = [
        "64x64/apps",
        "48x48/apps",
        "scalable/apps",
        "128x128/apps",
        "32x32/apps",
        "256x256/apps",
        "",
    ];

    let extensions = ["svg", "png"];

    // Try primary name and variations
    let mut search_names = vec![clean.clone()];
    if clean.contains('-') {
        search_names.push(clean.replace('-', "_"));
    }
    if clean == "spotify" {
        search_names.push("spotify-client".to_string());
        search_names.push("com.spotify.Client".to_string());
    } else if clean == "discord" {
        search_names.push("com.discordapp.Discord".to_string());
        search_names.push("discord-canary".to_string());
    } else if clean == "cs2" || clean.contains("counter-strike") {
        search_names.push("steam_icon_730".to_string());
        search_names.push("steam".to_string());
    } else if clean == "firefox" {
        search_names.push("org.mozilla.firefox".to_string());
    } else if clean == "chrome" || clean == "google-chrome" {
        search_names.push("google-chrome".to_string());
        search_names.push("chromium".to_string());
    }

    for base in &base_dirs {
        for sub in &subdirs {
            for sname in &search_names {
                for ext in &extensions {
                    let path = if sub.is_empty() {
                        std::path::Path::new(base).join(format!("{}.{}", sname, ext))
                    } else {
                        std::path::Path::new(base).join(sub).join(format!("{}.{}", sname, ext))
                    };
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

#[tauri::command]
fn get_app_icon(icon_name: String, app_name: Option<String>) -> Option<String> {
    use base64::Engine;

    let path = resolve_icon_path(&icon_name)
        .or_else(|| app_name.as_deref().and_then(resolve_icon_path))?;

    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let bytes = std::fs::read(&path).ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);

    if ext == "svg" {
        Some(format!("data:image/svg+xml;base64,{}", encoded))
    } else {
        Some(format!("data:image/png;base64,{}", encoded))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_gui_status,
            app_minimize,
            app_toggle_maximize,
            app_close,
            app_is_maximized,
            get_platform,
            get_app_icon
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_icon_path() {
        // Papirus or pixmaps or hicolor should resolve spotify / discord / steam
        let res = resolve_icon_path("spotify").or_else(|| resolve_icon_path("steam"));
        assert!(res.is_some());
    }
}
