//! Conversão de texto com escapes ANSI (SGR) em `Line`/`Span` do ratatui,
//! para preservar as cores de saídas como a do `ghpending`.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Converte uma linha com escapes ANSI em spans estilizados.
///
/// `base` é o estilo inicial (e o estilo de "fg padrão", para o código 39/0).
pub fn to_spans(input: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = base;
    let mut buf = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                let mut code = String::new();
                let mut final_byte = '\0';
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        final_byte = ch;
                        break;
                    }
                    code.push(ch);
                }
                if final_byte == 'm' {
                    if !buf.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut buf), current));
                    }
                    current = apply_sgr(current, &code, base);
                }
            }
            // Outros escapes (não-SGR) são ignorados.
        } else {
            buf.push(c);
        }
    }

    if !buf.is_empty() {
        spans.push(Span::styled(buf, current));
    }
    spans
}

/// Converte uma linha com escapes ANSI em um `Line` do ratatui.
pub fn to_line(input: &str, base: Style) -> Line<'static> {
    Line::from(to_spans(input, base))
}

fn apply_sgr(mut style: Style, code: &str, base: Style) -> Style {
    for part in code.split(';') {
        match part {
            "" | "0" => style = base, // reset
            "1" => style = style.add_modifier(Modifier::BOLD),
            "2" => style = style.add_modifier(Modifier::DIM),
            "22" => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            "39" => style = style.fg(base.fg.unwrap_or(Color::Reset)),
            _ => {
                if let Some(color) = fg_color(part) {
                    style = style.fg(color);
                }
            }
        }
    }
    style
}

fn fg_color(code: &str) -> Option<Color> {
    Some(match code {
        "30" => Color::Black,
        "31" => Color::Red,
        "32" => Color::Green,
        "33" => Color::Yellow,
        "34" => Color::Blue,
        "35" => Color::Magenta,
        "36" => Color::Cyan,
        "37" => Color::Gray,
        "90" => Color::DarkGray,
        "91" => Color::LightRed,
        "92" => Color::LightGreen,
        "93" => Color::LightYellow,
        "94" => Color::LightBlue,
        "95" => Color::LightMagenta,
        "96" => Color::LightCyan,
        "97" => Color::White,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Style {
        Style::new().fg(Color::Rgb(230, 230, 230))
    }

    #[test]
    fn plain_text_is_one_span_with_base_style() {
        let spans = to_spans("sem cor", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "sem cor");
        assert_eq!(spans[0].style.fg, Some(Color::Rgb(230, 230, 230)));
    }

    #[test]
    fn cyan_bold_repo_name() {
        // Padrão real do ghpending para nome de repo.
        let spans = to_spans("\x1b[36m\x1b[1mrepo/name\x1b[0m\x1b[39m", base());
        let colored: Vec<_> = spans.iter().filter(|s| !s.content.is_empty()).collect();
        assert_eq!(colored.len(), 1);
        assert_eq!(colored[0].content, "repo/name");
        assert_eq!(colored[0].style.fg, Some(Color::Cyan));
        assert!(colored[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn magenta_pr_marker() {
        let spans = to_spans("\x1b[35mPR \x1b[39m #12", base());
        assert_eq!(spans[0].content, "PR ");
        assert_eq!(spans[0].style.fg, Some(Color::Magenta));
        // Após o 39, volta ao fg base.
        assert_eq!(spans[1].content, " #12");
        assert_eq!(spans[1].style.fg, Some(Color::Rgb(230, 230, 230)));
    }

    #[test]
    fn dim_meta_line() {
        let spans = to_spans("  \x1b[2m(nothing pending)\x1b[0m", base());
        // Primeiro span é o texto antes do escape; o segundo é dim.
        let dim = spans.iter().find(|s| s.content == "(nothing pending)").unwrap();
        assert!(dim.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn reset_clears_modifiers_back_to_base() {
        let spans = to_spans("\x1b[1mbold\x1b[0mnormal", base());
        let normal = spans.iter().find(|s| s.content == "normal").unwrap();
        assert!(!normal.style.add_modifier.contains(Modifier::BOLD));
    }
}
