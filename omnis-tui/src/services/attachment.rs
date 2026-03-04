/// Build `PendingAttachment` from a file path or clipboard image,
/// including a terminal pixel-preview for image files.
use crate::types::{PendingAttachment, PreviewCell};

// ── Public entry points ────────────────────────────────────────────────────

/// Read `path` metadata and (for images) generate a pixel preview.
pub fn build_pending_attachment(path: &str) -> crate::error::Result<PendingAttachment> {
    let p = std::path::Path::new(path);

    let filename = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "attachment".to_string());

    let file_size = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);

    let ext = p
        .extension()
        .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
        .unwrap_or_default();

    let file_type = classify_extension(&ext).to_string();

    // Build pixel preview only for images
    let pixel_preview = if file_type == "image" {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| build_image_preview(&bytes, 52, 10))
    } else {
        None
    };

    Ok(PendingAttachment {
        path: path.to_string(),
        filename,
        file_type,
        file_size,
        caption: String::new(),
        pixel_preview,
    })
}

/// Map a lowercase file extension to a broad category label.
pub fn classify_extension(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "avif" => "image",
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v"                    => "video",
        "mp3" | "m4a" | "ogg" | "opus" | "flac" | "wav" | "aac"           => "audio",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx"
        | "txt" | "md" | "csv" | "rtf"                                    => "document",
        _                                                                   => "file",
    }
}

// ── Image preview ──────────────────────────────────────────────────────────

/// Public wrapper for external callers (e.g. after a media download).
pub fn build_image_preview_pub(bytes: &[u8], max_cols: u32, max_rows: u32) -> Option<Vec<Vec<PreviewCell>>> {
    build_image_preview(bytes, max_cols, max_rows)
}

/// Decode an image from raw bytes and render it as a grid of coloured
/// Unicode half-block cells ready for Ratatui.
///
/// Uses the `▀` (U+2580 UPPER HALF BLOCK) character: the terminal cell
/// foreground colour represents the top pixel and the background colour
/// represents the bottom pixel, giving 2× vertical resolution.
///
/// `max_cols` / `max_rows` are terminal character dimensions (rows × 2
/// actual pixel rows).  Returns `None` if the image cannot be decoded.
fn build_image_preview(bytes: &[u8], max_cols: u32, max_rows: u32) -> Option<Vec<Vec<PreviewCell>>> {
    let img = image::load_from_memory(bytes).ok()?;

    // Scale so pixels fit within max_cols wide × (max_rows * 2) tall
    let pix_h = max_rows * 2;
    let img = img.thumbnail(max_cols, pix_h).to_rgba8();
    let (w, h) = img.dimensions();

    let mut lines: Vec<Vec<PreviewCell>> = Vec::new();

    let mut row = 0u32;
    while row < h {
        let mut line: Vec<PreviewCell> = Vec::with_capacity(w as usize);
        for col in 0..w {
            let tp = img.get_pixel(col, row);
            let bp = if row + 1 < h {
                *img.get_pixel(col, row + 1)
            } else {
                *tp
            };
            line.push(('\u{2580}', (tp[0], tp[1], tp[2]), (bp[0], bp[1], bp[2])));
        }
        lines.push(line);
        row += 2;
    }

    Some(lines)
}

// ── Human-readable file size ───────────────────────────────────────────────

pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1_024 {
        format!("{} B", bytes)
    } else if bytes < 1_024 * 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else if bytes < 1_024 * 1_024 * 1_024 {
        format!("{:.1} MB", bytes as f64 / 1_024.0 / 1_024.0)
    } else {
        format!("{:.1} GB", bytes as f64 / 1_024.0 / 1_024.0 / 1_024.0)
    }
}
