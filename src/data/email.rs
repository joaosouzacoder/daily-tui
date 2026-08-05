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
    let page = limit.to_string();
    let stdout = run(&[
        "envelope",
        "list",
        "-a",
        account.himalaya_name(),
        "--page-size",
        &page,
        "-o",
        "json",
    ])?;
    parse_envelopes(&stdout, account)
}

/// Aliases que a config do himalaya declara; usados como fallback quando a
/// listagem de pastas da conta falha.
pub const FOLDER_ALIASES: [&str; 6] = ["inbox", "sent", "drafts", "trash", "spam", "all"];

#[derive(Deserialize)]
struct Folder {
    name: String,
}

/// Pastas de verdade da conta — no Gmail, isso inclui todas as suas etiquetas.
///
/// Os aliases da config cobrem só as seis pastas canônicas, o que deixava de
/// fora qualquer etiqueta criada por você. Aqui a lista vem do servidor.
/// Ordenada com as canônicas primeiro e o resto em ordem alfabética, para o
/// seletor não começar com uma etiqueta aleatória.
pub fn folders(account: Account) -> Result<Vec<String>, String> {
    let raw = run(&["folder", "list", "-a", account.himalaya_name(), "-o", "json"])?;
    let mut names: Vec<String> = serde_json::from_str::<Vec<Folder>>(&raw)
        .map_err(|e| format!("JSON inválido do himalaya: {e}"))?
        .into_iter()
        .map(|f| f.name)
        .collect();
    names.sort_by_key(|n| {
        let rank = FOLDER_ALIASES
            .iter()
            .position(|a| a.eq_ignore_ascii_case(n))
            .unwrap_or(FOLDER_ALIASES.len());
        (rank, n.to_lowercase())
    });
    Ok(names)
}

/// Excluir é mover para a Lixeira: recuperável, e é o que o Gmail espera.
pub const DELETE_FOLDER: &str = "trash";

/// Subcomando de `himalaya flag` para ligar ou desligar uma flag.
const fn flag_verb(seen: bool) -> &'static str {
    if seen {
        "add"
    } else {
        "remove"
    }
}

/// Marca (ou desmarca) o e-mail como lido.
pub fn set_seen(account: Account, id: &str, seen: bool) -> Result<(), String> {
    run(&[
        "flag",
        flag_verb(seen),
        id,
        "seen",
        "-a",
        account.himalaya_name(),
    ])
    .map(|_| ())
}

/// Move o e-mail para a pasta dada (nome ou alias conhecido do himalaya).
pub fn move_to(account: Account, id: &str, folder: &str) -> Result<(), String> {
    run(&[
        "message",
        "move",
        folder,
        id,
        "-a",
        account.himalaya_name(),
    ])
    .map(|_| ())
}

/// Exclui movendo para a Lixeira — não apaga do servidor.
pub fn delete(account: Account, id: &str) -> Result<(), String> {
    move_to(account, id, DELETE_FOLDER)
}

