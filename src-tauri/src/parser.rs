use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use quick_xml::{Reader, Writer, events::Event};
use tracing::instrument;

use crate::clipboard::{ClipboardData, ClipboardEntry};

/// How we decoded a single clipboard format.
#[derive(Debug)]
pub enum ContentKind {
    /// Valid text.
    Text(String),
    /// XML that has been pretty-printed.
    Xml(String),
    /// Raw bytes shown as a hex dump.
    Binary { size: usize, hex_preview: String },
    /// Bitmap image with a data URL for inline display.
    Image { width: u32, height: u32, data_url: String },
}

/// One decoded clipboard format block ready for display.
#[derive(Debug)]
pub struct FormatBlock {
    pub format_id: u32,
    pub format_name: String,
    /// True if this entry is a visual preview/duplicate (e.g. CF_DIB, CF_BITMAP).
    pub is_preview: bool,
    pub content: ContentKind,
    /// Raw bytes from the clipboard for this format (used for binary toggle view).
    pub raw_bytes: Vec<u8>,
}

/// All decoded blocks from the clipboard.
#[derive(Debug)]
pub enum ParsedContent {
    Formats(Vec<FormatBlock>),
    Empty,
}

/// FileMaker Windows clipboard format names and their friendly descriptions.
const FILEMAKER_FORMATS: &[(&str, &str)] = &[
    ("Mac-XMFD", "FileMaker Field"),
    ("Mac-XMSC", "FileMaker Script"),
    ("Mac-XMSS", "FileMaker Script Step"),
    ("Mac-XML2", "FileMaker Layout Object"),
    ("Mac-XMFN", "FileMaker Custom Function"),
    ("Mac-XMTB", "FileMaker Table"),
];

pub fn filemaker_label(format_name: &str) -> Option<&'static str> {
    FILEMAKER_FORMATS
        .iter()
        .find(|(name, _)| *name == format_name)
        .map(|(_, label)| *label)
}

fn is_filemaker_format(name: &str) -> bool {
    filemaker_label(name).is_some()
}

/// Format IDs that are always visual previews / rendering hints with no
/// actionable data content.
///
/// - 2  CF_BITMAP       (HBITMAP handle, not raw pixel data)
/// - 3  CF_METAFILEPICT
/// - 9  CF_PALETTE
/// - 14 CF_ENHMETAFILE
/// - 16 CF_LOCALE  (locale tag for the text, not content itself)
///
/// CF_DIB (8) and CF_DIBV5 (17) are NOT here — they contain real pixel data
/// and are decoded as images.
const PREVIEW_FORMAT_IDS: &[u32] = &[2, 3, 9, 14, 16];

/// Returns true if this entry should be suppressed as a preview / duplicate.
fn is_preview(entry: &ClipboardEntry, all: &[ClipboardEntry]) -> bool {
    // Drop known image/graphics/metadata format IDs.
    if PREVIEW_FORMAT_IDS.contains(&entry.format_id) {
        return true;
    }

    // Mac-PICT is FileMaker's picture preview.
    if entry.format_name == "Mac-PICT" {
        return true;
    }

    // CF_TEXT (1) and CF_OEMTEXT (7) are redundant when CF_UNICODETEXT (13) is present.
    if (entry.format_id == 1 || entry.format_id == 7) && all.iter().any(|e| e.format_id == 13) {
        return true;
    }

    false
}

/// Detect and parse every format entry on the clipboard.
#[instrument(skip(data))]
pub fn detect_and_parse(data: &ClipboardData) -> Result<ParsedContent> {
    if data.entries.is_empty() {
        tracing::debug!("Clipboard is empty");
        return Ok(ParsedContent::Empty);
    }

    let blocks: Vec<FormatBlock> = data
        .entries
        .iter()
        .filter_map(|entry| {
            let preview = is_preview(entry, &data.entries);
            parse_entry(entry).ok().map(|mut b| {
                b.is_preview = preview;
                b
            })
        })
        .collect();

    if blocks.is_empty() {
        Ok(ParsedContent::Empty)
    } else {
        Ok(ParsedContent::Formats(blocks))
    }
}

fn parse_entry(entry: &ClipboardEntry) -> Result<FormatBlock> {
    let content = match entry.format_id {
        // CF_UNICODETEXT (13) — UTF-16LE, null-terminated
        13 => decode_utf16le(&entry.data),
        // CF_TEXT (1) / CF_OEMTEXT (7) — try UTF-8, fall back to Latin-1
        1 | 7 => decode_ansi(&entry.data),
        // CF_DIB (8) / CF_DIBV5 (17) — Device Independent Bitmap
        8 | 17 => decode_dib(&entry.data),
        // CF_HTML — UTF-8 with a Windows header block
        _ if entry.format_name == "HTML Format" => decode_ansi(&entry.data),
        // FileMaker formats — 4-byte LE length prefix followed by UTF-8 XML
        _ if is_filemaker_format(&entry.format_name) => decode_filemaker(&entry.data),
        // Everything else
        _ => decode_best_effort(&entry.data),
    };

    // If we decoded text that looks like XML, pretty-print it.
    let content = if let ContentKind::Text(ref text) = content {
        let trimmed = text.trim();
        if trimmed.starts_with('<') {
            match pretty_print_xml(trimmed) {
                Ok(xml) => ContentKind::Xml(xml),
                Err(_) => content,
            }
        } else {
            content
        }
    } else {
        content
    };

    Ok(FormatBlock {
        format_id: entry.format_id,
        format_name: entry.format_name.clone(),
        is_preview: false, // set by detect_and_parse
        content,
        raw_bytes: entry.data.clone(),
    })
}

