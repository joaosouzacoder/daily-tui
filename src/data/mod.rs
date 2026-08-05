//! Modelos de dados e busca (via CLIs externas) para os painéis.

pub mod agenda;
pub mod email;
pub mod jira;
pub mod notify;
pub mod pulls;
pub mod tasks;

pub use agenda::AgendaItem;
pub use email::EmailItem;
pub use tasks::TaskItem;

/// Cria um `Command` para um helper externo, tratando a diferença do Windows.
///
/// No Windows, `jira` e `mstodo` são shims `.cmd` — e o `CreateProcess`
/// (usado por `Command`) só executa `.exe` diretamente, então rodamos via
/// `cmd /C`. As CLIs `.exe` (himalaya/gcalcli/ghpending) chamam direto e não
/// passam por aqui. No Unix é sempre exec direto.
pub fn helper_command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    let mut cmd = {
        let mut cmd = std::process::Command::new("cmd");
        cmd.arg("/C").arg(program);
        cmd
    };
    #[cfg(not(windows))]
    let mut cmd = std::process::Command::new(program);
    helper_env(&mut cmd, program);
    cmd
}

/// Passa ao helper o que o config souber sobre ele.
///
/// Campo vazio não é injetado: o helper cai na variável do ambiente, que é como
/// o launcher do Windows e o `.bashrc` de quem já usava o painel entregam esses
/// valores. Token nenhum passa por aqui — `JIRA_TOKEN` e `GITHUB_TOKEN` vêm só
/// do ambiente, e o config de exemplo diz isso.
fn helper_env(cmd: &mut std::process::Command, program: &str) {
    let cfg = crate::config::get();
    let pairs: [(&str, &str); 2] = match program {
        "jira" => [
            ("JIRA_CLOUD", cfg.jira.cloud.as_str()),
            ("JIRA_EMAIL", cfg.jira.email.as_str()),
        ],
        "mstodo" => [
            ("DAILY_TUI_TODO_CLIENT_ID", cfg.tasks.client_id.as_str()),
            ("DAILY_TUI_TODO_LIST", cfg.tasks.list.as_str()),
        ],
        _ => return,
    };
    for (key, value) in pairs {
        if !value.is_empty() {
            cmd.env(key, value);
        }
    }
}

/// Força as CLIs escritas em Python (`gcalcli`, `jira`, `mstodo`) a emitir UTF-8.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Account {
    /// Primeira conta configurada (por convenção, a do trabalho).
    Work,
    /// Segunda conta configurada (por convenção, a pessoal).
    Personal,
}

