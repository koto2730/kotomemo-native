use super::char_index_at_byte;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Comment,
    StringLit,
}

pub struct SyntaxRule {
    pub kind: TokenKind,
    pub pattern: Regex,
}

pub struct SyntaxRuleSet {
    pub name: &'static str,
    pub rules: Vec<SyntaxRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end_exclusive: usize,
    pub kind: TokenKind,
}

/// Greedy left-to-right scan: at each position try each rule's pattern
/// anchored exactly there, take the first match, advance past it; else
/// advance one char. Recomputed from scratch on every call (O(text length)
/// full rescan, not incremental) — same complexity the Kotlin original
/// accepted at notepad scale.
pub fn highlight(text: &str, rule_set: Option<&SyntaxRuleSet>) -> Vec<Token> {
    let Some(rule_set) = rule_set else {
        return Vec::new();
    };
    if text.is_empty() {
        return Vec::new();
    }
    let mut tokens = Vec::new();
    let mut byte_i = 0usize;
    while byte_i < text.len() {
        let mut matched = false;
        for rule in &rule_set.rules {
            let candidate = &text[byte_i..];
            // A leftmost-first match starting exactly at 0 in `candidate`
            // is equivalent to Kotlin's `Regex.matchAt(text, i)`.
            if let Some(m) = rule.pattern.find(candidate) {
                if m.start() == 0 && m.end() > 0 {
                    let start_char = char_index_at_byte(text, byte_i);
                    let end_byte = byte_i + m.end();
                    let end_char = char_index_at_byte(text, end_byte);
                    tokens.push(Token {
                        start: start_char,
                        end_exclusive: end_char,
                        kind: rule.kind,
                    });
                    byte_i = end_byte;
                    matched = true;
                    break;
                }
            }
        }
        if !matched {
            let ch_len = text[byte_i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
            byte_i += ch_len;
        }
    }
    tokens
}

fn rule(kind: TokenKind, pattern: &str) -> SyntaxRule {
    SyntaxRule {
        kind,
        pattern: Regex::new(pattern).expect("static syntax-highlight pattern must compile"),
    }
}

fn c_family() -> SyntaxRuleSet {
    SyntaxRuleSet {
        name: "c-family",
        rules: vec![
            rule(TokenKind::Comment, r"//[^\n]*"),
            rule(TokenKind::Comment, r"(?s)/\*.*?\*/"),
            rule(TokenKind::StringLit, r#""(?:\\.|[^"\\\n])*""#),
            rule(TokenKind::StringLit, r"'(?:\\.|[^'\\\n])*'"),
        ],
    }
}

fn hash_family() -> SyntaxRuleSet {
    SyntaxRuleSet {
        name: "hash",
        rules: vec![
            rule(TokenKind::Comment, r"#[^\n]*"),
            rule(TokenKind::StringLit, r#""(?:\\.|[^"\\\n])*""#),
            rule(TokenKind::StringLit, r"'(?:\\.|[^'\\\n])*'"),
        ],
    }
}

fn xml_family() -> SyntaxRuleSet {
    SyntaxRuleSet {
        name: "xml",
        rules: vec![
            rule(TokenKind::Comment, r"(?s)<!--.*?-->"),
            rule(TokenKind::StringLit, r#""(?:\\.|[^"\\\n])*""#),
            rule(TokenKind::StringLit, r"'(?:\\.|[^'\\\n])*'"),
        ],
    }
}

fn sql_family() -> SyntaxRuleSet {
    SyntaxRuleSet {
        name: "sql-dash",
        rules: vec![
            rule(TokenKind::Comment, r"--[^\n]*"),
            rule(TokenKind::Comment, r"(?s)/\*.*?\*/"),
            rule(TokenKind::StringLit, r"'(?:''|[^'\n])*'"),
        ],
    }
}

struct Registry {
    by_extension: HashMap<&'static str, Arc<SyntaxRuleSet>>,
    by_basename: HashMap<&'static str, Arc<SyntaxRuleSet>>,
}

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| {
        let c = Arc::new(c_family());
        let hash = Arc::new(hash_family());
        let xml = Arc::new(xml_family());
        let sql = Arc::new(sql_family());

        let mut by_extension: HashMap<&'static str, Arc<SyntaxRuleSet>> = HashMap::new();
        for ext in [
            "kt", "kts", "java", "js", "mjs", "ts", "tsx", "jsx", "c", "cc", "cpp", "cxx", "h",
            "hpp", "cs", "go", "rs", "swift", "scala", "dart", "css", "scss", "less", "json",
            "json5", "groovy", "gradle",
        ] {
            by_extension.insert(ext, c.clone());
        }
        for ext in [
            "py", "pyw", "sh", "bash", "zsh", "rb", "yml", "yaml", "toml", "conf", "ini", "cfg",
            "env", "tf", "pl", "r",
        ] {
            by_extension.insert(ext, hash.clone());
        }
        for ext in ["html", "htm", "xml", "svg", "xhtml", "xsd", "xsl"] {
            by_extension.insert(ext, xml.clone());
        }
        for ext in ["sql", "lua", "hs", "ex", "exs"] {
            by_extension.insert(ext, sql.clone());
        }

        let mut by_basename: HashMap<&'static str, Arc<SyntaxRuleSet>> = HashMap::new();
        by_basename.insert("Makefile", hash.clone());
        by_basename.insert("Dockerfile", hash.clone());
        by_basename.insert(".gitignore", hash.clone());
        by_basename.insert(".gitattributes", hash.clone());

        Registry {
            by_extension,
            by_basename,
        }
    })
}

pub fn rules_for(path: Option<&Path>) -> Option<Arc<SyntaxRuleSet>> {
    let path = path?;
    let name = path.file_name()?.to_str()?;
    let reg = registry();
    if let Some(rs) = reg.by_basename.get(name) {
        return Some(rs.clone());
    }
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() {
        return None;
    }
    reg.by_extension.get(ext.to_lowercase().as_str()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn c_family_tokenizes_line_comment_and_string() {
        let rs = rules_for(Some(&PathBuf::from("foo.kt")));
        assert!(rs.is_some());
        let text = r#"val x = "hello" // greet"#;
        let tokens = highlight(text, rs.as_deref());
        let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&TokenKind::StringLit));
        assert!(kinds.contains(&TokenKind::Comment));
    }

    #[test]
    fn c_family_tokenizes_block_comment_spanning_lines() {
        let rs = rules_for(Some(&PathBuf::from("a.java"))).unwrap();
        let text = "code /* multi\nline */ more";
        let tokens = highlight(text, Some(&rs));
        let comment = tokens.iter().find(|t| t.kind == TokenKind::Comment).unwrap();
        assert_eq!(comment.start, 5);
        assert_eq!(comment.end_exclusive, text.find("*/").unwrap() + 2);
    }

    #[test]
    fn hash_family_treats_hash_as_comment() {
        let rs = rules_for(Some(&PathBuf::from("script.py"))).unwrap();
        let text = "x = 1 # comment";
        let tokens = highlight(text, Some(&rs));
        assert_eq!(tokens.last().unwrap().kind, TokenKind::Comment);
    }

    #[test]
    fn string_with_escaped_quote_is_one_token() {
        let rs = rules_for(Some(&PathBuf::from("a.js"))).unwrap();
        let text = r#"val s = "a \"b\" c""#;
        let tokens = highlight(text, Some(&rs));
        let strs: Vec<_> = tokens.iter().filter(|t| t.kind == TokenKind::StringLit).collect();
        assert_eq!(strs.len(), 1);
    }

    #[test]
    fn unknown_extension_yields_no_tokens() {
        let rs = rules_for(Some(&PathBuf::from("a.unknownext")));
        assert!(rs.is_none());
        let tokens = highlight("anything", rs.as_deref());
        assert!(tokens.is_empty());
    }

    #[test]
    fn none_path_yields_no_rule_set() {
        assert!(rules_for(None).is_none());
    }

    #[test]
    fn makefile_basename_matches_hash_family() {
        assert!(rules_for(Some(&PathBuf::from("Makefile"))).is_some());
    }

    #[test]
    fn xml_comment() {
        let rs = rules_for(Some(&PathBuf::from("a.html"))).unwrap();
        let text = "<p>x</p><!-- hi -->";
        let tokens = highlight(text, Some(&rs));
        assert_eq!(tokens.iter().filter(|t| t.kind == TokenKind::Comment).count(), 1);
    }
}
