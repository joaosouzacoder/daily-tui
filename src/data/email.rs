//! Busca e parsing de e-mails via `himalaya`.

use std::process::Command;

use serde::{Deserialize, Deserializer};

use super::Account;

/// Aceita `null` no JSON tratando-o como o valor default do tipo.
///
/// O `#[serde(default)]` só cobre campo *ausente*; o himalaya às vezes
/// emite o campo presente com valor `null` (ex.: `"subject":null`).
fn null_as_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::deserialize(de)?.unwrap_or_default())
}

/// Um e-mail (envelope) já normalizado para exibição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailItem {
    /// ID do envelope no himalaya (usado para buscar o corpo).
    pub id: String,
    /// Conta de origem.
    pub account: Account,
    /// Remetente (nome, ou endereço se o nome estiver vazio).
    pub from: String,
    /// Assunto.
    pub subject: String,
    /// `true` se ainda não foi lido (sem a flag `Seen`).
    pub unread: bool,
    /// Data crua como o himalaya devolve (ex.: "2026-06-09 13:12+00:00").
    pub date: String,
}

#[derive(Deserialize)]
struct Envelope {
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    flags: Vec<String>,
    #[serde(default, deserialize_with = "null_as_default")]
    subject: String,
    #[serde(default, deserialize_with = "null_as_default")]
    from: Addr,
    #[serde(default, deserialize_with = "null_as_default")]
    date: String,
}

#[derive(Deserialize, Default)]
struct Addr {
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, deserialize_with = "null_as_default")]
    addr: String,
}

/// Faz o parse da saída JSON de `himalaya envelope list -o json`.
pub fn parse_envelopes(json: &str, account: Account) -> Result<Vec<EmailItem>, String> {
    let envelopes: Vec<Envelope> =
        serde_json::from_str(json).map_err(|e| format!("JSON inválido: {e}"))?;

    Ok(envelopes
        .into_iter()
        .map(|env| {
            let from = if env.from.name.trim().is_empty() {
                env.from.addr
            } else {
                env.from.name
            };
            let unread = !env.flags.iter().any(|f| f.eq_ignore_ascii_case("seen"));
            EmailItem {
                id: env.id,
                account,
                from,
                subject: env.subject,
                unread,
                date: env.date,
            }
        })
        .collect())
}

/// Ordena e-mails do mais recente para o mais antigo, parseando a data
/// (com offset). Itens com data não-parseável vão para o fim.
pub fn sort_recent_first(items: &mut [EmailItem]) {
    use chrono::DateTime;
    let key = |d: &str| DateTime::parse_from_str(d, "%Y-%m-%d %H:%M%:z").ok();
    items.sort_by(|a, b| key(&b.date).cmp(&key(&a.date)));
}

/// Busca os envelopes de uma conta rodando o `himalaya`.
///
/// Lê apenas o stdout (o himalaya manda warnings de IMAP para o stderr).
pub fn fetch(account: Account, limit: u32) -> Result<Vec<EmailItem>, String> {
    let output = Command::new("himalaya")
        .args([
            "envelope",
            "list",
            "-a",
            account.himalaya_name(),
            "--page-size",
            &limit.to_string(),
            "-o",
            "json",
        ])
        .output()
        .map_err(|e| format!("falha ao executar himalaya: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("himalaya falhou: {}", super::stderr_summary(&err)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_envelopes(&stdout, account)
}

/// Busca o corpo (texto) de um e-mail específico.
pub fn fetch_body(account: Account, id: &str) -> Result<String, String> {
    let output = Command::new("himalaya")
        .args(["message", "read", id, "-a", account.himalaya_name(), "--no-headers"])
        .output()
        .map_err(|e| format!("falha ao executar himalaya: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("himalaya falhou: {}", super::stderr_summary(&err)));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Amostra real de `himalaya envelope list -a work -o json` (stdout).
    const SAMPLE: &str = r#"[{"id":"822","flags":["Seen"],"subject":"RE: Report JSM","from":{"name":"Alexander Bonfim","addr":"alexander.bonfim@nimbleevolution.com"},"to":{"name":"x","addr":"x@y.com"},"date":"2026-06-09 13:12+00:00","has_attachment":false},{"id":"821","flags":[],"subject":"Sem nome no from","from":{"name":"","addr":"raw@addr.com"},"to":{"name":"x","addr":"x@y.com"},"date":"2026-06-09 12:03+00:00","has_attachment":false}]"#;

    #[test]
    fn parses_id_subject_from_and_date() {
        let items = parse_envelopes(SAMPLE, Account::Work).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "822");
        assert_eq!(items[0].subject, "RE: Report JSM");
        assert_eq!(items[0].from, "Alexander Bonfim");
        assert_eq!(items[0].date, "2026-06-09 13:12+00:00");
        assert_eq!(items[0].account, Account::Work);
    }

    #[test]
    fn unread_is_true_when_seen_flag_absent() {
        let items = parse_envelopes(SAMPLE, Account::Work).unwrap();
        assert!(!items[0].unread, "tem flag Seen -> lido");
        assert!(items[1].unread, "sem flag Seen -> não lido");
    }

    #[test]
    fn falls_back_to_address_when_name_is_empty() {
        let items = parse_envelopes(SAMPLE, Account::Work).unwrap();
        assert_eq!(items[1].from, "raw@addr.com");
    }

    #[test]
    fn empty_array_yields_no_items() {
        assert_eq!(parse_envelopes("[]", Account::Personal).unwrap().len(), 0);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_envelopes("not json", Account::Work).is_err());
    }

    #[test]
    fn null_fields_fall_back_to_defaults() {
        let json = r#"[{"id":"99","flags":null,"subject":null,"from":null,"date":null}]"#;
        let items = parse_envelopes(json, Account::Work).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "99");
        assert_eq!(items[0].subject, "");
        assert_eq!(items[0].from, "");
        assert_eq!(items[0].date, "");
        assert!(items[0].unread, "sem flag Seen -> não lido");
    }

    #[test]
    fn sort_puts_most_recent_first_respecting_offset() {
        let mut items = vec![
            EmailItem { id: "1".into(), account: Account::Work, from: "a".into(), subject: "antigo".into(), unread: false, date: "2026-06-09 10:00+00:00".into() },
            EmailItem { id: "2".into(), account: Account::Work, from: "b".into(), subject: "novo".into(), unread: false, date: "2026-06-09 13:00+00:00".into() },
            // 12:00-03:00 == 15:00Z, ou seja, o mais recente de todos.
            EmailItem { id: "3".into(), account: Account::Personal, from: "c".into(), subject: "mais novo".into(), unread: false, date: "2026-06-09 12:00-03:00".into() },
        ];
        sort_recent_first(&mut items);
        assert_eq!(items[0].subject, "mais novo");
        assert_eq!(items[1].subject, "novo");
        assert_eq!(items[2].subject, "antigo");
    }
}
