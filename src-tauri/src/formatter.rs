use crate::parser::{ContentKind, FormatBlock, filemaker_label};

/// The content-type tag string for a block.
pub fn content_kind_label(block: &FormatBlock) -> &'static str {
    match &block.content {
        ContentKind::Text(_) => "text",
        ContentKind::Xml(_) => "xml",
        ContentKind::Binary { .. } => "binary",
    }
}

/// The rendered body text for a block (without header).
pub fn format_block_content(block: &FormatBlock) -> String {
    match &block.content {
        ContentKind::Text(text) => text.clone(),
        ContentKind::Xml(xml) => xml.clone(),
        ContentKind::Binary { size, hex_preview } => {
            format!("size: {size} bytes\n{hex_preview}")
        }
    }
}

/// Friendly sidebar label: FileMaker name if known, otherwise format_name.
pub fn sidebar_label(block: &FormatBlock) -> String {
    filemaker_label(&block.format_name)
        .map(|l| l.to_string())
        .unwrap_or_else(|| block.format_name.clone())
}
