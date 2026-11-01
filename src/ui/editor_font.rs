#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamilyKind {
    Monospace,
    SansSerif,
    Serif,
}

impl FontFamilyKind {
    /// egui only ships two built-in generic families (Monospace and
    /// Proportional), unlike Compose's Monospace/SansSerif/Serif trio.
    /// SansSerif and Serif both map to Proportional for now; a real serif
    /// slot would need its own embedded font, same as `fonts.rs` did for
    /// Japanese.
    pub fn to_egui_family(self) -> egui::FontFamily {
        match self {
            FontFamilyKind::Monospace => egui::FontFamily::Monospace,
            FontFamilyKind::SansSerif | FontFamilyKind::Serif => egui::FontFamily::Proportional,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorFont {
    pub family: FontFamilyKind,
    pub size: i32,
}

impl EditorFont {
    pub const MIN_SIZE: i32 = 8;
    pub const MAX_SIZE: i32 = 48;
}

impl Default for EditorFont {
    fn default() -> Self {
        Self {
            family: FontFamilyKind::Monospace,
            size: 14,
        }
    }
}
