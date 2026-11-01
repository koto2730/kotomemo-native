pub mod bulk_indent;
pub mod find_replace;
pub mod highlight;
pub mod json_path;
pub mod send_request;
pub mod template;

/// Converts a byte offset (as produced by the `regex` crate, which operates
/// on UTF-8 bytes) into a char offset (as used everywhere else in this
/// module tree, matching Kotlin's UTF-16-code-unit indexing closely enough
/// for BMP text, and matching egui's char-index-based cursor model).
pub(crate) fn char_index_at_byte(text: &str, byte_idx: usize) -> usize {
    text[..byte_idx].chars().count()
}