/// Roda `himalaya <args...>` e devolve o stdout (ou um erro com o stderr).
///
/// Lê apenas o stdout: o himalaya manda warnings de IMAP para o stderr, e só o
/// resumo dele interessa quando o comando falha.
fn run(args: &[&str]) -> Result<String, String> {
    let output = Command::new("himalaya")
        .args(args)
        .output()
        .map_err(|e| format!("falha ao executar himalaya: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("himalaya falhou: {}", super::stderr_summary(&err)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `Message-ID` do e-mail, sem os `<>`.
///
/// Usa `--preview` pelo mesmo motivo do corpo: abrir no Gmail não pode marcar o
/// e-mail como lido de lambuja.
pub fn message_id(account: Account, id: &str) -> Result<String, String> {
    let raw = run(&[
        "message",
        "read",
        id,
        "-a",
        account.himalaya_name(),
        "-H",
        "Message-ID",
        "--preview",
    ])?;
    parse_message_id(&raw)
}

/// Extrai o `Message-ID` da saída do himalaya.
///
/// O header vem no topo e o corpo vem logo depois, então a primeira linha que
/// casa é a que vale — e o corpo de um e-mail pode citar "Message-ID:" no meio
/// de uma resposta.
pub fn parse_message_id(raw: &str) -> Result<String, String> {
    raw.lines()
        .find_map(|line| {
            let rest = line
                .strip_prefix("Message-ID:")
                .or_else(|| line.strip_prefix("Message-Id:"))
                .or_else(|| line.strip_prefix("message-id:"))?;
            let value = rest.trim().trim_start_matches('<').trim_end_matches('>');
            (!value.is_empty()).then(|| value.to_string())
        })
        .ok_or_else(|| "e-mail sem Message-ID: não dá para achá-lo no Gmail".to_string())
}

/// Link que abre esta mensagem no Gmail.
///
/// O Gmail não expõe o id do himalaya (que é a UID do IMAP), mas acha a mensagem
/// pelo header com o operador de busca `rfc822msgid`.
///
/// A conta vai **no caminho** (`/mail/u/<e-mail>/`), não em `?authuser=`: o
/// `/mail/u/?authuser=…` não é a URL canônica, e o Gmail redireciona para
/// resolver o índice da conta — no redirect o `#search/…` se perde e a aba abre
/// na home da caixa (medido em 2026-08-05). O caminho com a conta já é canônico,
/// então o fragmento chega inteiro. Sem endereço conhecido, sobra o índice 0,
/// que é a única conta quando cada uma vive num profile do navegador.
pub fn gmail_url(address: &str, message_id: &str) -> String {
    let msgid = percent_encode(message_id);
    let account = if address.is_empty() {
        "0".to_string()
    } else {
        percent_encode(address)
    };
    format!("https://mail.google.com/mail/u/{account}/#search/rfc822msgid%3A{msgid}")
}

/// Escapa o que não é seguro numa URL. `Message-ID` costuma ter `@`, `+`, `/` e
/// `=`, e sem escapar o Gmail lê parte deles como outra coisa.
fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Abre no Gmail o e-mail dado. Roda no worker: buscar o header é uma ida ao
/// IMAP, e a tela não pode congelar por causa dela.
pub fn open_in_gmail(account: Account, id: &str) -> Result<(), String> {
    let msgid = message_id(account, id)?;
    let address = account.address().unwrap_or_default();
    super::open_url(&gmail_url(&address, &msgid))
}

/// Busca o corpo de um e-mail, já legível.
///
/// Usa `--preview` de propósito: sem ele o himalaya marca o envelope como lido
/// só por ter sido aberto, e aqui o corpo também é buscado em segundo plano —
/// marcar como lido tem de ser uma decisão sua (`Espaço`), não efeito colateral
/// de o cursor ter passado por cima.
pub fn fetch_body(account: Account, id: &str) -> Result<String, String> {
    let raw = run(&[
        "message",
        "read",
        id,
        "-a",
        account.himalaya_name(),
        "--no-headers",
        "--preview",
    ])?;
    Ok(readable(&raw))
}

/// Deixa o corpo legível num terminal.
///
/// Muito e-mail só tem parte HTML, e o himalaya entrega a marcação crua — uma
/// parede de `<table>` e `style` que não se lê. Isto não é um renderizador de
/// HTML: é o suficiente para o texto voltar a ter parágrafos e linhas.
pub fn readable(raw: &str) -> String {
    if !looks_like_html(raw) {
        return collapse_blank_lines(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    let mut in_tag = false;
    let mut tag = String::new();
    let mut skip_until: Option<&'static str> = None;

    while let Some(c) = chars.next() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let name: String = tag
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect::<String>()
                    .to_ascii_lowercase();
                // `script`/`style` têm conteúdo que não é texto: pula até fechar.
                if skip_until.is_none() {
                    match name.as_str() {
                        "script" if !tag.starts_with('/') => skip_until = Some("script"),
                        "style" if !tag.starts_with('/') => skip_until = Some("style"),
                        _ => {}
                    }
                } else if Some(name.as_str()) == skip_until && tag.starts_with('/') {
                    skip_until = None;
                }
                // Tags que separam blocos viram quebra de linha.
                if skip_until.is_none()
                    && matches!(
                        name.as_str(),
                        "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "table" | "blockquote"
                    )
                {
                    out.push('\n');
                }
            }
            _ if in_tag => tag.push(c),
            _ if skip_until.is_some() => {}
            '&' => {
                let mut entity = String::new();
                while let Some(&n) = chars.peek() {
                    chars.next();
                    if n == ';' || entity.len() > 8 {
                        break;
                    }
                    entity.push(n);
                }
                out.push_str(&decode_entity(&entity));
            }
            _ => out.push(c),
        }
    }
    collapse_blank_lines(&out)
}

fn looks_like_html(raw: &str) -> bool {
    let head: String = raw.chars().take(2000).collect::<String>().to_lowercase();
    ["<html", "<body", "<div", "<table", "<p>", "<p ", "<br", "<!doctype html"]
        .iter()
        .any(|m| head.contains(m))
}

/// Entidades nomeadas que aparecem de fato em e-mail.
///
/// A lista é curta de propósito, mas cobre as acentuadas do português: e-mail em
/// pt-BR vem cheio de `&atilde;` e `&ccedil;`, e sem isso o corpo fica ilegível
/// justamente nas palavras que importam.
const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"), ("lt", "<"), ("gt", ">"), ("quot", "\""),
    ("apos", "'"), ("nbsp", " "),
    ("hellip", "…"), ("mdash", "—"), ("ndash", "–"), ("bull", "•"),
    ("laquo", "«"), ("raquo", "»"), ("deg", "°"), ("middot", "·"),
    ("copy", "©"), ("reg", "®"), ("trade", "™"), ("euro", "€"), ("pound", "£"),
    ("aacute", "á"), ("agrave", "à"), ("atilde", "ã"), ("acirc", "â"), ("auml", "ä"),
    ("eacute", "é"), ("egrave", "è"), ("ecirc", "ê"),
    ("iacute", "í"), ("icirc", "î"),
    ("oacute", "ó"), ("otilde", "õ"), ("ocirc", "ô"), ("ouml", "ö"),
    ("uacute", "ú"), ("ucirc", "û"), ("uuml", "ü"),
    ("ccedil", "ç"), ("ntilde", "ñ"),
    ("Aacute", "Á"), ("Atilde", "Ã"), ("Acirc", "Â"),
    ("Eacute", "É"), ("Ecirc", "Ê"), ("Iacute", "Í"),
    ("Oacute", "Ó"), ("Otilde", "Õ"), ("Ocirc", "Ô"),
    ("Uacute", "Ú"), ("Ccedil", "Ç"),
];

