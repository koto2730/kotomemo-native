use regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::OnceLock;

pub struct TemplateContext<'a> {
    pub selection: &'a str,
    pub filename: &'a str,
    pub tokens: &'a HashMap<String, String>,
}

fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{\s*([a-zA-Z0-9_.]+)\s*\}\}").unwrap())
}

pub fn render(template: &str, context: &TemplateContext) -> String {
    if template.is_empty() {
        return template.to_string();
    }
    placeholder_re()
        .replace_all(template, |caps: &Captures| {
            let key = &caps[1];
            resolve(key, context).unwrap_or_else(|| caps[0].to_string())
        })
        .into_owned()
}

fn resolve(key: &str, context: &TemplateContext) -> Option<String> {
    if key == "selection" {
        Some(context.selection.to_string())
    } else if key == "selectionJson" {
        Some(json_escape(context.selection))
    } else if key == "filename" {
        Some(context.filename.to_string())
    } else if let Some(name) = key.strip_prefix("tokens.") {
        context.tokens.get(name).cloned()
    } else {
        None
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        selection: &'a str,
        filename: &'a str,
        tokens: &'a HashMap<String, String>,
    ) -> TemplateContext<'a> {
        TemplateContext {
            selection,
            filename,
            tokens,
        }
    }

    #[test]
    fn replaces_selection_placeholder() {
        let tokens = HashMap::new();
        assert_eq!(
            render("hi {{selection}}", &ctx("world", "", &tokens)),
            "hi world"
        );
    }

    #[test]
    fn json_escapes_selection_json() {
        let tokens = HashMap::new();
        let out = render(r#"{"q":"{{selectionJson}}"}"#, &ctx("a\"b\nc", "", &tokens));
        assert_eq!(out, r#"{"q":"a\"b\nc"}"#);
    }

    #[test]
    fn looks_up_token_by_name() {
        let mut tokens = HashMap::new();
        tokens.insert("openai".to_string(), "sk-xxx".to_string());
        let out = render("Bearer {{tokens.openai}}", &ctx("", "", &tokens));
        assert_eq!(out, "Bearer sk-xxx");
    }

    #[test]
    fn unknown_placeholder_is_left_intact() {
        let tokens = HashMap::new();
        assert_eq!(render("hi {{unknown}}", &ctx("", "", &tokens)), "hi {{unknown}}");
    }

    #[test]
    fn filename_placeholder_works() {
        let tokens = HashMap::new();
        assert_eq!(
            render("file is {{filename}}", &ctx("", "foo.txt", &tokens)),
            "file is foo.txt"
        );
    }

    #[test]
    fn tolerates_whitespace_inside_braces() {
        let tokens = HashMap::new();
        assert_eq!(render("={{ selection }}=", &ctx("hi", "", &tokens)), "=hi=");
    }
}
