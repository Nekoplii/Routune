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

#[cfg(target_os = "linux")]
#[repr(C)]
struct GLogField {
    key: *const std::ffi::c_char,
    value: *const std::ffi::c_void,
    length: isize,
}

#[cfg(target_os = "linux")]
unsafe extern "C" fn structured_log_writer(
    log_level: std::ffi::c_uint,
    fields: *const GLogField,
    n_fields: usize,
    user_data: *mut std::ffi::c_void,
) -> std::ffi::c_int {
    if !fields.is_null() {
        for i in 0..n_fields {
            let field = unsafe { &*fields.add(i) };
            if !field.key.is_null() && !field.value.is_null() {
                let key = unsafe { std::ffi::CStr::from_ptr(field.key).to_string_lossy() };
                if key == "MESSAGE" {
                    let msg: std::borrow::Cow<str> = if field.length < 0 {
                        unsafe { std::ffi::CStr::from_ptr(field.value as *const std::ffi::c_char).to_string_lossy() }
                    } else {
                        let slice = unsafe { std::slice::from_raw_parts(field.value as *const u8, field.length as usize) };
                        String::from_utf8_lossy(slice).into_owned().into()
                    };
                    if msg.contains("gtk_window_set_titlebar") {
                        return 1; // G_LOG_WRITER_HANDLED — suppress
                    }
                    break;
                }
            }
        }
    }
    unsafe extern "C" {
        fn g_log_writer_default(
            log_level: std::ffi::c_uint,
            fields: *const GLogField,
            n_fields: usize,
            user_data: *mut std::ffi::c_void,
        ) -> std::ffi::c_int;
    }
    unsafe { g_log_writer_default(log_level, fields, n_fields, user_data) }
}

#[cfg(target_os = "linux")]
fn silence_gtk_warnings() {
    unsafe extern "C" {
        fn g_log_set_writer_func(
            func: unsafe extern "C" fn(
                std::ffi::c_uint,
                *const GLogField,
                usize,
                *mut std::ffi::c_void,
            ) -> std::ffi::c_int,
            user_data: *mut std::ffi::c_void,
            user_data_free: *const std::ffi::c_void,
        );
    }
    unsafe {
        g_log_set_writer_func(structured_log_writer, std::ptr::null_mut(), std::ptr::null());
    }
}

#[tauri::command]
fn sync_hyprland_shortcuts(
    mute_mic: String,
    mute_master: String,
    mute_game: String,
    mute_chat: String,
    mute_media: String,
    mute_aux: String,
) -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").map_err(|e| e.to_string())?;
        let hypr_dir = std::path::Path::new(&home).join(".config/hypr");
        let hypr_lua = hypr_dir.join("hyprland.lua");

        if !hypr_lua.exists() {
            return Ok(false);
        }

        // Ensure ~/.local/bin/routune-daemon symlink exists
        let local_bin = std::path::Path::new(&home).join(".local/bin");
        let symlink_path = local_bin.join("routune-daemon");
        if !symlink_path.exists() {
            let _ = std::fs::create_dir_all(&local_bin);
            if let Ok(exe) = std::env::current_exe()
                && let Some(bin_dir) = exe.parent() {
                    let candidate = bin_dir.join("routune-daemon");
                    if candidate.exists() {
                        let _ = std::os::unix::fs::symlink(candidate, &symlink_path);
                    }
                }
        }

        // Ensure hyprland.lua contains optional("routune")
        if let Ok(content) = std::fs::read_to_string(&hypr_lua)
            && !content.contains("optional(\"routune\")") {
                let updated = format!("{}\n\n-- Routune hotkeys (the app owns it)\noptional(\"routune\")\n", content.trim_end());
                let _ = std::fs::write(&hypr_lua, updated);
            }

        fn to_hypr_combo(combo: &str) -> Option<String> {
            let trimmed = combo.trim();
            if trimmed.is_empty() {
                return None;
            }
            let parts: Vec<String> = trimmed.split('+').map(|p| {
                match p.trim().to_uppercase().as_str() {
                    "CTRL" | "CMDORCTRL" => "CTRL".to_string(),
                    "CMD" | "SUPER" => "SUPER".to_string(),
                    "ALT" => "ALT".to_string(),
                    "SHIFT" => "SHIFT".to_string(),
                    other => other.to_string(),
                }
            }).collect();
            Some(parts.join(" + "))
        }

        let mut lua_content = String::from("-- Routune Global Shortcuts (managed automatically by Routune)\n");
        if let Some(c) = to_hypr_combo(&mute_mic) {
            lua_content.push_str(&format!("hl.bind(\"{c}\", hl.dsp.exec_cmd(\"routune-daemon --toggle-mute mic\"))\n"));
        }
        if let Some(c) = to_hypr_combo(&mute_master) {
            lua_content.push_str(&format!("hl.bind(\"{c}\", hl.dsp.exec_cmd(\"routune-daemon --toggle-mute master\"))\n"));
        }
        if let Some(c) = to_hypr_combo(&mute_game) {
            lua_content.push_str(&format!("hl.bind(\"{c}\", hl.dsp.exec_cmd(\"routune-daemon --toggle-mute game\"))\n"));
        }
        if let Some(c) = to_hypr_combo(&mute_chat) {
            lua_content.push_str(&format!("hl.bind(\"{c}\", hl.dsp.exec_cmd(\"routune-daemon --toggle-mute chat\"))\n"));
        }
        if let Some(c) = to_hypr_combo(&mute_media) {
            lua_content.push_str(&format!("hl.bind(\"{c}\", hl.dsp.exec_cmd(\"routune-daemon --toggle-mute media\"))\n"));
        }
        if let Some(c) = to_hypr_combo(&mute_aux) {
            lua_content.push_str(&format!("hl.bind(\"{c}\", hl.dsp.exec_cmd(\"routune-daemon --toggle-mute aux\"))\n"));
        }

        let routune_lua = hypr_dir.join("routune.lua");
        std::fs::write(&routune_lua, lua_content).map_err(|e| e.to_string())?;

        // Reload Hyprland config dynamically
        let _ = std::process::Command::new("hyprctl")
            .args(["reload", "config-only"])
            .status();

        Ok(true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (mute_mic, mute_master, mute_game, mute_chat, mute_media, mute_aux);
        Ok(false)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    silence_gtk_warnings();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_gui_status,
            app_minimize,
            app_toggle_maximize,
            app_close,
            app_is_maximized,
            get_platform,
            get_app_icon,
            sync_hyprland_shortcuts
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
