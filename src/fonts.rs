use std::path::PathBuf;
use std::sync::Arc;

/// Built-in and system font choices for the editor.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FontSelection {
    /// Bundled Noto Sans JP (SIL OFL 1.1, assets/fonts/NotoSansJP-Regular.otf).
    NotoSansJp,
    /// Windows BIZ UDGothic (monospace Japanese + ASCII).
    BizUdGothic,
    /// Windows BIZ UDPGothic (proportional Japanese + ASCII).
    BizUdpGothic,
    /// Windows Meiryo.
    Meiryo,
    /// Windows Yu Gothic Medium/Regular.
    YuGothic,
    /// Windows MS Gothic (monospace Japanese + ASCII).
    MsGothic,
    /// Windows BIZ UDMincho.
    BizUdMincho,
    /// Noto Serif JP (if installed on system).
    NotoSerifJp,
    /// User-selected font file (.ttf / .otf / .ttc).
    Custom {
        path: String,
        #[serde(default)]
        index: u32,
    },
}

impl Default for FontSelection {
    fn default() -> Self {
        Self::NotoSansJp
    }
}

impl FontSelection {
    pub fn label(&self) -> String {
        match self {
            Self::NotoSansJp => "Noto Sans JP (標準・内蔵)".to_string(),
            Self::BizUdGothic => "BIZ UDGothic (等幅・UDゴシック)".to_string(),
            Self::BizUdpGothic => "BIZ UDPGothic (Pゴシック)".to_string(),
            Self::Meiryo => "Meiryo (メイリオ)".to_string(),
            Self::YuGothic => "Yu Gothic (游ゴシック)".to_string(),
            Self::MsGothic => "MS Gothic (ＭＳ ゴシック・等幅)".to_string(),
            Self::BizUdMincho => "BIZ UDMincho (UD明朝)".to_string(),
            Self::NotoSerifJp => "Noto Serif JP (明朝)".to_string(),
            Self::Custom { path, .. } => {
                let name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                format!("Custom ({name})")
            }
        }
    }

    /// Resolves `(file_path, face_index)` for system/custom fonts, or `None` for the bundled font.
    pub fn resolve_path_and_index(&self) -> Option<(PathBuf, u32)> {
        match self {
            Self::NotoSansJp => None,
            Self::BizUdGothic => find_system_font(&["BIZ-UDGothicR.ttc", "BIZ-UDGothicB.ttc"]).map(|p| (p, 0)),
            Self::BizUdpGothic => find_system_font(&["BIZ-UDGothicR.ttc", "BIZ-UDGothicB.ttc"]).map(|p| (p, 1)),
            Self::Meiryo => find_system_font(&["meiryo.ttc", "Meiryo.ttf"]).map(|p| (p, 0)),
            Self::YuGothic => find_system_font(&["YuGothM.ttc", "YuGothR.ttc", "YuGothic-Medium.otf"]).map(|p| (p, 0)),
            Self::MsGothic => find_system_font(&["msgothic.ttc", "MS-Gothic.ttf"]).map(|p| (p, 0)),
            Self::BizUdMincho => find_system_font(&["BIZ-UDMinchoM.ttc"]).map(|p| (p, 0)),
            Self::NotoSerifJp => find_system_font(&[
                "NotoSerifJP-VF.ttf",
                "NotoSerifCJKjp-Regular.otf",
                "NotoSerifJP-Regular.otf",
            ])
            .map(|p| (p, 0)),
            Self::Custom { path, index } => {
                let p = PathBuf::from(path);
                if p.is_file() {
                    Some((p, *index))
                } else {
                    None
                }
            }
        }
    }

    pub fn is_available(&self) -> bool {
        match self {
            Self::NotoSansJp => true,
            _ => self.resolve_path_and_index().is_some(),
        }
    }

