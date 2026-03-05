use anyhow::{Context, Result};
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
}

/// One decoded clipboard format block ready for display.
#[derive(Debug)]
pub struct FormatBlock {
    pub format_id: u32,
    pub format_name: String,
    pub content: ContentKind,
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
/// - 2  CF_BITMAP
/// - 3  CF_METAFILEPICT
/// - 8  CF_DIB
/// - 9  CF_PALETTE
/// - 14 CF_ENHMETAFILE
/// - 17 CF_DIBV5
/// - 16 CF_LOCALE  (locale tag for the text, not content itself)
const PREVIEW_FORMAT_IDS: &[u32] = &[2, 3, 8, 9, 14, 16, 17];

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
        .filter(|e| !is_preview(e, &data.entries))
        .filter_map(|entry| parse_entry(entry).ok())
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
        content,
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
