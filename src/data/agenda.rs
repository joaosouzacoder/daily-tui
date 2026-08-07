//! Busca e parsing de eventos de agenda via `gcalcli --tsv`.

use std::path::PathBuf;
use std::process::Command;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime};

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

/// Junta data e hora do item num instante local.
///
/// Devolve `None` para evento de dia inteiro (sem hora) e para qualquer coisa
/// que não parseie: as duas colunas vêm como texto da saída do `gcalcli`, e um
/// campo torto não pode derrubar o header.
fn starts_at(item: &AgendaItem) -> Option<DateTime<Local>> {
    let date = NaiveDate::parse_from_str(item.date.trim(), "%Y-%m-%d").ok()?;
    let time = NaiveTime::parse_from_str(item.time.trim(), "%H:%M").ok()?;
    date.and_time(time).and_local_timezone(Local).single()
}

/// O próximo evento que ainda não começou.
///
/// Ignora os de dia inteiro: eles não colidem com um bloco de foco, e não teriam
/// contagem regressiva. Não assume que a lista está ordenada — ela está, mas
/// depender disso deixaria o resultado dependendo de quem chamou `sort` antes.
pub fn next_upcoming(items: &[AgendaItem], now: DateTime<Local>) -> Option<&AgendaItem> {
    items
        .iter()
        .filter_map(|item| starts_at(item).map(|at| (at, item)))
        .filter(|(at, _)| *at > now)
        .min_by_key(|(at, _)| *at)
        .map(|(_, item)| item)
}

/// Os eventos que caem nos próximos `days` dias, contando hoje como o primeiro.
///
/// A busca traz 7 dias e continua trazendo: a linha do próximo compromisso no
/// header precisa enxergar até a segunda-feira quando você olha numa sexta à
/// noite. O painel mostra menos que isso porque cabe em menos altura — são
/// recortes diferentes do mesmo dado, não uma busca menor.
pub fn within_days(items: &[AgendaItem], now: DateTime<Local>, days: i64) -> Vec<AgendaItem> {
    let today = now.date_naive();
    let last = today + Duration::days(days - 1);
    items
        .iter()
        .filter(|item| {
            match NaiveDate::parse_from_str(item.date.trim(), "%Y-%m-%d") {
                Ok(d) => d >= today && d <= last,
                // Data que não parseia fica na lista. Este filtro existe para
                // encurtar a janela, não para virar um lugar onde evento some
                // em silêncio — quem tem data torta aparece e se explica.
                Err(_) => true,
            }
        })
        .cloned()
        .collect()
}

/// Quantos minutos faltam para o evento começar. `None` para dia inteiro ou
/// campo malformado. Existe para a tela decidir quando destacar a linha sem
/// precisar interpretar o texto que o `format_lead` devolve.
pub fn starts_in_minutes(item: &AgendaItem, now: DateTime<Local>) -> Option<i64> {
    starts_at(item).map(|at| (at - now).num_minutes())
}

/// O "quando" do próximo compromisso: `agora`, `em 12 min`, `15:00 (em 5h)`,
/// `amanhã 09:00`, `Sexta 14:00`.
pub fn format_lead(item: &AgendaItem, now: DateTime<Local>) -> String {
    let Some(at) = starts_at(item) else {
        return String::new();
    };
    let days = (at.date_naive() - now.date_naive()).num_days();
    let time = at.format("%H:%M").to_string();
    if days >= 2 {
        return format!("{} {time}", crate::clock::weekday_short_ptbr(at.weekday()));
    }
    if days == 1 {
        return format!("amanhã {time}");
    }

    let minutes = (at - now).num_minutes();
    if minutes < 1 {
        // "em 0 min" leria como "não tem nada marcado", que é o oposto.
        return "agora".to_string();
    }
    if minutes < 60 {
        return format!("em {minutes} min");
    }
    format!("{time} (em {}h)", minutes / 60)
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

/// Diretório de dados por conta do gcalcli no Unix.
///
/// Lá o `gcalcli` (via `platformdirs`) respeita `XDG_DATA_HOME`, então basta
/// isolar cada conta num subdiretório e passar essa var no comando — os tokens
/// OAuth de *work* e *personal* ficam separados naturalmente.
#[cfg(not(windows))]
fn gcalcli_data_home(account: Account) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or_else(|| "HOME não definido".to_string())?;
    Ok(PathBuf::from(home)
        .join(".local/share/gcalcli-accounts")
        .join(account.gcalcli_dir()))
}

