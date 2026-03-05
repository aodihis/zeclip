use crate::parser::{ContentKind, FormatBlock, ParsedContent, filemaker_label};

/// Convert all parsed clipboard format blocks into a display string.
pub fn format(content: &ParsedContent) -> String {
    match content {
        ParsedContent::Empty => "--- content-type: empty ---\n[clipboard is empty]\n".to_string(),
        ParsedContent::Formats(blocks) => blocks
            .iter()
            .map(format_block)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn format_block(block: &FormatBlock) -> String {
    // Show a friendly label for known FileMaker formats.
    let label = filemaker_label(&block.format_name)
        .map(|l| format!(" ({l})"))
        .unwrap_or_default();

    let header = format!(
        "=== format #{} : {}{} ===",
        block.format_id, block.format_name, label
    );

    let body = match &block.content {
        ContentKind::Text(text) => format!("content-type: text/plain\n{text}"),
        ContentKind::Xml(xml) => format!("content-type: text/xml\n{xml}"),
        ContentKind::Binary { size, hex_preview } => {
            format!("content-type: application/octet-stream\nsize: {size} bytes\n{hex_preview}")
        }
    };

    format!("{header}\n{body}")
}