impl Account {
    /// Nome estável do slot, para persistência.
    ///
    /// **Não** muda com o config: o cache de pastas no banco é chaveado por ele,
    /// e renomear a conta no himalaya não pode invalidar o cache.
    pub const fn slot_key(self) -> &'static str {
        match self {
            Account::Work => "work",
            Account::Personal => "personal",
        }
    }

    /// O slot com esta chave, se for um dos dois.
    pub fn from_slot_key(key: &str) -> Option<Self> {
        match key {
            "work" => Some(Account::Work),
            "personal" => Some(Account::Personal),
            _ => None,
        }
    }

    /// Posição do slot na lista de contas do config.
    const fn slot(self) -> usize {
        match self {
            Account::Work => 0,
            Account::Personal => 1,
        }
    }

    /// Config desta conta, ou `None` quando ela não existe no arquivo.
    pub fn cfg(self) -> Option<&'static crate::config::AccountCfg> {
        crate::config::get().accounts.get(self.slot())
    }

    /// Contas que existem, na ordem do config. Quem só tem uma conta configurada
    /// não recebe erro pela outra: ela simplesmente não é buscada.
    pub fn configured() -> Vec<Account> {
        [Account::Work, Account::Personal]
            .into_iter()
            .filter(|a| a.cfg().is_some())
            .collect()
    }

    /// Marcador curto exibido na lista (`[W]`, `[P]`).
    pub fn marker(self) -> String {
        let label = match self.cfg() {
            Some(c) if !c.label.is_empty() => c.label.clone(),
            // Sem rótulo no config, a inicial do slot serve.
            _ => self.slot_key()[..1].to_uppercase(),
        };
        format!("[{label}]")
    }

    /// Nome da conta no himalaya.
    pub fn himalaya_name(self) -> &'static str {
        match self.cfg() {
            Some(c) if !c.id.is_empty() => c.id.as_str(),
            _ => self.slot_key(),
        }
    }

    /// Subdiretório da conta no gcalcli (sob `~/.local/share/gcalcli-accounts`).
    pub fn gcalcli_dir(self) -> &'static str {
        match self.cfg() {
            Some(c) if !c.calendar.is_empty() => c.calendar.as_str(),
            _ => self.slot_key(),
        }
    }

    /// E-mail da calendar primária da conta — usado no `--calendar` do gcalcli
    /// para filtrar só a sua agenda (sem salas nem calendars de colegas).
    ///
    /// Vem do config; na falta dele, das variáveis `DAILY_TUI_WORK_EMAIL` /
    /// `DAILY_TUI_PERSONAL_EMAIL`, que é como isso funcionava antes do config
    /// existir. Por último, um placeholder — nunca um e-mail no código.
    pub fn primary_calendar(self) -> String {
        if let Some(c) = self.cfg()
            && !c.email.is_empty()
        {
            return c.email.clone();
        }
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

    // Falha real do antigo helper de Jira em PowerShell (removido na migração
    // para o helper `jira` em Python) quando o Atlassian devolveu 400. O repo é
    // público: o nome do script e o domínio foram trocados; o formato do erro —
    // que é o que este fixture testa — está como o PowerShell o emitiu.
    const POWERSHELL_HTTP_FAIL: &str = concat!(
        "Invoke-RestMethod : The remote server returned an error: (400) Bad Request.\n",
        "At C:\\Users\\voce\\projects\\daily-tui\\scripts\\jira-legacy.ps1:39 char:9\n",
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
        let msg = stderr_summary(POWERSHELL_HTTP_FAIL);
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
        // Antes o painel mostrava só "jira falhou:" e nada mais.
        assert_eq!(stderr_summary(""), "sem detalhes no stderr");
        assert_eq!(stderr_summary("\n  \n"), "sem detalhes no stderr");
    }

    #[test]
    fn output_carries_no_ansi_escapes() {
        assert!(!stderr_summary(HIMALAYA_OAUTH_FAIL).contains('\x1b'));
    }

    #[test]
    fn an_account_reads_its_names_from_the_config() {
        // Sem config, os nomes são os de sempre: quem já usava não sente.
        assert_eq!(Account::Work.himalaya_name(), "work");
        assert_eq!(Account::Personal.himalaya_name(), "personal");
        assert_eq!(Account::Work.marker(), "[W]");
        assert_eq!(Account::Personal.marker(), "[P]");
        assert_eq!(Account::Work.gcalcli_dir(), "work");
    }

    #[test]
    fn the_slot_key_does_not_follow_a_renamed_account() {
        // O banco chaveia o cache de pastas por ele: renomear a conta no
        // himalaya não pode invalidar o cache.
        assert_eq!(Account::Work.slot_key(), "work");
        assert_eq!(Account::Personal.slot_key(), "personal");
        assert_eq!(Account::from_slot_key("personal"), Some(Account::Personal));
        assert_eq!(Account::from_slot_key("faculdade"), None);
    }

    #[test]
    fn the_default_config_has_both_slots() {
        assert_eq!(
            Account::configured(),
            vec![Account::Work, Account::Personal],
            "sem config, as duas contas de sempre"
        );
    }

    #[test]
    fn the_task_helper_gets_the_client_id_from_the_config() {
        // O default do config já tem o client público, então ele é injetado
        // mesmo sem o usuário exportar variável nenhuma.
        let cmd = helper_command("mstodo");
        let value = cmd
            .get_envs()
            .find(|(k, _)| *k == std::ffi::OsStr::new("DAILY_TUI_TODO_CLIENT_ID"))
            .and_then(|(_, v)| v);
        assert!(value.is_some(), "o client id vai para o helper");
    }

    #[test]
    fn an_empty_config_field_leaves_the_environment_in_charge() {
        // `jira.cloud` vazio (o default) não pode virar `JIRA_CLOUD=""` e
        // atropelar quem exporta a variável no shell ou no launcher.
        let cmd = helper_command("jira");
        assert!(
            !cmd.get_envs()
                .any(|(k, _)| k == std::ffi::OsStr::new("JIRA_CLOUD")),
            "sem valor no config, nada é injetado"
        );
    }

    #[test]
    fn a_helper_without_settings_gets_none_injected() {
        let cmd = helper_command("gcalcli");
        let injected: Vec<_> = cmd
            .get_envs()
            .filter(|(k, _)| *k != std::ffi::OsStr::new("PYTHONIOENCODING"))
            .collect();
        assert!(injected.is_empty());
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
