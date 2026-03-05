#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod formatter;
mod parser;

use tracing_subscriber::EnvFilter;

#[tauri::command]
fn read_clipboard() -> Result<String, String> {
    let data = clipboard::read().map_err(|e| e.to_string())?;
    let parsed = parser::detect_and_parse(&data).map_err(|e| e.to_string())?;
    Ok(formatter::format(&parsed))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![read_clipboard])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
