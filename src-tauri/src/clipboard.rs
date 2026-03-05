use anyhow::{Result, anyhow};
use clipboard_win::{Clipboard, EnumFormats, raw};
use tracing::instrument;

/// One clipboard format entry: its Windows format ID, a human-readable name,
/// and the raw bytes stored under that format.
#[derive(Debug)]
pub struct ClipboardEntry {
    pub format_id: u32,
    pub format_name: String,
    pub data: Vec<u8>,
}

/// All format entries currently on the clipboard.
#[derive(Debug)]
pub struct ClipboardData {
    pub entries: Vec<ClipboardEntry>,
}

// Raw FFI for querying custom clipboard format names registered by apps like FileMaker.
unsafe extern "system" {
    fn GetClipboardFormatNameW(
        format: u32,
        lp_sz_format_name: *mut u16,
        cch_max_count: i32,
    ) -> i32;
}

/// Read every available clipboard format and its raw bytes.
#[instrument]
pub fn read() -> Result<ClipboardData> {
    let _clip =
        Clipboard::new_attempts(10).map_err(|e| anyhow!("Failed to open clipboard: {e:?}"))?;

    let mut entries = Vec::new();

    for fmt_id in EnumFormats::new() {
        let format_name = format_name(fmt_id);
        let mut buf = Vec::new();

        match raw::get_vec(fmt_id, &mut buf) {
            Ok(bytes) => {
                tracing::debug!(format = %format_name, bytes, "Read format");
                entries.push(ClipboardEntry {
                    format_id: fmt_id,
                    format_name,
                    data: buf,
                });
            }
            Err(e) => {
                tracing::debug!(format = %format_name, error = ?e, "Skipped unreadable format");
            }
        }
    }

    tracing::debug!(count = entries.len(), "Clipboard enumeration complete");
    Ok(ClipboardData { entries })
}

fn format_name(id: u32) -> String {
    let known = match id {
        1 => Some("CF_TEXT"),
        2 => Some("CF_BITMAP"),
        3 => Some("CF_METAFILEPICT"),
        4 => Some("CF_SYLK"),
        5 => Some("CF_DIF"),
        6 => Some("CF_TIFF"),
        7 => Some("CF_OEMTEXT"),
        8 => Some("CF_DIB"),
        9 => Some("CF_PALETTE"),
        10 => Some("CF_PENDATA"),
        11 => Some("CF_RIFF"),
        12 => Some("CF_WAVE"),
        13 => Some("CF_UNICODETEXT"),
        14 => Some("CF_ENHMETAFILE"),
        15 => Some("CF_HDROP"),
        16 => Some("CF_LOCALE"),
        17 => Some("CF_DIBV5"),
        _ => None,
    };

    if let Some(name) = known {
        return name.to_string();
    }

    let mut buf = [0u16; 256];
    let len = unsafe { GetClipboardFormatNameW(id, buf.as_mut_ptr(), buf.len() as i32) };

    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        format!("#{id}")
    }
}
