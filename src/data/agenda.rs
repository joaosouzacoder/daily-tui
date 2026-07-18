//! Busca e parsing de eventos de agenda via `gcalcli --tsv`.

use std::path::PathBuf;
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
