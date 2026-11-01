use regex::Regex;
use std::sync::OnceLock;

const MAX_LEN: usize = 32;
const FALLBACK: &str = "untitled.txt";

fn invalid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"[\\/:*?"<>|\r\n\t]"#).unwrap())
}

/// Derives a suggested `Save As` filename from buffer content when there's
/// no existing path: first line, invalid filename chars stripped, capped at
/// 32 chars, `.txt` always appended.
pub fn from(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("").trim();
    let stripped = invalid_re().replace_all(first_line, "");
    let truncated: String = stripped.chars().take(MAX_LEN).collect();
    let sanitized = truncated.trim();
    if sanitized.is_empty() {
        FALLBACK.to_string()
    } else {
        format!("{sanitized}.txt")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_when_text_is_empty() {
        assert_eq!(from(""), "untitled.txt");
    }

    #[test]
    fn uses_first_line_trimmed() {
        assert_eq!(from("  hello  \nworld"), "hello.txt");
    }

    #[test]
    fn strips_invalid_filename_chars() {
        assert_eq!(from("ab/cd"), "abcd.txt");
        assert_eq!(from("name?<>"), "name.txt");
    }

    #[test]
    fn caps_length_at_32() {
        let long = "a".repeat(80);
        let expected = format!("{}.txt", "a".repeat(32));
        assert_eq!(from(&long), expected);
    }

    #[test]
    fn falls_back_when_first_line_is_only_whitespace_or_symbols() {
        assert_eq!(from("\n\nbody"), "untitled.txt");
        assert_eq!(from("///"), "untitled.txt");
    }
}
