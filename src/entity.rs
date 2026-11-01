use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const UNTITLED: &str = "Untitled";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentsId(pub String);

impl ContentsId {
    pub fn generate() -> Self {
        // No uuid crate needed: this only has to be a stable per-tab
        // identity key (used as an egui/list key), not a globally unique or
        // parseable UUID. Nanosecond timestamp + a process-local monotonic
        // counter is enough to guarantee uniqueness within one run.
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!("{nanos:x}-{seq:x}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    Utf8,
    Utf16Be,
    Utf16Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn sequence(&self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }

    pub fn detect(text: &str) -> Self {
        if text.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    }

    pub fn platform_default() -> Self {
        if cfg!(windows) {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    }
}

#[derive(Debug, Clone)]
pub struct Contents {
    pub id: ContentsId,
    pub text: String,
    pub file_path: Option<PathBuf>,
    pub charset: Charset,
    pub line_ending: LineEnding,
    pub has_bom: bool,
    pub is_dirty: bool,
}

impl Contents {
    pub fn empty() -> Self {
        Self {
            id: ContentsId::generate(),
            text: String::new(),
            file_path: None,
            charset: Charset::Utf8,
            line_ending: LineEnding::platform_default(),
            has_bom: false,
            is_dirty: false,
        }
    }

    pub fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNTITLED.to_string())
    }

    pub fn with_text(&self, new_text: impl Into<String>) -> Self {
        let new_text = new_text.into();
        let is_dirty = new_text != self.text || self.is_dirty;
        Self {
            text: new_text,
            is_dirty,
            ..self.clone()
        }
    }

    pub fn mark_saved(&self, path: PathBuf) -> Self {
        Self {
            file_path: Some(path),
            is_dirty: false,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Default for HttpMethod {
    fn default() -> Self {
        HttpMethod::Post
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseTarget {
    NewTab,
    AfterSelection,
    StatusOnly,
}

impl Default for ResponseTarget {
    fn default() -> Self {
        ResponseTarget::NewTab
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiPreset {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub method: HttpMethod,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body_template: String,
    #[serde(default)]
    pub response_json_path: Option<String>,
    #[serde(default)]
    pub response_target: ResponseTarget,
}

impl Default for ApiPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            url: String::new(),
            method: HttpMethod::default(),
            headers: HashMap::new(),
            body_template: String::new(),
            response_json_path: None,
            response_target: ResponseTarget::default(),
        }
    }
}

pub const DEFAULT_ATTACHMENTS_FOLDER: &str = "attachments";
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

fn default_attachments_folder() -> String {
    DEFAULT_ATTACHMENTS_FOLDER.to_string()
}

fn default_request_timeout_secs() -> u64 {
    DEFAULT_REQUEST_TIMEOUT_SECS
}

fn default_show_line_numbers() -> bool {
    true
}

fn default_word_wrap() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub presets: Vec<ApiPreset>,
    #[serde(default)]
    pub tokens: HashMap<String, String>,
    #[serde(default = "default_attachments_folder")]
    pub attachments_folder: String,
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
    #[serde(default = "default_word_wrap")]
    pub word_wrap: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            presets: Vec::new(),
            tokens: HashMap::new(),
            attachments_folder: default_attachments_folder(),
            request_timeout_secs: default_request_timeout_secs(),
            show_line_numbers: default_show_line_numbers(),
            word_wrap: default_word_wrap(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_creates_an_untitled_non_dirty_contents() {
        let c = Contents::empty();
        assert_eq!(c.text, "");
        assert!(c.file_path.is_none());
        assert!(!c.is_dirty);
        assert_eq!(c.display_name(), UNTITLED);
    }

    #[test]
    fn with_text_flips_dirty_when_text_changes() {
        let c = Contents::empty().with_text("hello");
        assert!(c.is_dirty);
        assert_eq!(c.text, "hello");
    }

    #[test]
    fn with_text_keeps_clean_state_when_text_is_identical() {
        let c = Contents::empty();
        let same = c.with_text("");
        assert!(!same.is_dirty);
    }

    #[test]
    fn detects_crlf() {
        assert_eq!(LineEnding::detect("a\r\nb"), LineEnding::Crlf);
    }

    #[test]
    fn detects_lf_when_no_crlf_present() {
        assert_eq!(LineEnding::detect("a\nb"), LineEnding::Lf);
    }

    #[test]
    fn detects_lf_on_empty_input() {
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
    }
}
