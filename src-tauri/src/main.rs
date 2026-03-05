#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod formatter;
mod parser;

use serde::Serialize;
use tracing_subscriber::EnvFilter;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardBlock {
    format_id: u32,
    format_name: String,
    /// Friendly label for the sidebar (FileMaker name or format_name).
    label: String,
    /// True for visual-preview / duplicate formats (CF_DIB, CF_BITMAP, etc.).
    is_preview: bool,
    /// "text" | "xml" | "binary"
    content_kind: String,
    /// Rendered content body ready for display.
    formatted_text: String,
}

#[tauri::command]
fn read_clipboard() -> Result<Vec<ClipboardBlock>, String> {
    let data = clipboard::read().map_err(|e| e.to_string())?;
    let parsed = parser::detect_and_parse(&data).map_err(|e| e.to_string())?;
    match parsed {
        parser::ParsedContent::Empty => Ok(vec![]),
        parser::ParsedContent::Formats(blocks) => Ok(blocks
            .iter()
            .map(|b| ClipboardBlock {
                format_id: b.format_id,
                format_name: b.format_name.clone(),
                label: formatter::sidebar_label(b),
                is_preview: b.is_preview,
                content_kind: formatter::content_kind_label(b).to_string(),
                formatted_text: formatter::format_block_content(b),
            })
            .collect()),
    }
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
