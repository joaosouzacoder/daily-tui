//! Modelos de dados e busca (via CLIs externas) para os painéis.

pub mod agenda;
pub mod email;
pub mod jira;
pub mod pulls;
pub mod tasks;

pub use agenda::AgendaItem;
pub use email::EmailItem;
pub use tasks::TaskItem;

/// Cria um `Command` para um helper externo, tratando a diferença do Windows.
///
/// No Windows, `jirapending` e `mstodo` são shims `.cmd` — e o `CreateProcess`
/// (usado por `Command`) só executa `.exe` diretamente, então rodamos via
/// `cmd /C`. As CLIs `.exe` (himalaya/gcalcli/ghpending) chamam direto e não
/// passam por aqui. No Unix é sempre exec direto.
pub fn helper_command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("cmd");
        cmd.arg("/C").arg(program);
        cmd
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(program)
    }
}

/// Força as CLIs escritas em Python (`gcalcli`, `mstodo`) a emitir UTF-8.
///
/// Com o stdout num pipe o Python não usa UTF-8, e sim a codificação de locale
/// — no Windows, a ANSI code page (cp1252 nesta máquina). Aí "Escritório" sai
/// como o byte solto `0xF3`, que não é UTF-8 válido, e o `from_utf8_lossy` do
/// lado Rust o troca por `�` ("Escrit�rio").
pub fn force_utf8_stdout(cmd: &mut std::process::Command) {
    cmd.env("PYTHONIOENCODING", "utf-8");
}

/// Extrai do stderr de um helper a linha que explica a falha.
///
/// Pegar a *última* linha não serve: o himalaya termina com dicas
/// (`Note: Run with --trace…`) e o PowerShell com metadados do erro
/// (`+ FullyQualifiedErrorId : …`) — o motivo real fica escondido.
///
/// Três formas cobertas, todas vistas em falhas reais dos helpers:
/// - cadeia de causas do himalaya (`Error:` seguido de `0: …`, `1: …`), onde a
///   causa mais funda é a específica ("cannot refresh access token…");
/// - traceback de Python (`gcalcli`), onde a primeira linha é só o cabeçalho
///   `Traceback (most recent call last):` e a exceção real é a última;
/// - erro de uma linha só (PowerShell/`ghpending`), onde vale a primeira.
///
/// Devolve texto sem escapes ANSI, ou uma nota quando o stderr nada diz.
pub fn stderr_summary(raw: &str) -> String {
    const PY_TRACEBACK: &str = "Traceback (most recent call last):";

    let clean = strip_ansi(raw);
    let meaningful: Vec<&str> = clean
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Note:") && *l != "Error:")
        .collect();

    // Traceback de Python: o Python pode encadear vários blocos ("The above
    // exception was the direct cause of…"), e a exceção que de fato abortou o
    // processo é sempre a última linha.
    if meaningful.first() == Some(&PY_TRACEBACK) {
        return meaningful.last().copied().unwrap_or(PY_TRACEBACK).to_string();
    }

    // Causa mais funda da cadeia do himalaya, quando houver.
    let deepest_cause = meaningful
        .iter()
        .rev()
        .find(|l| matches!(l.split_once(": "), Some((n, _)) if n.parse::<u32>().is_ok()))
        .and_then(|l| l.split_once(": ").map(|(_, msg)| msg));

    match deepest_cause.or_else(|| meaningful.first().copied()) {
        Some(msg) => msg.to_string(),
        None => "sem detalhes no stderr".to_string(),
    }
}

/// Remove escapes ANSI (CSI) do texto.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for ch in chars.by_ref() {
                if ch.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Conta de origem de um item (e-mail ou agenda).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Account {
    /// Conta de trabalho (Empresa).
    Work,
    /// Conta pessoal (Gmail).
    Personal,
}

