/// Converts a flat char offset into 1-based (line, column). Char-based (not
/// byte-based) to match egui's `CCursor` model directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub line: usize,
    pub column: usize,
}

impl CursorPosition {
    pub fn of(text: &str, offset: usize) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let safe = offset.min(chars.len());
        let mut line = 1usize;
        let mut last_break: i64 = -1;
        for (i, &c) in chars.iter().enumerate().take(safe) {
            if c == '\n' {
                line += 1;
                last_break = i as i64;
            }
        }
        let column = (safe as i64 - last_break) as usize;
        Self { line, column }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_of_empty_text_is_line_1_col_1() {
        assert_eq!(CursorPosition::of("", 0), CursorPosition { line: 1, column: 1 });
    }

    #[test]
    fn column_counts_from_previous_line_break() {
        assert_eq!(
            CursorPosition::of("ab\ncd", 5),
            CursorPosition { line: 2, column: 3 }
        );
    }

    #[test]
    fn multiple_lines_tracked() {
        assert_eq!(
            CursorPosition::of("a\nb\nc", 4),
            CursorPosition { line: 3, column: 1 }
        );
    }

    #[test]
    fn out_of_range_offset_is_clamped() {
        assert_eq!(CursorPosition::of("abc", 99), CursorPosition { line: 1, column: 4 });
    }
}
