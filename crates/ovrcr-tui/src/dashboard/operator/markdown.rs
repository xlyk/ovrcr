//! Assistant replies are GitHub-flavored Markdown. `tui-markdown` owns parsing
//! and rendering into Ratatui text; this adapter only fits that text to the
//! feed: it applies the active palette, escapes terminal controls, wraps prose
//! at word boundaries, and keeps table borders intact.
use super::display_text;
use crate::theme::{ThemeTokens, active};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tui_markdown::{AlertKind, Options, StyleSheet, from_str_with_options};
use unicode_width::UnicodeWidthStr;

/// Styles for Markdown constructs using the Dashboard's active palette.
#[derive(Clone, Copy, Debug)]
struct FeedStyleSheet {
    tokens: ThemeTokens,
}

impl StyleSheet for FeedStyleSheet {
    fn heading(&self, level: u8) -> Style {
        let color = match level {
            1 => self.tokens.mauve,
            2 => self.tokens.blue,
            3 => self.tokens.teal,
            _ => self.tokens.subtext,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    fn code(&self) -> Style {
        Style::default().fg(self.tokens.teal)
    }

    fn link(&self) -> Style {
        Style::default()
            .fg(self.tokens.blue)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::default()
            .fg(self.tokens.subtext)
            .add_modifier(Modifier::ITALIC)
    }

    fn heading_meta(&self) -> Style {
        Style::default().fg(self.tokens.muted)
    }

    fn metadata_block(&self) -> Style {
        Style::default().fg(self.tokens.muted)
    }

    fn heading_marker(&self, _level: u8) -> &str {
        ""
    }

    fn code_block_fence(&self) -> &str {
        ""
    }

    fn html(&self) -> Style {
        Style::default().fg(self.tokens.muted)
    }

    fn math_inline(&self) -> Style {
        Style::default()
            .fg(self.tokens.mauve)
            .add_modifier(Modifier::ITALIC)
    }

    fn math_display(&self) -> Style {
        Style::default().fg(self.tokens.mauve)
    }

    fn footnote_ref(&self) -> Style {
        Style::default()
            .fg(self.tokens.muted)
            .add_modifier(Modifier::ITALIC)
    }

    fn footnote_def(&self) -> Style {
        Style::default().fg(self.tokens.subtext)
    }

    fn definition_term(&self) -> Style {
        Style::default()
            .fg(self.tokens.text)
            .add_modifier(Modifier::BOLD)
    }

    fn definition_description(&self) -> Style {
        Style::default().fg(self.tokens.subtext)
    }

    fn alert(&self, kind: AlertKind) -> Style {
        let color = match kind {
            AlertKind::Note => self.tokens.blue,
            AlertKind::Tip => self.tokens.green,
            AlertKind::Important => self.tokens.mauve,
            AlertKind::Warning => self.tokens.yellow,
            AlertKind::Caution => self.tokens.red,
        };
        Style::default().fg(color)
    }

    fn table_header(&self) -> Style {
        Style::default()
            .fg(self.tokens.blue)
            .add_modifier(Modifier::BOLD)
    }

    fn table_cell(&self) -> Style {
        Style::default().fg(self.tokens.text)
    }

    fn table_border(&self) -> Style {
        Style::default().fg(self.tokens.surface2)
    }

    fn list_marker(&self) -> Style {
        Style::default().fg(self.tokens.subtext)
    }

    fn image_alt(&self) -> Style {
        Style::default()
            .fg(self.tokens.muted)
            .add_modifier(Modifier::ITALIC)
    }
}

/// Render one assistant reply as feed rows no wider than `width` cells.
///
/// Rows borrow nothing and carry their final styles. The caller adds the feed
/// gutter and durable anchors.
pub(super) fn lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let tokens = active();
    let table_width = u16::try_from(width).unwrap_or(u16::MAX);
    let options = Options::new(FeedStyleSheet { tokens }).table_width(table_width);
    let rendered = from_str_with_options(text, &options);
    let base = Style::default().fg(tokens.text);
    let mut lines = Vec::with_capacity(rendered.lines.len());
    for line in rendered.lines {
        if is_rule(&line) {
            lines.push(Line::from(Span::styled(
                "─".repeat(width),
                Style::default().fg(tokens.surface2),
            )));
            continue;
        }
        lines.extend(fit(owned_line(line, base), width));
    }
    trim_blank_edges(&mut lines);
    lines
}

/// A standalone Markdown rule renders as `---` by default; draw it instead.
fn is_rule(line: &Line<'_>) -> bool {
    line.style == Style::default()
        && line.spans.len() == 1
        && line.spans[0].style == Style::default()
        && line.spans[0].content == "---"
}

/// Copy borrowed Markdown output and escape controls before measuring it.
fn owned_line(line: Line<'_>, base: Style) -> Line<'static> {
    let line_style = base.patch(line.style);
    let mut spans = Vec::with_capacity(line.spans.len());
    for span in line.spans {
        let style = line_style.patch(span.style);
        push(&mut spans, display_text(span.content.as_ref()), style);
    }
    Line::from(spans)
}

fn push(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(&text);
        return;
    }
    spans.push(Span::styled(text, style));
}

fn fit(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if line.width() <= width || is_table_line(&line) {
        return vec![line];
    }
    wrap(line, width)
}

/// A rendered table is already laid out for the width. Never re-wrap its
/// borders; a cell whose minimum width exceeds the pane clips at the edge.
fn is_table_line(line: &Line<'_>) -> bool {
    let text = line.to_string();
    let text = text.trim_end();
    text.chars().any(is_table_border) && matches!(text.chars().last(), Some('┐' | '┤' | '┘' | '│'))
}

fn is_table_border(ch: char) -> bool {
    matches!(
        ch,
        '┌' | '┬' | '┐' | '├' | '┼' | '┤' | '└' | '┴' | '┘' | '│' | '─'
    )
}

/// One-line plain text for the Operator sidebar's reply preview.
///
/// The feed renders through [`lines`]; the preview only needs the same source
/// markers removed. Table borders become spaces so the narrow sidebar reads as
/// prose rather than fragmented cells.
pub(super) fn preview(text: &str) -> String {
    const WIDTH: usize = 200;
    const LIMIT: usize = 240;
    let mut preview = String::new();
    for line in lines(text, WIDTH) {
        let text: String = line
            .to_string()
            .chars()
            .map(|ch| if is_table_border(ch) { ' ' } else { ch })
            .collect();
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        if !preview.is_empty() {
            preview.push(' ');
        }
        preview.push_str(&text);
        if preview.len() >= LIMIT {
            break;
        }
    }
    preview
}

/// Wrap a styled line at spaces, falling back to grapheme boundaries for a word
/// wider than the row. Styles and wide-glyph widths survive the split.
fn wrap(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<(String, Style)>> = Vec::new();
    let mut row: Vec<(String, Style)> = Vec::new();
    let mut used = 0;
    // Row cells through the last space that follows visible content.
    let mut gap: Option<(usize, usize)> = None;
    for span in &line.spans {
        for grapheme in span.styled_graphemes(line.style) {
            let cells = grapheme.symbol.width();
            if used + cells > width && !row.is_empty() {
                let (at, kept) = gap.take().unwrap_or((row.len(), used));
                let rest = row.split_off(at);
                rows.push(std::mem::replace(&mut row, rest));
                used = used.saturating_sub(kept);
            }
            row.push((grapheme.symbol.to_owned(), grapheme.style));
            used = used.saturating_add(cells);
            if grapheme.symbol == " "
                && row
                    .iter()
                    .any(|(symbol, _)| !symbol.chars().all(char::is_whitespace))
            {
                gap = Some((row.len(), used));
            }
        }
    }
    rows.push(row);
    rows.into_iter()
        .map(|row| Line::from(recombine(row)))
        .collect()
}

fn recombine(row: Vec<(String, Style)>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (symbol, style) in row {
        if let Some(last) = spans.last_mut()
            && last.style == style
        {
            last.content.to_mut().push_str(&symbol);
        } else {
            spans.push(Span::styled(symbol, style));
        }
    }
    spans
}

fn trim_blank_edges(lines: &mut Vec<Line<'static>>) {
    let Some(first) = lines.iter().position(|line| !line.spans.is_empty()) else {
        lines.clear();
        return;
    };
    let last = lines
        .iter()
        .rposition(|line| !line.spans.is_empty())
        .unwrap_or(first);
    lines.truncate(last + 1);
    lines.drain(..first);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{BLUE, TEAL, TEXT};
    use ratatui::style::Modifier;

    fn render(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| line.to_string().trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn style_of(lines: &[Line<'static>], needle: &str) -> Style {
        lines
            .iter()
            .flat_map(|line| &line.spans)
            .find(|span| span.content.contains(needle))
            .unwrap_or_else(|| panic!("no span contains {needle:?}"))
            .style
    }

    #[test]
    fn inline_markdown_drops_source_markers_and_keeps_theme_styles() {
        let lines = lines("**bold** *italic* `code` [site](https://example.com)", 80);
        assert_eq!(
            render(&lines),
            "bold italic code site (https://example.com)"
        );
        assert!(
            style_of(&lines, "bold")
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            style_of(&lines, "italic")
                .add_modifier
                .contains(Modifier::ITALIC)
        );
        assert_eq!(style_of(&lines, "code").fg, Some(TEAL()));
        assert!(
            style_of(&lines, "site")
                .add_modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn headings_code_fences_and_tables_are_presented_not_echoed() {
        let source = "## Status\n\n```text\ncargo test\n```\n\n| Area | Result |\n| --- | --- |\n| tests | pass |";
        let lines = lines(source, 60);
        let rendered = render(&lines);
        assert!(!rendered.contains("##"), "{rendered}");
        assert!(!rendered.contains("```"), "{rendered}");
        assert!(!rendered.contains("| ---"), "{rendered}");
        assert!(rendered.contains("Status"), "{rendered}");
        assert!(rendered.contains("cargo test"), "{rendered}");
        assert!(rendered.contains("┌"), "{rendered}");
        assert!(rendered.contains("tests"), "{rendered}");
        assert_eq!(style_of(&lines, "Status").fg, Some(BLUE()));
        assert_eq!(style_of(&lines, "cargo test").fg, Some(TEAL()));
        assert_eq!(style_of(&lines, "Area").fg, Some(BLUE()));
        assert!(
            style_of(&lines, "Area")
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn lists_quotes_and_rules_keep_their_structure_without_emphasis_markers() {
        let lines = lines(
            "- **one**\n- *two*\n\n> **quoted**\n\n1. first\n2. second\n\n---",
            40,
        );
        let rendered = render(&lines);
        assert!(rendered.contains("- one"), "{rendered}");
        assert!(rendered.contains("- two"), "{rendered}");
        assert!(rendered.contains("> quoted"), "{rendered}");
        assert!(rendered.contains("1. first"), "{rendered}");
        assert!(rendered.contains("2. second"), "{rendered}");
        assert!(rendered.contains(&"─".repeat(40)), "{rendered}");
        assert!(!rendered.contains("**"), "{rendered}");
        assert!(!rendered.contains("---\n"), "{rendered}");
    }

    #[test]
    fn prose_wraps_at_spaces_with_styles_and_wide_glyphs_intact() {
        let lines = lines("**abcdefgh ijklmnop** qrstuv 界界界", 12);
        assert!(lines.iter().all(|line| line.width() <= 12));
        assert_eq!(render(&lines), "abcdefgh\nijklmnop\nqrstuv\n界界界");
        assert!(
            style_of(&lines, "abcdefgh")
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            style_of(&lines, "ijklmnop")
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            !style_of(&lines, "qrstuv")
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn tables_fit_the_transcript_width_without_wrapping_their_borders() {
        let source = "| Column | Value |\n| --- | --- |\n| a long cell that must wrap | second |";
        let lines = lines(source, 30);
        assert!(
            lines.iter().all(|line| line.width() <= 30),
            "{:?}",
            render(&lines)
        );
        assert!(is_table_line(&lines[0]), "{:?}", render(&lines));
        assert!(render(&lines).contains("must"), "{:?}", render(&lines));
        assert!(render(&lines).contains("wrap"), "{:?}", render(&lines));
    }

    #[test]
    fn control_characters_are_escaped_before_they_reach_the_terminal() {
        let lines = lines("plain \u{1b}]52;c;INJECTED\u{7}", 80);
        let rendered = render(&lines);
        assert!(!rendered.contains('\u{1b}'), "{rendered:?}");
        assert!(!rendered.contains('\u{7}'), "{rendered:?}");
        assert!(
            rendered.contains("\\u{1b}]52;c;INJECTED\\u{7}"),
            "{rendered:?}"
        );
    }

    #[test]
    fn preview_is_one_plain_line_without_source_markers_or_table_borders() {
        let preview = preview(
            "## Status\n\n**Ready** with `cargo test`.\n\n| Area | Result |\n| --- | --- |\n| tests | pass |",
        );
        assert!(!preview.contains("##"), "{preview}");
        assert!(!preview.contains("**"), "{preview}");
        assert!(!preview.contains('|'), "{preview}");
        assert!(!preview.contains('┌'), "{preview}");
        assert!(preview.contains("Status"), "{preview}");
        assert!(preview.contains("Ready"), "{preview}");
        assert!(preview.contains("tests"), "{preview}");
    }

    #[test]
    fn partial_markdown_and_empty_replies_never_panic() {
        assert!(lines("", 40).is_empty());
        assert!(!lines("**open", 40).is_empty());
        assert!(!lines("```rust\nopen", 40).is_empty());
        assert!(!lines("| a |\n| --- |", 40).is_empty());
    }

    #[test]
    fn plain_soft_breaks_follow_markdown_and_become_spaces() {
        assert_eq!(render(&lines("first\nsecond", 80)), "first second");
    }

    #[test]
    fn paragraphs_and_inline_markdown_use_the_theme_text_color() {
        let lines = lines("plain `code`", 80);
        assert_eq!(style_of(&lines, "plain").fg, Some(TEXT()));
        assert_eq!(style_of(&lines, "code").fg, Some(TEAL()));
    }
}
