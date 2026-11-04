// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Runs Tauri Builder with registered plugins:
    // - tauri_plugin_autostart
    // - tauri_plugin_global_shortcut
    routune_gui_lib::run();
}
