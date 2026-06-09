//! Formatação de data e hora em português (Brasil). Funções puras.

use chrono::{Datelike, Timelike, Weekday};

/// Formata a hora como `HH:MM:SS` (24h, com zero à esquerda).
pub fn format_time<T: Timelike>(t: &T) -> String {
    format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
}

/// Formata a data por extenso em pt-BR: "terça-feira, 09 de junho de 2026".
pub fn format_date<T: Datelike>(d: &T) -> String {
    format!(
        "{}, {:02} de {} de {}",
        weekday_ptbr(d.weekday()),
        d.day(),
        month_ptbr(d.month()),
        d.year(),
    )
}

/// Altura (em linhas) dos glifos grandes do relógio.
pub const BIG_HEIGHT: usize = 5;

/// Renderiza uma string (dígitos e `:`) como arte ASCII de 5 linhas, para
/// exibir a hora em "fonte" grande no header.
pub fn big_glyphs(text: &str) -> [String; BIG_HEIGHT] {
    let mut rows: [String; BIG_HEIGHT] = Default::default();
    for c in text.chars() {
        let g = glyph(c);
        for (r, row) in rows.iter_mut().enumerate() {
            if !row.is_empty() {
                row.push(' ');
            }
            row.push_str(g[r]);
        }
    }
    rows
}

fn glyph(c: char) -> [&'static str; BIG_HEIGHT] {
    match c {
        '0' => ["███", "█ █", "█ █", "█ █", "███"],
        '1' => [" █ ", "██ ", " █ ", " █ ", "███"],
        '2' => ["███", "  █", "███", "█  ", "███"],
        '3' => ["███", "  █", "███", "  █", "███"],
        '4' => ["█ █", "█ █", "███", "  █", "  █"],
        '5' => ["███", "█  ", "███", "  █", "███"],
        '6' => ["███", "█  ", "███", "█ █", "███"],
        '7' => ["███", "  █", "  █", "  █", "  █"],
        '8' => ["███", "█ █", "███", "█ █", "███"],
        '9' => ["███", "█ █", "███", "  █", "███"],
        ':' => [" ", "█", " ", "█", " "],
        _ => ["   ", "   ", "   ", "   ", "   "],
    }
}

fn weekday_ptbr(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "segunda-feira",
        Weekday::Tue => "terça-feira",
        Weekday::Wed => "quarta-feira",
        Weekday::Thu => "quinta-feira",
        Weekday::Fri => "sexta-feira",
        Weekday::Sat => "sábado",
        Weekday::Sun => "domingo",
    }
}

fn month_ptbr(m: u32) -> &'static str {
    match m {
        1 => "janeiro",
        2 => "fevereiro",
        3 => "março",
        4 => "abril",
        5 => "maio",
        6 => "junho",
        7 => "julho",
        8 => "agosto",
        9 => "setembro",
        10 => "outubro",
        11 => "novembro",
        12 => "dezembro",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime};

    #[test]
    fn time_is_zero_padded_24h() {
        let t = NaiveTime::from_hms_opt(9, 5, 7).unwrap();
        assert_eq!(format_time(&t), "09:05:07");
        let t = NaiveTime::from_hms_opt(23, 59, 0).unwrap();
        assert_eq!(format_time(&t), "23:59:00");
    }

    #[test]
    fn date_in_full_ptbr() {
        // 2026-06-09 é uma terça-feira.
        let d = NaiveDate::from_ymd_opt(2026, 6, 9).unwrap();
        assert_eq!(format_date(&d), "terça-feira, 09 de junho de 2026");
    }

    #[test]
    fn big_glyphs_have_fixed_height_and_render_colon() {
        let big = big_glyphs("12:30");
        assert_eq!(big.len(), 5);
        // a coluna do ':' tem bloco nas linhas 1 e 3 (índices 1 e 3).
        assert!(big.iter().any(|r| r.contains('█')));
        // todas as 5 linhas têm a mesma largura (monospace).
        let w = big[0].chars().count();
        assert!(big.iter().all(|r| r.chars().count() == w));
    }

    #[test]
    fn weekday_and_month_names_are_translated() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(); // quinta
        assert_eq!(format_date(&d), "quinta-feira, 01 de janeiro de 2026");
        let d = NaiveDate::from_ymd_opt(2026, 12, 25).unwrap(); // sexta
        assert_eq!(format_date(&d), "sexta-feira, 25 de dezembro de 2026");
    }
}