/// Decode FileMaker's clipboard format: 4-byte LE uint32 length prefix + UTF-8 XML body.
fn decode_filemaker(data: &[u8]) -> ContentKind {
    if data.len() < 4 {
        return hex_block(data);
    }
    let declared_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let body = &data[4..data.len().min(4 + declared_len)];
    match std::str::from_utf8(body) {
        Ok(s) => ContentKind::Text(s.to_string()),
        Err(_) => hex_block(data),
    }
}

/// Decode UTF-16LE (standard Windows Unicode clipboard format).
fn decode_utf16le(data: &[u8]) -> ContentKind {
    if data.len() < 2 {
        return hex_block(data);
    }
    let words: Vec<u16> = data
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .take_while(|&w| w != 0)
        .collect();
    ContentKind::Text(String::from_utf16_lossy(&words).to_owned())
}

/// Decode plain-text clipboard formats (CF_TEXT, HTML Format header, etc.).
fn decode_ansi(data: &[u8]) -> ContentKind {
    let trimmed = data
        .iter()
        .rposition(|&b| b != 0)
        .map_or(&data[..0], |i| &data[..=i]);
    match std::str::from_utf8(trimmed) {
        Ok(s) => ContentKind::Text(s.to_string()),
        Err(_) => ContentKind::Text(trimmed.iter().map(|&b| b as char).collect()),
    }
}

/// Try UTF-8, then UTF-16LE, then fall back to a hex dump.
fn decode_best_effort(data: &[u8]) -> ContentKind {
    let trimmed = data
        .iter()
        .rposition(|&b| b != 0)
        .map_or(&data[..0], |i| &data[..=i]);

    if let Ok(s) = std::str::from_utf8(trimmed) {
        return ContentKind::Text(s.to_string());
    }

    if data.len() >= 2 && data.len().is_multiple_of(2) {
        let words: Vec<u16> = data
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        if words.iter().all(|&w| !(0xD800..=0xDFFF).contains(&w)) {
            let s = String::from_utf16_lossy(&words);
            let s = s.trim_end_matches('\0');
            if !s.is_empty() {
                return ContentKind::Text(s.to_string());
            }
        }
    }

    hex_block(data)
}

/// Decode a CF_DIB or CF_DIBV5 clipboard entry into a displayable image.
///
/// Both formats are a raw BITMAPINFO (no file header). We prepend a
/// BITMAPFILEHEADER (14 bytes) to form a valid BMP, then base64-encode it as a
/// data URL that the browser can display directly.
fn decode_dib(data: &[u8]) -> ContentKind {
    // Need at least a BITMAPINFOHEADER (40 bytes).
    if data.len() < 40 {
        return hex_block(data);
    }

    let bi_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if data.len() < bi_size {
        return hex_block(data);
    }

    let width = i32::from_le_bytes([data[4], data[5], data[6], data[7]]).unsigned_abs();
    let height = i32::from_le_bytes([data[8], data[9], data[10], data[11]]).unsigned_abs();
    let bit_count = u16::from_le_bytes([data[14], data[15]]);
    let clr_used = u32::from_le_bytes([data[32], data[33], data[34], data[35]]);

    // Number of RGBQUAD entries in the color table.
    let color_entries = if clr_used > 0 {
        clr_used as usize
    } else if bit_count <= 8 {
        1usize << bit_count
    } else {
        0
    };

    let bf_off_bits = (14 + bi_size + color_entries * 4) as u32;
    let bf_size = (14 + data.len()) as u32;

    // Build a BMP file in memory: 14-byte file header + DIB data.
    let mut bmp = Vec::with_capacity(14 + data.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&bf_size.to_le_bytes());
    bmp.extend_from_slice(&[0u8; 4]); // reserved
    bmp.extend_from_slice(&bf_off_bits.to_le_bytes());
    bmp.extend_from_slice(data);

    let data_url = format!("data:image/bmp;base64,{}", BASE64.encode(&bmp));
    ContentKind::Image { width, height, data_url }
}

fn hex_block(data: &[u8]) -> ContentKind {
    let preview_bytes = data.len().min(256);
    let mut hex = String::new();
    for (i, chunk) in data[..preview_bytes].chunks(16).enumerate() {
        let offset = i * 16;
        hex.push_str(&format!("{offset:08x}  "));
        for b in chunk {
            hex.push_str(&format!("{b:02x} "));
        }
        for _ in chunk.len()..16 {
            hex.push_str("   ");
        }
        hex.push(' ');
        for &b in chunk {
            let c = if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            };
            hex.push(c);
        }
        hex.push('\n');
    }
    ContentKind::Binary {
        size: data.len(),
        hex_preview: hex,
    }
}

fn pretty_print_xml(input: &str) -> Result<String> {
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);

    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);

    loop {
        match reader.read_event().context("Failed to read XML event")? {
            Event::Eof => break,
            event => writer
                .write_event(event)
                .context("Failed to write XML event")?,
        }
    }

    String::from_utf8(writer.into_inner()).context("XML output is not valid UTF-8")
}
