//! Busca e parsing de eventos de agenda via `gcalcli --tsv`.

use std::process::Command;

use chrono::{Duration, Local};

use super::Account;

/// Um evento de agenda já normalizado para exibição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaItem {
    /// Conta de origem.
    pub account: Account,
    /// Data de início, formato ISO ("2026-06-12").
    pub date: String,
    /// Hora de início ("14:00"); vazio para eventos de dia inteiro.
    pub time: String,
    /// Título do evento.
    pub title: String,
}

impl AgendaItem {
    /// `true` se for evento de dia inteiro (sem hora).
    pub fn all_day(&self) -> bool {
        self.time.trim().is_empty()
    }
}

/// Faz o parse da saída de `gcalcli agenda --tsv`.
///
/// Colunas: `start_date  start_time  end_date  end_time  title`.
/// A primeira linha (cabeçalho) é descartada.
pub fn parse_agenda_tsv(tsv: &str, account: Account) -> Vec<AgendaItem> {
    tsv.lines()
        .filter(|line| !line.trim().is_empty())
        .filter(|line| !line.starts_with("start_date"))
        .filter_map(|line| {
            let mut cols = line.splitn(5, '\t');
            let date = cols.next()?.trim().to_string();
            let time = cols.next()?.trim().to_string();
            let _end_date = cols.next()?;
            let _end_time = cols.next()?;
            let title = cols.next().unwrap_or("").trim().to_string();
            if date.is_empty() {
                return None;
            }
            Some(AgendaItem {
                account,
                date,
                time,
                title,
            })
        })
        .collect()
}

/// Ordena uma lista de eventos por (data, hora). Dia inteiro vem antes dos
/// eventos com horário no mesmo dia.
pub fn sort_chronologically(items: &mut [AgendaItem]) {
    items.sort_by(|a, b| a.date.cmp(&b.date).then(a.time.cmp(&b.time)));
}

/// Busca a agenda dos próximos 7 dias de uma conta rodando o `gcalcli` com o
/// `XDG_DATA_HOME` isolado por conta.
pub fn fetch(account: Account) -> Result<Vec<AgendaItem>, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME não definido".to_string())?;
    let data_home = format!("{home}/.local/share/gcalcli-accounts/{}", account.gcalcli_dir());

    let today = Local::now().date_naive();
    let end = today + Duration::days(7);
    let start_arg = today.format("%Y-%m-%d").to_string();
    let end_arg = end.format("%Y-%m-%d").to_string();

    // `--calendar` é opção global (antes do subcomando) e restringe à calendar
    // primária da conta — exclui salas e calendars de colegas assinadas.
    let calendar = account.primary_calendar();
    let output = Command::new("gcalcli")
        .env("XDG_DATA_HOME", &data_home)
        .args([
            "--calendar",
            &calendar,
            "agenda",
            &start_arg,
            &end_arg,
            "--tsv",
        ])
        .output()
        .map_err(|e| format!("falha ao executar gcalcli: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gcalcli falhou: {}", err.lines().last().unwrap_or("")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_agenda_tsv(&stdout, account))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cabeçalho + dia inteiro: amostra real de `gcalcli agenda --tsv`.
    const REAL_ALLDAY: &str =
        "start_date\tstart_time\tend_date\tend_time\ttitle\n2026-06-12\t\t2026-06-13\t\tDia dos Namorados\n";

    #[test]
    fn skips_header_and_parses_all_day_event() {
        let items = parse_agenda_tsv(REAL_ALLDAY, Account::Personal);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].date, "2026-06-12");
        assert_eq!(items[0].title, "Dia dos Namorados");
        assert!(items[0].all_day());
        assert_eq!(items[0].account, Account::Personal);
    }

    #[test]
    fn parses_timed_event() {
        let tsv = "start_date\tstart_time\tend_date\tend_time\ttitle\n2026-06-10\t14:00\t2026-06-10\t15:00\tDaily\n";
        let items = parse_agenda_tsv(tsv, Account::Work);
        assert_eq!(items[0].time, "14:00");
        assert!(!items[0].all_day());
        assert_eq!(items[0].title, "Daily");
    }

    #[test]
    fn sorts_by_date_then_time() {
        let mut items = vec![
            AgendaItem { account: Account::Work, date: "2026-06-10".into(), time: "14:00".into(), title: "tarde".into() },
            AgendaItem { account: Account::Work, date: "2026-06-10".into(), time: "".into(), title: "dia inteiro".into() },
            AgendaItem { account: Account::Work, date: "2026-06-09".into(), time: "09:00".into(), title: "ontem cedo".into() },
        ];
        sort_chronologically(&mut items);
        assert_eq!(items[0].title, "ontem cedo");
        assert_eq!(items[1].title, "dia inteiro");
        assert_eq!(items[2].title, "tarde");
    }

    #[test]
    fn empty_input_yields_no_items() {
        assert_eq!(parse_agenda_tsv("", Account::Work).len(), 0);
        assert_eq!(parse_agenda_tsv("start_date\tstart_time\tend_date\tend_time\ttitle\n", Account::Work).len(), 0);
    }
}