impl Account {
    /// Marcador curto exibido na lista.
    pub const fn marker(self) -> &'static str {
        match self {
            Account::Work => "[W]",
            Account::Personal => "[P]",
        }
    }

    /// Nome da conta no himalaya.
    pub const fn himalaya_name(self) -> &'static str {
        match self {
            Account::Work => "work",
            Account::Personal => "personal",
        }
    }

    /// Subdiretório da conta no gcalcli (sob `~/.local/share/gcalcli-accounts`).
    pub const fn gcalcli_dir(self) -> &'static str {
        match self {
            Account::Work => "work",
            Account::Personal => "personal",
        }
    }

    /// E-mail da calendar primária da conta — usado no `--calendar` do gcalcli
    /// para filtrar só a sua agenda (sem salas nem calendars de colegas).
    ///
    /// Lido das variáveis `DAILY_TUI_WORK_EMAIL` / `DAILY_TUI_PERSONAL_EMAIL`,
    /// com placeholder de fallback, para não fixar e-mail no código.
    pub fn primary_calendar(self) -> String {
        let (var, default) = match self {
            Account::Work => ("DAILY_TUI_WORK_EMAIL", "you-work@example.com"),
            Account::Personal => ("DAILY_TUI_PERSONAL_EMAIL", "you@example.com"),
        };
        std::env::var(var).unwrap_or_else(|_| default.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // stderr real de `himalaya envelope list -a work` com o refresh token
    // inválido (2026-07-31), incluindo os escapes ANSI e as dicas finais.
    const HIMALAYA_OAUTH_FAIL: &str = concat!(
        "\x1b[2m2026-07-31T13:55:41.295098Z\x1b[0m \x1b[31mERROR\x1b[0m \x1b[2mimap_client::tasks::tasks::authenticate\x1b[0m\x1b[2m:\x1b[0m cannot authenticate using XOAUTH2 mechanism: {\\\"status\\\":\\\"400\\\",\\\"scope\\\":\\\"https://mail.google.com/\\\"}\n",
        "\x1b[2m2026-07-31T13:55:41.519835Z\x1b[0m \x1b[33m WARN\x1b[0m \x1b[2memail::imap\x1b[0m\x1b[2m:\x1b[0m authentication failed, refreshing access token and retrying…\n",
        "Error: \n",
        "   0: \x1b[91mcannot build IMAP client\x1b[0m\n",
        "   1: \x1b[91mcannot refresh oauth access token\x1b[0m\n",
        "   2: \x1b[91mcannot refresh oauth2 access token\x1b[0m\n",
        "   3: \x1b[91mcannot refresh access token using the refresh token\x1b[0m\n",
        "\n",
        "\x1b[96mNote\x1b[0m: Run with --debug to enable logs with spantrace.\n",
        "\x1b[96mNote\x1b[0m: Run with --trace to enable verbose logs with backtrace.\n",
    );

    // stderr real do `jirapending` quando o domínio Atlassian devolveu 400.
    const JIRAPENDING_HTTP_FAIL: &str = concat!(
        "Invoke-RestMethod : The remote server returned an error: (400) Bad Request.\n",
        "At C:\\Users\\voce\\projects\\daily-tui\\scripts\\jirapending.ps1:39 char:9\n",
        "+ $resp = Invoke-RestMethod -Method Post -Uri \"https://$cloud/rest/api/ ...\n",
        "+         ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~\n",
        "    + CategoryInfo          : InvalidOperation: (System.Net.HttpWebRequest:HttpWebRequest) [Invoke-RestMethod], WebException\n",
        "    + FullyQualifiedErrorId : WebCmdletWebResponseException,Microsoft.PowerShell.Commands.InvokeRestMethodCommand\n",
    );

    #[test]
    fn picks_deepest_cause_of_himalaya_chain_not_the_trailing_hint() {
        let msg = stderr_summary(HIMALAYA_OAUTH_FAIL);
        assert_eq!(msg, "cannot refresh access token using the refresh token");
    }

    #[test]
    fn picks_first_line_of_a_powershell_error() {
        let msg = stderr_summary(JIRAPENDING_HTTP_FAIL);
        assert_eq!(
            msg,
            "Invoke-RestMethod : The remote server returned an error: (400) Bad Request."
        );
    }

    // Traceback real do `mstodo list` com `HTTPS_PROXY=http://127.0.0.1:9`
    // (2026-08-03) — o mesmo cenário de uma queda de Wi-Fi. Capturado do stderr
    // do helper; só os fins de linha foram normalizados para LF.
    const MSTODO_PROXY_TRACEBACK: &str = include_str!("testdata/mstodo-proxy-traceback.txt");

    #[test]
    fn picks_the_exception_of_a_python_traceback_not_its_header() {
        let msg = stderr_summary(MSTODO_PROXY_TRACEBACK);
        assert_eq!(
            msg,
            "requests.exceptions.ProxyError: HTTPSConnectionPool(host='login.microsoftonline.com', port=443): Max retries exceeded with url: /consumers/v2.0/.well-known/openid-configuration (Caused by ProxyError('Unable to connect to proxy', NewConnectionError(\"HTTPSConnection(host='127.0.0.1', port=9): Failed to establish a new connection: [WinError 10061] No connection could be made because the target machine actively refused it\")))"
        );
    }

    #[test]
    fn keeps_a_single_line_message_as_is() {
        assert_eq!(stderr_summary("defina JIRA_EMAIL\n"), "defina JIRA_EMAIL");
    }

    #[test]
    fn says_something_when_stderr_is_silent() {
        // Antes o painel mostrava só "jirapending falhou:" e nada mais.
        assert_eq!(stderr_summary(""), "sem detalhes no stderr");
        assert_eq!(stderr_summary("\n  \n"), "sem detalhes no stderr");
    }

    #[test]
    fn output_carries_no_ansi_escapes() {
        assert!(!stderr_summary(HIMALAYA_OAUTH_FAIL).contains('\x1b'));
    }

    #[test]
    fn forces_python_helpers_to_emit_utf8() {
        let mut cmd = std::process::Command::new("gcalcli");
        force_utf8_stdout(&mut cmd);
        let set: Vec<_> = cmd
            .get_envs()
            .filter(|(k, _)| *k == std::ffi::OsStr::new("PYTHONIOENCODING"))
            .collect();
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].1, Some(std::ffi::OsStr::new("utf-8")));
    }
}
