use std::path::{Path, PathBuf};

/// Resolves the shared attachments folder for a tab, Obsidian-style
/// (sibling folder next to the edited file): for `D:\notes\my-notes.txt`,
/// images live under `D:\notes\{folder_name}\`. `None` for files with no
/// on-disk path (Untitled), so callers can gate paste/view behaviour with
/// a "save first" prompt. Ports the Kotlin `AttachmentsPath`.
pub fn folder_for(file_path: Option<&Path>, folder_name: &str) -> Option<PathBuf> {
    let file_path = file_path?;
    let name = if folder_name.trim().is_empty() {
        crate::entity::DEFAULT_ATTACHMENTS_FOLDER
    } else {
        folder_name
    };
    file_path.parent().map(|p| p.join(name))
}
