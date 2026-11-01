use super::char_index_at_byte;
use regex::{Captures, Regex, RegexBuilder};

pub struct FindMatchesInput<'a> {
    pub text: &'a str,
    pub query: &'a str,
    pub regex: bool,
    pub case_sensitive: bool,
}

/// Compiles the query the same way Find and Replace-All both do: a literal
/// query is escaped so it goes through the exact same match engine as
/// regex mode. MULTILINE (`^`/`$` anchor per line, the VS Code / Notepad++ /
/// CodeMirror convention) is only enabled in regex mode; DOTALL stays off
/// in both modes so `.` never crosses a line boundary.
pub fn compile(input: &FindMatchesInput) -> Option<Regex> {
    let raw = if input.regex {
        input.query.to_string()
    } else {
        regex::escape(input.query)
    };
    RegexBuilder::new(&raw)
        .case_insensitive(!input.case_sensitive)
        .multi_line(input.regex)
        .build()
        .ok()
}

/// Returns inclusive (start, end) char-index ranges, matching Kotlin's
/// `IntRange` representation used by the original `FindMatchesCommand`.
pub fn find_matches(input: &FindMatchesInput) -> Vec<(usize, usize)> {
    if input.query.is_empty() {
        return Vec::new();
    }
    let Some(re) = compile(input) else {
        return Vec::new();
    };
    re.find_iter(input.text)
        .map(|m| {
            let start = char_index_at_byte(input.text, m.start());
            let end_exclusive = char_index_at_byte(input.text, m.end());
            (start, end_exclusive.saturating_sub(1))
        })
        .collect()
}

pub struct ReplaceAllInput<'a> {
    pub text: &'a str,
    pub query: &'a str,
    pub replacement: &'a str,
    pub regex: bool,
    pub case_sensitive: bool,
}

pub struct ReplaceAllResult {
    pub text: String,
    pub count: usize,
}

pub fn replace_all(input: &ReplaceAllInput) -> ReplaceAllResult {
    if input.query.is_empty() {
        return ReplaceAllResult {
            text: input.text.to_string(),
            count: 0,
        };
    }
    let find_input = FindMatchesInput {
        text: input.text,
        query: input.query,
        regex: input.regex,
        case_sensitive: input.case_sensitive,
    };
    let Some(re) = compile(&find_input) else {
        return ReplaceAllResult {
            text: input.text.to_string(),
            count: 0,
        };
    };

    let mut count = 0usize;
    let is_regex = input.regex;
    let replacement = input.replacement;
    let replaced = re.replace_all(input.text, |caps: &Captures| {
        count += 1;
        if is_regex {
            expand_backreferences(replacement, caps)
        } else {
            replacement.to_string()
        }
    });
    ReplaceAllResult {
        text: replaced.into_owned(),
        count,
    }
}

/// Custom backreference syntax (`\` escapes the next char, `$N` substitutes
/// capture group N) — deliberately not the `regex` crate's own replacement
/// template syntax, to match the original Kotlin `expandBackreferences`
/// exactly.
fn expand_backreferences(template: &str, caps: &Captures) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
        } else if c == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            let mut end = i + 1;
            while end < chars.len() && chars[end].is_ascii_digit() {
                end += 1;
            }
            let idx: usize = chars[i + 1..end].iter().collect::<String>().parse().unwrap_or(0);
            if let Some(m) = caps.get(idx) {
                out.push_str(m.as_str());
            }
            i = end;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_find_returns_all_match_ranges() {
        let r = find_matches(&FindMatchesInput {
            text: "abcabc",
            query: "ab",
            regex: false,
            case_sensitive: true,
        });
        assert_eq!(r, vec![(0, 1), (3, 4)]);
    }

    #[test]
    fn case_insensitive_literal_find() {
        let r = find_matches(&FindMatchesInput {
            text: "AbcABC",
            query: "abc",
            regex: false,
            case_sensitive: false,
        });
        assert_eq!(r, vec![(0, 2), (3, 5)]);
    }

    #[test]
    fn regex_find_returns_matched_ranges() {
        let r = find_matches(&FindMatchesInput {
            text: "a1 b22 c333",
            query: r"\d+",
            regex: true,
            case_sensitive: true,
        });
        assert_eq!(r, vec![(1, 1), (4, 5), (8, 10)]);
    }

    #[test]
    fn invalid_regex_returns_empty() {
        let r = find_matches(&FindMatchesInput {
            text: "abc",
            query: "[unterminated",
            regex: true,
            case_sensitive: true,
        });
        assert!(r.is_empty());
    }

    #[test]
    fn replace_all_literal_counts_replacements() {
        let r = replace_all(&ReplaceAllInput {
            text: "foo foo foo",
            query: "foo",
            replacement: "bar",
            regex: false,
            case_sensitive: true,
        });
        assert_eq!(r.text, "bar bar bar");
        assert_eq!(r.count, 3);
    }

    #[test]
    fn replace_all_regex_with_backreference() {
        let r = replace_all(&ReplaceAllInput {
            text: "name=John age=30",
            query: r"(\w+)=(\w+)",
            replacement: "$2:$1",
            regex: true,
            case_sensitive: true,
        });
        assert_eq!(r.text, "John:name 30:age");
        assert_eq!(r.count, 2);
    }

    #[test]
    fn non_regex_replacement_treats_dollar_signs_literally() {
        let r = replace_all(&ReplaceAllInput {
            text: "xxx",
            query: "x",
            replacement: "$1",
            regex: false,
            case_sensitive: true,
        });
        assert_eq!(r.text, "$1$1$1");
    }

    #[test]
    fn regex_caret_anchor_matches_every_line_not_just_the_first() {
        let r = replace_all(&ReplaceAllInput {
            text: "foo a\nfoo b\nfoo c",
            query: "^foo",
            replacement: "bar",
            regex: true,
            case_sensitive: true,
        });
        assert_eq!(r.text, "bar a\nbar b\nbar c");
        assert_eq!(r.count, 3);
    }

    #[test]
    fn regex_dollar_anchor_matches_every_line_end() {
        let r = replace_all(&ReplaceAllInput {
            text: "a;\nb;\nc;",
            query: ";$",
            replacement: "",
            regex: true,
            case_sensitive: true,
        });
        assert_eq!(r.text, "a\nb\nc");
        assert_eq!(r.count, 3);
    }

    #[test]
    fn regex_find_with_caret_anchor_returns_a_range_per_line() {
        let r = find_matches(&FindMatchesInput {
            text: "x1\nx2\nx3",
            query: "^x",
            regex: true,
            case_sensitive: true,
        });
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn regex_dot_does_not_cross_line_boundaries() {
        let r = replace_all(&ReplaceAllInput {
            text: "ab\ncd",
            query: "a.+",
            replacement: "X",
            regex: true,
            case_sensitive: true,
        });
        assert_eq!(r.text, "X\ncd");
        assert_eq!(r.count, 1);
    }

    #[test]
    fn literal_multiline_replace_all_hits_every_line() {
        let r = replace_all(&ReplaceAllInput {
            text: "foo\nfoo\nfoo",
            query: "foo",
            replacement: "bar",
            regex: false,
            case_sensitive: true,
        });
        assert_eq!(r.text, "bar\nbar\nbar");
        assert_eq!(r.count, 3);
    }
}
