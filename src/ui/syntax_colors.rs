use crate::usecase::highlight::TokenKind;
use egui::Color32;

const COMMENT: Color32 = Color32::from_rgb(0x6A, 0x73, 0x7D);
const STRING_LIT: Color32 = Color32::from_rgb(0xA3, 0x15, 0x15);

/// Fixed colors, not theme-aware (same in light/dark mode) — matches the
/// Kotlin `SyntaxColors` original.
pub fn color_of(kind: TokenKind) -> Color32 {
    match kind {
        TokenKind::Comment => COMMENT,
        TokenKind::StringLit => STRING_LIT,
    }
}