/// Decodifica uma entidade; o que não estiver na tabela vira o próprio texto.
fn decode_entity(entity: &str) -> String {
    if let Some((_, ch)) = ENTITIES.iter().find(|(name, _)| *name == entity) {
        return (*ch).into();
    }
    match entity {
        "#39" => "'".into(),
        "#160" => " ".into(),
        other => other
            .strip_prefix('#')
            .and_then(|n| n.parse::<u32>().ok())
            .and_then(char::from_u32)
            .map(String::from)
            .unwrap_or_else(|| format!("&{other};")),
    }
}

/// Tira espaço no fim das linhas e comprime sequências de linhas vazias.
fn collapse_blank_lines(raw: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut blanks = 0;
    for line in raw.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push(line);
    }
    out.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Saída real do `himalaya message read -H Message-ID --preview`: o header no
    // topo, o corpo logo abaixo (com o texto normalizado).
    const HEADER_THEN_BODY: &str = concat!(
        "Message-ID: <ua3mxwoUQ1yWgVpJXSkKYg@geopod-ismtpd-4>
",
        "
",
        "Bom dia, seguem os dados.
",
        "Message-ID: <isto-esta-no-corpo@exemplo.com>
",
    );

    #[test]
    fn the_message_id_comes_from_the_header_not_from_the_body() {
        // Um e-mail de resposta cita headers no corpo; vale a primeira linha.
        assert_eq!(
            parse_message_id(HEADER_THEN_BODY).unwrap(),
            "ua3mxwoUQ1yWgVpJXSkKYg@geopod-ismtpd-4"
        );
    }

    #[test]
    fn an_email_without_the_header_says_why_it_cannot_be_opened() {
        let err = parse_message_id("
só corpo aqui
").unwrap_err();
        assert!(err.contains("Message-ID"), "{err}");
    }

    #[test]
    fn the_gmail_link_searches_by_message_id_in_the_right_account() {
        let url = gmail_url("voce@exemplo.com", "abc+def/ghi=@mail.example.com");
        assert!(
            url.starts_with("https://mail.google.com/mail/u/voce%40exemplo.com/#search/"),
            "{url}"
        );
        // `+`, `/`, `=` e `@` escapados: sem isso o Gmail lê parte do id como
        // outra coisa e não acha a mensagem.
        assert!(url.ends_with("abc%2Bdef%2Fghi%3D%40mail.example.com"), "{url}");
    }

    #[test]
    fn the_account_goes_in_the_path_and_never_in_authuser() {
        // `/mail/u/?authuser=…` redireciona para resolver o índice da conta, e o
        // `#search/…` se perde no redirect: a aba abria na home da caixa.
        let url = gmail_url("voce@exemplo.com", "x@y");
        assert!(!url.contains("authuser"), "{url}");
        assert!(url.contains("/#search/rfc822msgid%3A"), "{url}");
    }

    #[test]
    fn without_a_configured_address_the_link_falls_back_to_the_first_account() {
        let url = gmail_url("", "x@y");
        assert!(url.starts_with("https://mail.google.com/mail/u/0/#search/"), "{url}");
    }

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
    fn plain_text_body_is_left_alone_apart_from_blank_lines() {
        let raw = "Oi João,   \n\n\n\nSegue o relatório.\n\n\n";
        assert_eq!(readable(raw), "Oi João,\n\nSegue o relatório.");
    }

    #[test]
    fn html_body_becomes_readable_text() {
        // Forma típica de e-mail de marketing: tabela, style, entidades.
        let raw = concat!(
            "<html><head><style>.x{color:red}</style></head><body>",
            "<div>Ol&aacute;, <b>Jo&atilde;o</b>!</div>",
            "<p>Seu saldo &eacute; R$&nbsp;1.234 &amp; sobe.</p>",
            "<script>track()</script>",
            "<table><tr><td>Item</td></tr><tr><td>Outro</td></tr></table>",
            "</body></html>"
        );
        let out = readable(raw);
        assert!(!out.contains('<'), "nenhuma tag sobra: {out}");
        assert!(!out.contains("color:red"), "o css não é texto");
        assert!(!out.contains("track()"), "o script não é texto");
        assert!(out.contains("João"), "entidade nomeada decodificada");
        assert!(out.contains("R$ 1.234 & sobe"), "nbsp e amp: {out}");
        let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(
            lines,
            vec!["Olá, João!", "Seu saldo é R$ 1.234 & sobe.", "Item", "Outro"],
            "cada bloco do HTML vira uma linha própria"
        );
    }

    #[test]
    fn numeric_entities_and_unknown_ones_survive() {
        assert_eq!(readable("<p>caf&#233; &naoexiste; fim</p>"), "café &naoexiste; fim");
    }

    #[test]
    fn seen_flag_verb_switches_between_add_and_remove() {
        assert_eq!(flag_verb(true), "add");
        assert_eq!(flag_verb(false), "remove");
    }

    #[test]
    fn delete_targets_the_trash_alias() {
        // Excluir é mover para a Lixeira, não apagar do servidor.
        assert_eq!(DELETE_FOLDER, "trash");
        assert!(FOLDER_ALIASES.contains(&DELETE_FOLDER));
    }

    #[test]
    fn folder_aliases_are_the_ones_the_config_declares() {
        assert_eq!(FOLDER_ALIASES, ["inbox", "sent", "drafts", "trash", "spam", "all"]);
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
