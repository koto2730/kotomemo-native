use image::RgbaImage;

/// Returns the clipboard content as an RGBA image if it holds one (Windows
/// Win+Shift+S, macOS Cmd+Shift+Ctrl+4, etc.). `None` for text-only
/// clipboard content or on any read failure — mirrors the Kotlin
/// `ClipboardBridge.pasteImage()`. Plain text copy/paste/cut isn't ported
/// here at all: egui's `TextEdit` already wires Ctrl+C/X/V to the system
/// clipboard (via this same `arboard` crate, pulled in transitively by
/// `eframe`) on its own.
pub fn paste_image() -> Option<RgbaImage> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let data = clipboard.get_image().ok()?;
    RgbaImage::from_raw(data.width as u32, data.height as u32, data.bytes.into_owned())
}