    pub fn builtin_presets() -> Vec<Self> {
        vec![
            Self::NotoSansJp,
            Self::BizUdGothic,
            Self::BizUdpGothic,
            Self::Meiryo,
            Self::YuGothic,
            Self::MsGothic,
            Self::BizUdMincho,
            Self::NotoSerifJp,
        ]
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FontConfig {
    #[serde(default)]
    pub selection: FontSelection,
    #[serde(default = "default_font_size")]
    pub size: f32,
}

fn default_font_size() -> f32 {
    14.0
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            selection: FontSelection::NotoSansJp,
            size: default_font_size(),
        }
    }
}

impl FontConfig {
    pub const MIN_SIZE: f32 = 8.0;
    pub const MAX_SIZE: f32 = 48.0;

    pub fn clamped_size(&self) -> f32 {
        self.size.clamp(Self::MIN_SIZE, Self::MAX_SIZE)
    }
}

pub fn font_config_path() -> PathBuf {
    crate::io::config::config_dir().join("font.json")
}

pub fn load_config() -> FontConfig {
    let path = font_config_path();
    let mut cfg: FontConfig = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    cfg.size = cfg.clamped_size();
    cfg
}

pub fn save_config(config: &FontConfig) -> Result<(), String> {
    let path = font_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write {path:?}: {e}"))
}

fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(windir) = std::env::var_os("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        } else {
            dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        }
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local_app_data).join(r"Microsoft\Windows\Fonts"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            let home_path = PathBuf::from(home);
            dirs.push(home_path.join(".local/share/fonts"));
            dirs.push(home_path.join(".fonts"));
        }
    }
    dirs
}

fn find_system_font(candidates: &[&str]) -> Option<PathBuf> {
    for dir in system_font_dirs() {
        for name in candidates {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Builds `egui::FontDefinitions` for `config`.
///
/// Key fix for ASCII vs. multi-byte glyph mismatch:
/// Previously `noto_sans_jp` was appended with `.push(...)` after egui's
/// default Latin-only fonts (`Hack` in Monospace, `Ubuntu-Light` in
/// Proportional). That caused ASCII digits/letters/symbols to render in
/// `Hack` (which looks noticeably bolder/heavier) while multi-byte
/// characters (`０-９`, `Ａ-Ｚ`, Kana, Kanji) fell back to `NotoSansJP`.
/// By inserting the CJK+ASCII font at index `0` (`.insert(0, ...)`), both
/// ASCII and multi-byte characters are rendered from the exact same font.
pub fn build_font_definitions(config: &FontConfig) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "noto_sans_jp".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/NotoSansJP-Regular.otf"
        ))),
    );

    // Place Noto Sans JP at index 0 as the base primary font for both families
    // so ASCII and multi-byte characters share unified glyph metrics and weight.
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "noto_sans_jp".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "noto_sans_jp".to_owned());

    // If a specific system or custom font is selected and readable, place it
    // at index 0 ahead of noto_sans_jp (keeping noto_sans_jp as immediate fallback).
    if let Some((path, index)) = config.selection.resolve_path_and_index() {
        if let Ok(bytes) = std::fs::read(&path) {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            fonts.font_data.insert("selected_editor_font".to_owned(), Arc::new(data));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "selected_editor_font".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "selected_editor_font".to_owned());
        }
    }

    fonts
}

/// Applies `config` to `ctx`.
pub fn apply(ctx: &egui::Context, config: &FontConfig) {
    ctx.set_fonts(build_font_definitions(config));
}

/// Loads saved font configuration (or default) and applies it to `ctx`.
pub fn setup(ctx: &egui::Context) {
    let config = load_config();
    apply(ctx, &config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_font_definitions_put_noto_sans_jp_first_in_both_families() {
        let cfg = FontConfig::default();
        let defs = build_font_definitions(&cfg);
        let prop = defs.families.get(&egui::FontFamily::Proportional).unwrap();
        let mono = defs.families.get(&egui::FontFamily::Monospace).unwrap();
        assert_eq!(prop.first().map(String::as_str), Some("noto_sans_jp"));
        assert_eq!(mono.first().map(String::as_str), Some("noto_sans_jp"));
    }

    #[test]
    fn font_config_round_trips_json() {
        let cfg = FontConfig {
            selection: FontSelection::BizUdGothic,
            size: 16.0,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: FontConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, cfg);
    }
}
