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
    fn weekday_and_month_names_are_translated() {
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(); // quinta
        assert_eq!(format_date(&d), "quinta-feira, 01 de janeiro de 2026");
        let d = NaiveDate::from_ymd_opt(2026, 12, 25).unwrap(); // sexta
        assert_eq!(format_date(&d), "sexta-feira, 25 de dezembro de 2026");
    }
}
