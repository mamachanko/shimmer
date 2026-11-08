use std::io::Write;
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_width::UnicodeWidthChar;

use crate::terminal::Viewport;

pub const RESET: &[u8] = b"\x1b[0m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub col: usize,
    pub row: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Native { origin: Cell, overflow: usize },
    Relative { anchor_row: usize },
}

impl Placement {
    pub fn position(self, output: &mut Vec<u8>, row: usize, col: usize) {
        match self {
            Self::Native { .. } => write!(output, "\x1b[{};{}H", row + 1, col + 1).unwrap(),
            Self::Relative { anchor_row } => {
                output.extend_from_slice(b"\x1b8");
                if row != anchor_row {
                    write!(
                        output,
                        "\x1b[{}{}",
                        row.abs_diff(anchor_row),
                        if row > anchor_row { 'B' } else { 'A' }
                    )
                    .unwrap();
                }
                if col != 0 {
                    write!(output, "\x1b[{col}C").unwrap();
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Glyph {
    pub text: String,
    pub width: usize,
    pub cell: Cell,
    pub style: usize,
}

#[derive(Debug)]
pub struct Layout {
    pub glyphs: Vec<Glyph>,
    pub styles: Vec<String>,
    pub safe_prompt: String,
    pub placement: Placement,
}

fn category_is_control(category: GeneralCategory) -> bool {
    matches!(
        category,
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::Surrogate
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
    )
}

pub fn sanitize(prompt: &str) -> String {
    let mut result = String::with_capacity(prompt.len());
    let mut chars = prompt.chars().peekable();
    while let Some(ch) = chars.next() {
        let command = if ch == '\x1b' {
            chars.peek().copied()
        } else {
            match ch {
                '\u{9b}' => Some('['),
                '\u{9d}' => Some(']'),
                '\u{90}' => Some('P'),
                '\u{98}' => Some('X'),
                '\u{9e}' => Some('^'),
                '\u{9f}' => Some('_'),
                _ => None,
            }
        };
        match command {
            Some('[') => {
                if ch == '\x1b' {
                    chars.next();
                }
                let mut parameters = String::new();
                while let Some(&parameter) = chars.peek() {
                    if !('0'..='?').contains(&parameter) {
                        break;
                    }
                    parameters.push(parameter);
                    chars.next();
                }
                let mut intermediate = false;
                while chars.peek().is_some_and(|c| (' '..='/').contains(c)) {
                    intermediate = true;
                    chars.next();
                }
                if chars.peek().is_some_and(|c| ('@'..='~').contains(c)) {
                    let final_byte = chars.next().unwrap();
                    if final_byte == 'm'
                        && !intermediate
                        && parameters
                            .chars()
                            .all(|c| c.is_ascii_digit() || c == ';' || c == ':')
                    {
                        result.push_str("\x1b[");
                        result.push_str(&parameters);
                        result.push('m');
                    }
                }
            }
            Some(kind @ (']' | 'P' | 'X' | '^' | '_')) => {
                if ch == '\x1b' {
                    chars.next();
                }
                while let Some(content) = chars.next() {
                    if content == '\u{9c}' || (kind == ']' && content == '\x07') {
                        break;
                    }
                    if content == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ if ch == '\x1b' => {
                while chars.peek().is_some_and(|c| (' '..='/').contains(c)) {
                    chars.next();
                }
                if chars.peek().is_some_and(|c| ('0'..='~').contains(c)) {
                    chars.next();
                }
            }
            _ if matches!(ch, '\r' | '\n' | '\t')
                || !category_is_control(get_general_category(ch)) =>
            {
                result.push(ch);
            }
            _ => {}
        }
    }
    result
}

impl Layout {
    pub fn parse(prompt: &str, viewport: Viewport, native: Option<Cell>) -> Option<Self> {
        let safe_prompt = sanitize(prompt);
        let origin = native.unwrap_or(Cell { col: 0, row: 0 });
        let mut cell = origin;
        let mut highest = cell.row;
        let mut glyphs: Vec<Glyph> = Vec::new();
        let mut styles = vec![String::new()];
        let mut style = 0;
        let mut chars = safe_prompt.chars();
        while let Some(ch) = chars.next() {
            match ch {
                '\x1b' => {
                    chars.next();
                    let parameters: String = chars.by_ref().take_while(|&c| c != 'm').collect();
                    if parameters.is_empty() || parameters == "0" {
                        style = 0;
                    } else {
                        let next = format!("{}\x1b[{parameters}m", styles[style]);
                        if let Some(index) = styles.iter().position(|s| s == &next) {
                            style = index;
                        } else {
                            style = styles.len();
                            styles.push(next);
                        }
                    }
                }
                '\r' => cell.col = 0,
                '\n' => {
                    cell.row += 1;
                    cell.col = 0;
                    highest = highest.max(cell.row);
                }
                '\t' => {
                    cell.col = (viewport.columns - 1)
                        .min((cell.col.min(viewport.columns - 1) / 8 + 1) * 8);
                }
                _ => {
                    if matches!(
                        get_general_category(ch),
                        GeneralCategory::NonspacingMark | GeneralCategory::EnclosingMark
                    ) {
                        if let Some(glyph) = glyphs.last_mut() {
                            glyph.text.push(ch);
                        }
                        continue;
                    }
                    let width = ch.width().unwrap_or(1).max(1);
                    if width > viewport.columns {
                        return None;
                    }
                    if cell.col + width > viewport.columns {
                        cell.row += 1;
                        cell.col = 0;
                    }
                    glyphs.push(Glyph {
                        text: ch.to_string(),
                        width,
                        cell,
                        style,
                    });
                    cell.col += width;
                    highest = highest.max(cell.row);
                }
            }
        }
        let overflow = highest.saturating_sub(viewport.rows - 1);
        if glyphs.is_empty() || highest - origin.row >= viewport.rows || overflow > origin.row {
            return None;
        }
        let placement = if native.is_some() {
            for glyph in &mut glyphs {
                glyph.cell.row -= overflow;
            }
            Placement::Native {
                origin: Cell {
                    col: origin.col,
                    row: origin.row - overflow,
                },
                overflow,
            }
        } else {
            let anchor_row = viewport.rows / 2;
            for glyph in &mut glyphs {
                glyph.cell.row += anchor_row;
            }
            Placement::Relative { anchor_row }
        };
        Some(Self {
            glyphs,
            styles,
            safe_prompt,
            placement,
        })
    }

    pub fn final_frame(&self, output: &mut Vec<u8>) {
        if matches!(self.placement, Placement::Relative { .. }) {
            output.extend_from_slice(b"\x1b8\x1b[0m");
            output.extend_from_slice(self.safe_prompt.as_bytes());
        } else {
            for glyph in &self.glyphs {
                self.placement
                    .position(output, glyph.cell.row, glyph.cell.col);
                output.extend_from_slice(RESET);
                output.extend_from_slice(self.styles[glyph.style].as_bytes());
                output.extend_from_slice(glyph.text.as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_terminal_commands_and_normalizes_sgr() {
        assert_eq!(
            sanitize("\x01\x1b]0;title\x07\x1b[2J\u{9b}1;32m界e\u{301}\x1b[0m\x02\u{e000}\u{200d}"),
            "\x1b[1;32m界e\u{301}\x1b[0m"
        );
        assert_eq!(
            sanitize("a\x1bPpayload\x1b\\b\u{9d}link\u{9c}c\x1b]unterminated"),
            "abc"
        );
        assert_eq!(
            sanitize("\x1b[38:2::1:2:3mX\x1b[?25h"),
            "\x1b[38:2::1:2:3mX"
        );
    }

    #[test]
    fn lays_out_native_wrapping_styles_and_combining_marks() {
        let layout = Layout::parse(
            "\x1b[32m界e\u{301}\x1b[0m$ ",
            Viewport::new(8, 4).unwrap(),
            Some(Cell { col: 5, row: 3 }),
        )
        .unwrap();
        assert_eq!(
            layout.placement,
            Placement::Native {
                origin: Cell { col: 5, row: 2 },
                overflow: 1
            }
        );
        let cells: Vec<_> = layout
            .glyphs
            .iter()
            .map(|g| (g.text.as_str(), g.cell, g.width, g.style))
            .collect();
        assert_eq!(
            cells,
            vec![
                ("界", Cell { col: 5, row: 2 }, 2, 1),
                ("e\u{301}", Cell { col: 7, row: 2 }, 1, 1),
                ("$", Cell { col: 0, row: 3 }, 1, 0),
                (" ", Cell { col: 1, row: 3 }, 1, 0)
            ]
        );
    }

    #[test]
    fn newline_cancels_delayed_wrap_and_tabs_stop_at_margin() {
        let viewport = Viewport::new(8, 4).unwrap();
        let layout =
            Layout::parse("12345678\nA\tB", viewport, Some(Cell { col: 0, row: 0 })).unwrap();
        assert_eq!(layout.glyphs[8].cell, Cell { col: 0, row: 1 });
        assert_eq!(layout.glyphs[9].cell, Cell { col: 7, row: 1 });
    }

    #[test]
    fn relative_landing_uses_sequential_safe_prompt() {
        let layout = Layout::parse(
            "-\x1b[31mPS1\x1b[0m\n$ ",
            Viewport::new(20, 8).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(layout.placement, Placement::Relative { anchor_row: 4 });
        let mut output = Vec::new();
        layout.final_frame(&mut output);
        assert_eq!(output, b"\x1b8\x1b[0m-\x1b[31mPS1\x1b[0m\n$ ");
    }

    #[test]
    fn rejects_empty_or_taller_than_viewport() {
        let viewport = Viewport::new(4, 2).unwrap();
        for text in ["", "\x1b[31m", "a\nb\nc", "123456789"] {
            assert!(Layout::parse(text, viewport, None).is_none());
        }
    }
}