/// Caminho fixo do token OAuth do gcalcli no Windows —
/// `platformdirs.user_data_path("gcalcli")` = `%LOCALAPPDATA%\gcalcli\gcalcli`.
#[cfg(windows)]
fn gcalcli_canonical_token() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "LOCALAPPDATA não definido".to_string())?;
    Ok(PathBuf::from(base).join("gcalcli").join("gcalcli").join("oauth"))
}

/// Token OAuth guardado por conta: `%LOCALAPPDATA%\gcalcli-accounts\<conta>\oauth`.
#[cfg(windows)]
fn gcalcli_account_token(account: Account) -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "LOCALAPPDATA não definido".to_string())?;
    Ok(PathBuf::from(base)
        .join("gcalcli-accounts")
        .join(account.gcalcli_dir())
        .join("oauth"))
}

/// Ativa a conta no Windows copiando o token dela para o caminho fixo do gcalcli.
///
/// No Windows o `platformdirs` ignora env vars (usa a API do sistema) e o
/// gcalcli sempre lê/grava o token no mesmo lugar — não dá para isolar por
/// diretório como no Unix. Mantemos um token por conta e trocamos o ativo antes
/// de cada consulta. O worker chama as contas em série, então não há corrida.
#[cfg(windows)]
fn activate_account(account: Account) -> Result<(), String> {
    let src = gcalcli_account_token(account)?;
    if !src.exists() {
        return Err(format!(
            "conta '{}' sem token — rode scripts/google-auth.ps1",
            account.gcalcli_dir()
        ));
    }
    let dst = gcalcli_canonical_token()?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("falha ao criar {}: {e}", parent.display()))?;
    }
    std::fs::copy(&src, &dst).map_err(|e| format!("falha ao ativar conta gcalcli: {e}"))?;
    Ok(())
}

