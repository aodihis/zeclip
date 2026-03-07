use crate::parser::{ContentKind, FormatBlock, filemaker_label};

/// Hex dump of raw bytes (first 256 bytes, 16 bytes per row).
pub fn format_hex(data: &[u8]) -> String {
    let preview_bytes = data.len().min(256);
    let mut out = format!("size: {} bytes\n", data.len());
    for (i, chunk) in data[..preview_bytes].chunks(16).enumerate() {
        let offset = i * 16;
        out.push_str(&format!("{offset:08x}  "));
        for b in chunk {
            out.push_str(&format!("{b:02x} "));
        }
        for _ in chunk.len()..16 {
            out.push_str("   ");
        }
        out.push(' ');
        for &b in chunk {
            out.push(if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' });
        }
        out.push('\n');
    }
    if data.len() > 256 {
        out.push_str(&format!("… ({} bytes total, showing first 256)\n", data.len()));
    }
    out
}

/// The content-type tag string for a block.
pub fn content_kind_label(block: &FormatBlock) -> &'static str {
    match &block.content {
        ContentKind::Text(_) => "text",
        ContentKind::Xml(_) => "xml",
        ContentKind::Binary { .. } => "binary",
        ContentKind::Image { .. } => "image",
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
        ContentKind::Image { width, height, data_url } => {
            if data_url.is_empty() {
                format!("[BMP {width}\u{d7}{height} — too large for inline preview, use Show Binary]")
            } else {
                format!("[BMP {width}\u{d7}{height}]")
            }
        }
    }
}

/// For image blocks, returns the data URL; None for all other kinds.
pub fn image_data_url(block: &FormatBlock) -> Option<&str> {
    if let ContentKind::Image { data_url, .. } = &block.content {
        Some(data_url)
    } else {
        None
    }
}

/// Friendly sidebar label: FileMaker name if known, otherwise format_name.
pub fn sidebar_label(block: &FormatBlock) -> String {
    filemaker_label(&block.format_name)
        .map(|l| l.to_string())
        .unwrap_or_else(|| block.format_name.clone())
}
