#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentMode {
    Indent,
    Outdent,
}

pub struct BulkIndentInput<'a> {
    pub text: &'a str,
    /// Char offsets (not bytes) — matches egui's cursor model.
    pub selection_start: usize,
    pub selection_end: usize,
    pub mode: IndentMode,
}

pub struct BulkIndentResult {
    pub text: String,
    pub selection_start: usize,
    pub selection_end: usize,
}

pub struct BulkIndentCommand {
    indent: String,
    space_width: usize,
}

impl Default for BulkIndentCommand {
    fn default() -> Self {
        Self {
            indent: "\t".to_string(),
            space_width: 4,
        }
    }
}

impl BulkIndentCommand {
    pub fn new(indent: impl Into<String>, space_width: usize) -> Self {
        Self {
            indent: indent.into(),
            space_width,
        }
    }

    pub fn execute(&self, input: BulkIndentInput) -> BulkIndentResult {
        let chars: Vec<char> = input.text.chars().collect();
        let len = chars.len();
        let a = input.selection_start.min(input.selection_end).min(len);
        let b = input.selection_start.max(input.selection_end).min(len);

        let first_line_start = if len == 0 {
            0
        } else {
            let up_to = a.saturating_sub(1).min(len - 1);
            (0..=up_to)
                .rev()
                .find(|&i| chars[i] == '\n')
                .map(|i| i + 1)
                .unwrap_or(0)
        };
        let effective_end = if b > a && b > 0 && chars.get(b - 1) == Some(&'\n') {
            b - 1
        } else {
            b
        };
        let next_lf = (effective_end.min(len)..len).find(|&i| chars[i] == '\n');
        let last_line_end = next_lf.unwrap_or(len);

        let before: String = chars[..first_line_start].iter().collect();
        let middle: String = chars[first_line_start..last_line_end].iter().collect();
        let after: String = chars[last_line_end..].iter().collect();

        let lines: Vec<&str> = middle.split('\n').collect();
        let mut delta_first: i64 = 0;
        let mut delta_total: i64 = 0;
        let new_lines: Vec<String> = lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let processed = match input.mode {
                    IndentMode::Indent => format!("{}{}", self.indent, line),
                    IndentMode::Outdent => self.outdent_line(line),
                };
                let delta = processed.chars().count() as i64 - line.chars().count() as i64;
                if i == 0 {
                    delta_first = delta;
                }
                delta_total += delta;
                processed
            })
            .collect();

        let new_text = format!("{before}{}{after}", new_lines.join("\n"));
        let new_start = ((a as i64 + delta_first).max(first_line_start as i64)) as usize;
        let new_end = ((b as i64 + delta_total).max(new_start as i64)) as usize;

        BulkIndentResult {
            text: new_text,
            selection_start: new_start,
            selection_end: new_end,
        }
    }

    fn outdent_line(&self, line: &str) -> String {
        if let Some(rest) = line.strip_prefix('\t') {
            rest.to_string()
        } else {
            let leading = line
                .chars()
                .take_while(|&c| c == ' ')
                .count()
                .min(self.space_width);
            line.chars().skip(leading).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> BulkIndentCommand {
        BulkIndentCommand::new("\t", 4)
    }

    #[test]
    fn indent_single_full_selection_adds_tab_to_each_line() {
        let text = "a\nb\nc";
        let r = cmd().execute(BulkIndentInput {
            text,
            selection_start: 0,
            selection_end: text.chars().count(),
            mode: IndentMode::Indent,
        });
        assert_eq!(r.text, "\ta\n\tb\n\tc");
    }

    #[test]
    fn indent_two_line_selection_ignores_untouched_third_line() {
        let text = "a\nb\nc";
        let r = cmd().execute(BulkIndentInput {
            text,
            selection_start: 0,
            selection_end: 3,
            mode: IndentMode::Indent,
        });
        assert_eq!(r.text, "\ta\n\tb\nc");
    }

    #[test]
    fn indent_shifts_both_selection_ends_by_inserted_chars() {
        let text = "a\nb";
        let r = cmd().execute(BulkIndentInput {
            text,
            selection_start: 0,
            selection_end: 3,
            mode: IndentMode::Indent,
        });
        assert_eq!(r.selection_start, 1);
        assert_eq!(r.selection_end, 5);
    }

    #[test]
    fn outdent_removes_leading_tab() {
        let text = "\ta\n\tb";
        let r = cmd().execute(BulkIndentInput {
            text,
            selection_start: 0,
            selection_end: text.chars().count(),
            mode: IndentMode::Outdent,
        });
        assert_eq!(r.text, "a\nb");
    }

    #[test]
    fn outdent_removes_up_to_space_width_leading_spaces() {
        let text = "    a\n  b\nc";
        let r = cmd().execute(BulkIndentInput {
            text,
            selection_start: 0,
            selection_end: text.chars().count(),
            mode: IndentMode::Outdent,
        });
        assert_eq!(r.text, "a\nb\nc");
    }

    #[test]
    fn selection_ending_right_at_newline_does_not_include_next_line() {
        let text = "a\nb\nc";
        let r = cmd().execute(BulkIndentInput {
            text,
            selection_start: 0,
            selection_end: 2,
            mode: IndentMode::Indent,
        });
        assert_eq!(r.text, "\ta\nb\nc");
    }
}