/// Busca a agenda dos próximos 7 dias de uma conta rodando o `gcalcli`.
pub fn fetch(account: Account) -> Result<Vec<AgendaItem>, String> {
    // Windows: seleciona a conta trocando o token OAuth ativo (o platformdirs
    // ignora env vars). Unix: isola por `XDG_DATA_HOME` no comando abaixo.
    #[cfg(windows)]
    activate_account(account)?;

    let today = Local::now().date_naive();
    let end = today + Duration::days(7);
    let start_arg = today.format("%Y-%m-%d").to_string();
    let end_arg = end.format("%Y-%m-%d").to_string();

    // `--calendar` é opção global (antes do subcomando) e restringe à calendar
    // primária da conta — exclui salas e calendars de colegas assinadas.
    let calendar = account.primary_calendar();
    let mut cmd = Command::new("gcalcli");
    super::force_utf8_stdout(&mut cmd);
    #[cfg(not(windows))]
    {
        let data_home = gcalcli_data_home(account)?;
        cmd.env("XDG_DATA_HOME", &data_home);
    }
    let output = cmd
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
        return Err(format!("gcalcli falhou: {}", super::stderr_summary(&err)));
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

    // --- próximo compromisso ---

    /// Quarta-feira, 2026-06-10, 10:00. Fixo para "amanhã" e "quinta" não
    /// dependerem de que dia é hoje.
    fn now() -> chrono::DateTime<Local> {
        use chrono::TimeZone;
        Local.with_ymd_and_hms(2026, 6, 10, 10, 0, 0).unwrap()
    }

    fn at(date: &str, time: &str, title: &str) -> AgendaItem {
        AgendaItem {
            account: Account::Work,
            date: date.into(),
            time: time.into(),
            title: title.into(),
        }
    }

    #[test]
    fn the_next_one_is_the_first_that_has_not_started() {
        let items = vec![
            at("2026-06-10", "09:00", "já passou"),
            at("2026-06-10", "11:00", "essa"),
            at("2026-06-10", "15:00", "depois"),
        ];
        assert_eq!(next_upcoming(&items, now()).unwrap().title, "essa");
    }

    #[test]
    fn an_all_day_event_is_not_the_next_appointment() {
        // Sem hora, ele não colide com um bloco de foco — e não teria contagem.
        //
        // O dia inteiro é o de AMANHÃ de propósito. Com um de hoje, tratar a
        // hora vazia como meia-noite ainda cairia no passado e o filtro de
        // "já começou" esconderia o defeito: o teste passaria sem a exclusão.
        // Amanhã à meia-noite é futuro, e só a exclusão o mantém fora.
        let items = vec![
            at("2026-06-11", "", "feriado"),
            at("2026-06-11", "09:00", "essa"),
        ];
        assert_eq!(next_upcoming(&items, now()).unwrap().title, "essa");
    }

    #[test]
    fn nothing_ahead_means_nothing_to_show() {
        assert!(next_upcoming(&[], now()).is_none());
        let past = vec![at("2026-06-10", "09:00", "já passou"), at("2026-06-09", "23:00", "ontem")];
        assert!(next_upcoming(&past, now()).is_none());
    }

    #[test]
    fn a_malformed_date_or_time_is_skipped_instead_of_panicking() {
        // As duas colunas vêm como texto de um CLI externo.
        let items = vec![
            at("nao-e-data", "11:00", "lixo"),
            at("2026-06-10", "25:99", "hora impossível"),
            at("2026-06-10", "11:00", "essa"),
        ];
        assert_eq!(next_upcoming(&items, now()).unwrap().title, "essa");
    }

    #[test]
    fn the_lead_says_how_far_away_the_appointment_is() {
        assert_eq!(format_lead(&at("2026-06-10", "10:00", ""), now()), "agora");
        assert_eq!(format_lead(&at("2026-06-10", "10:12", ""), now()), "em 12 min");
        assert_eq!(format_lead(&at("2026-06-10", "15:00", ""), now()), "15:00 (em 5h)");
        assert_eq!(format_lead(&at("2026-06-11", "09:00", ""), now()), "amanhã 09:00");
        assert_eq!(format_lead(&at("2026-06-11", "23:30", ""), now()), "amanhã 23:30");
        // 2026-06-11 é quinta; 12 é sexta.
        assert_eq!(format_lead(&at("2026-06-12", "14:00", ""), now()), "Sexta 14:00");
    }

    #[test]
    fn the_panel_window_keeps_today_and_tomorrow_and_drops_the_rest() {
        let items = vec![
            at("2026-06-09", "09:00", "ontem"),
            at("2026-06-10", "11:00", "hoje"),
            at("2026-06-11", "09:00", "amanhã"),
            at("2026-06-12", "09:00", "depois de amanhã"),
        ];
        let window = within_days(&items, now(), 2);
        let kept: Vec<&str> = window.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(kept, vec!["hoje", "amanhã"]);
    }

    #[test]
    fn the_panel_window_does_not_hide_an_event_with_a_broken_date() {
        // Encurtar a janela não pode virar um lugar onde evento some calado.
        let items = vec![at("nao-e-data", "09:00", "torto")];
        assert_eq!(within_days(&items, now(), 2).len(), 1);
    }

    #[test]
    fn an_appointment_less_than_a_minute_away_reads_as_now() {
        // "em 0 min" seria pior que inútil: parece que não há nada marcado.
        let items = vec![at("2026-06-10", "10:00", "começando")];
        let next = next_upcoming(&items, now() - chrono::Duration::seconds(30)).unwrap();
        assert_eq!(format_lead(next, now() - chrono::Duration::seconds(30)), "agora");
    }
}
