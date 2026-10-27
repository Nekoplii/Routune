#[tauri::command]
fn get_gui_status() -> &'static str {
    "Routune GUI ready. Decoupled from audio thread."
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_gui_status])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
