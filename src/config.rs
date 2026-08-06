//! Config do usuário: quais painéis existem e quais contas existem.
//!
//! Um arquivo TOML no diretório de config do SO. Ausente é válido — vale o
//! default, que é exatamente o comportamento de antes deste módulo existir.
//! Inválido não é: cair no default em silêncio faria a pessoa achar que o
//! config não pega.
//!
//! O config do processo vive num `OnceLock`, fixado pelo `main` antes de
//! qualquer busca. É o que evita carregar `&Config` por uma dúzia de
//! assinaturas só para o `Account` saber o próprio nome.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

/// Client público first-party da Microsoft ("Microsoft Graph Command Line
/// Tools"), usado pelo helper de tarefas quando o config não diz outro.
const TODO_CLIENT_ID: &str = "14d82eec-204b-4c2f-b7e8-296a70dab67e";

/// Config completo, com defaults que reproduzem o painel de antes do config.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub panels: Panels,
    #[serde(default = "default_accounts")]
    pub accounts: Vec<AccountCfg>,
    #[serde(default)]
    pub email: EmailCfg,
    #[serde(default)]
    pub jira: JiraCfg,
    #[serde(default)]
    pub tasks: TasksCfg,
    #[serde(default)]
    pub refresh: RefreshCfg,
    #[serde(default)]
    pub pomodoro: PomodoroCfg,
    #[serde(default)]
    pub notify: NotifyCfg,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            panels: Panels::default(),
            accounts: default_accounts(),
            email: EmailCfg::default(),
            jira: JiraCfg::default(),
            tasks: TasksCfg::default(),
            refresh: RefreshCfg::default(),
            pomodoro: PomodoroCfg::default(),
            notify: NotifyCfg::default(),
        }
    }
}

/// Painéis ligados.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Panels {
    #[serde(default = "yes")]
    pub email: bool,
    #[serde(default = "yes")]
    pub jira: bool,
    #[serde(default = "yes")]
    pub agenda: bool,
    #[serde(default = "yes")]
    pub pulls: bool,
    #[serde(default = "yes")]
    pub tasks: bool,
}

const fn yes() -> bool {
    true
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            email: true,
            jira: true,
            agenda: true,
            pulls: true,
            tasks: true,
        }
    }
}

impl Panels {
    /// `true` se algum painel está ligado.
    pub const fn any(self) -> bool {
        self.email || self.jira || self.agenda || self.pulls || self.tasks
    }
}

/// Uma conta: como o himalaya e o gcalcli a chamam, e como ela aparece na tela.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccountCfg {
    /// Nome da conta no himalaya.
    pub id: String,
    /// Marcador exibido na lista (`W` vira `[W]`).
    #[serde(default)]
    pub label: String,
    /// Calendar primária no gcalcli.
    #[serde(default)]
    pub email: String,
    /// Subpasta da conta em `~/.local/share/gcalcli-accounts`.
    #[serde(default)]
    pub calendar: String,
}

/// As duas contas de sempre, para quem não escreveu config.
fn default_accounts() -> Vec<AccountCfg> {
    ["work", "personal"]
        .into_iter()
        .map(|id| AccountCfg {
            id: id.to_string(),
            label: id[..1].to_uppercase(),
            email: String::new(),
            calendar: id.to_string(),
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmailCfg {
    #[serde(default = "default_email_limit")]
    pub limit: u32,
}

const fn default_email_limit() -> u32 {
    30
}

impl Default for EmailCfg {
    fn default() -> Self {
        Self {
            limit: default_email_limit(),
        }
    }
}

/// Jira. Campo vazio = o helper cai na variável de ambiente equivalente, que é
/// como o launcher do Windows e o `.bashrc` de quem já usa o painel entregam
/// esses valores.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JiraCfg {
    #[serde(default)]
    pub cloud: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TasksCfg {
    /// Nome da lista no To Do; vazio = a lista padrão.
    #[serde(default)]
    pub list: String,
    #[serde(default = "default_client_id")]
    pub client_id: String,
}

fn default_client_id() -> String {
    TODO_CLIENT_ID.to_string()
}

impl Default for TasksCfg {
    fn default() -> Self {
        Self {
            list: String::new(),
            client_id: default_client_id(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RefreshCfg {
    #[serde(default = "default_refresh")]
    pub seconds: u64,
}

const fn default_refresh() -> u64 {
    300
}

impl Default for RefreshCfg {
    fn default() -> Self {
        Self {
            seconds: default_refresh(),
        }
    }
}

/// Pomodoro do header. Tempos em minutos: é a unidade em que se pensa um
/// pomodoro, e segundos no config só convidariam a erro de digitação.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PomodoroCfg {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_focus")]
    pub focus: u64,
    #[serde(default = "default_rest")]
    pub rest: u64,
}

const fn default_focus() -> u64 {
    25
}

const fn default_rest() -> u64 {
    5
}

/// Teto de uma fase, em minutos (24h). Sem ele, um typo como `focus = 2500`
/// (querendo dizer "25,00") rende `2500:00`, que estoura os 20 caracteres úteis
/// da caixa e corta em silêncio.
const MAX_POMODORO_MINUTES: u64 = 24 * 60;

impl Default for PomodoroCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            focus: default_focus(),
            rest: default_rest(),
        }
    }
}

/// Canal de notificação. Vazio = só a notificação do sistema; com tópico, o
/// ntfy.sh entra quando a do sistema falha.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NotifyCfg {
    #[serde(default)]
    pub ntfy_topic: String,
}

/// Parseia e valida. O erro é uma linha, para caber no stderr.
fn parse(raw: &str) -> Result<Config, String> {
    let cfg: Config = toml::from_str(raw).map_err(|e| {
        // A mensagem do `toml` tem várias linhas: a primeira dá a posição, o
        // meio desenha o trecho com uma seta, e a **última** é o motivo
        // ("unknown field `pannels`"). Pegar só a primeira jogaria fora
        // justamente o nome do campo errado.
        let msg = e.to_string();
        let lines: Vec<&str> = msg.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let position = lines.first().copied().unwrap_or("formato inválido");
        let reason = lines.last().copied().unwrap_or(position);
        if reason == position {
            format!("config inválido: {position}")
        } else {
            format!("config inválido: {reason} ({position})")
        }
    })?;
    cfg.validate()?;
    Ok(cfg)
}

impl Config {
    fn validate(&self) -> Result<(), String> {
        if !self.panels.any() {
            return Err("nenhum painel ligado: o [panels] desligou todos".into());
        }
        // Fase de zero minuto viraria a cada tick: uma rajada de notificações
        // que só para quando o painel fecha.
        if self.pomodoro.focus == 0 {
            return Err("[pomodoro] focus tem de ser maior que zero".into());
        }
        if self.pomodoro.rest == 0 {
            return Err("[pomodoro] rest tem de ser maior que zero".into());
        }
        if self.pomodoro.focus > MAX_POMODORO_MINUTES {
            return Err("[pomodoro] focus não pode passar de 1440 minutos (24h)".into());
        }
        if self.pomodoro.rest > MAX_POMODORO_MINUTES {
            return Err("[pomodoro] rest não pode passar de 1440 minutos (24h)".into());
        }
        match self.accounts.len() {
            0 => Err("nenhuma conta configurada: e-mail e agenda ficariam vazios".into()),
            1 | 2 => Ok(()),
            n => Err(format!("são no máximo duas contas, e o config traz {n}")),
        }
    }

    /// Despeja o config em linhas `CHAVE="valor"` que o `setup-auth.sh` come
    /// com um `eval`. Existe para o doctor cobrar só o painel que está ligado.
    pub fn print_shell(&self) -> String {
        let flag = |on: bool| if on { 1 } else { 0 };
        let ids: Vec<&str> = self.accounts.iter().map(|a| a.id.as_str()).collect();
        format!(
            "PANEL_EMAIL={}\nPANEL_JIRA={}\nPANEL_AGENDA={}\nPANEL_PULLS={}\nPANEL_TASKS={}\n\
             ACCOUNT_IDS=\"{}\"\nJIRA_CLOUD=\"{}\"\nTASKS_LIST=\"{}\"\n",
            flag(self.panels.email),
            flag(self.panels.jira),
            flag(self.panels.agenda),
            flag(self.panels.pulls),
            flag(self.panels.tasks),
            ids.join(" "),
            self.jira.cloud,
            self.tasks.list,
        )
    }
}

/// Onde o config vive quando ninguém passa `--config`.
pub fn default_path() -> PathBuf {
    // `cfg!` em vez de `#[cfg]`: os dois ramos são compilados em toda
    // plataforma, então o build no Windows também checa o caminho Unix. Com
    // `#[cfg]`, o lado inativo nunca passa pelo compilador — e foi assim que
    // este arquivo nasceu sem nunca ter sido verificado no Linux.
    let base = if cfg!(windows) {
        std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|_| PathBuf::from("."))
    };
    base.join("daily-tui").join("config.toml")
}

/// Carrega o config. Caminho pedido na mão que não existe é erro; o caminho
/// default ausente não é — é o caso de quem nunca escreveu config.
pub fn load(path: Option<&Path>) -> Result<Config, String> {
    let (path, required) = match path {
        Some(p) => (p.to_path_buf(), true),
        None => (default_path(), false),
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => parse(&raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => Ok(Config::default()),
        Err(e) => Err(format!("não deu para ler {}: {e}", path.display())),
    }
}

/// Escreve o exemplo comentado no caminho default. Não sobrescreve: o config de
/// alguém é trabalho manual.
pub fn write_example() -> Result<PathBuf, String> {
    let path = default_path();
    if path.exists() {
        return Err(format!("{} já existe — abra e edite", path.display()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("não deu para criar {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, include_str!("../config.example.toml"))
        .map_err(|e| format!("não deu para escrever {}: {e}", path.display()))?;
    Ok(path)
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Fixa o config do processo. Chamado uma vez, no `main`, antes de qualquer
/// busca.
pub fn init(cfg: Config) {
    let _ = CONFIG.set(cfg);
}

/// Config do processo. Em teste, sem `init`, vale o default — que é o
/// comportamento histórico do painel.
pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_example_parses_into_the_defaults_people_expect() {
        // O exemplo é documentação: se ele não parseia, a documentação mente.
        let cfg = parse(include_str!("../config.example.toml")).expect("exemplo válido");
        assert!(cfg.panels.email && cfg.panels.jira && cfg.panels.tasks);
        assert_eq!(cfg.accounts.len(), 2);
        assert_eq!(cfg.accounts[0].id, "work");
        assert_eq!(cfg.accounts[1].label, "P");
        assert_eq!(cfg.email.limit, 30);
        assert_eq!(cfg.refresh.seconds, 300);
        assert_eq!(cfg.pomodoro.focus, 25);
        assert_eq!(cfg.pomodoro.rest, 5);
    }

    #[test]
    fn an_empty_file_gives_the_classic_twenty_five_five_pomodoro() {
        let cfg = parse("").expect("vazio é válido");
        assert!(cfg.pomodoro.enabled);
        assert_eq!(cfg.pomodoro.focus, 25);
        assert_eq!(cfg.pomodoro.rest, 5);
        // Sem tópico, o único canal é a notificação do sistema.
        assert!(cfg.notify.ntfy_topic.is_empty());
    }

    #[test]
    fn the_pomodoro_times_come_from_the_file() {
        let cfg = parse("[pomodoro]\nfocus = 50\nrest = 10\n").unwrap();
        assert_eq!(cfg.pomodoro.focus, 50);
        assert_eq!(cfg.pomodoro.rest, 10);
        // Campo omitido não desliga a caixa.
        assert!(cfg.pomodoro.enabled);
    }

    #[test]
    fn a_zero_length_phase_is_refused_with_the_field_named() {
        // Fase de zero minuto viraria a cada tick: um laço de notificações.
        let err = parse("[pomodoro]\nfocus = 0\n").unwrap_err();
        assert!(err.contains("focus"), "o erro nomeia o campo: {err}");
        let err = parse("[pomodoro]\nrest = 0\n").unwrap_err();
        assert!(err.contains("rest"), "o erro nomeia o campo: {err}");
    }

    #[test]
    fn a_phase_over_twenty_four_hours_is_refused_with_the_field_named() {
        // `focus = 2500` (typo de "25,00") renderizaria `2500:00`, estourando
        // a caixa de 20 colunas em silêncio.
        let err = parse("[pomodoro]\nfocus = 2500\n").unwrap_err();
        assert!(err.contains("focus"), "o erro nomeia o campo: {err}");
        let err = parse("[pomodoro]\nrest = 1441\n").unwrap_err();
        assert!(err.contains("rest"), "o erro nomeia o campo: {err}");
    }

    #[test]
    fn the_ntfy_topic_comes_from_the_notify_section() {
        let cfg = parse("[notify]\nntfy_topic = \"meutopico\"\n").unwrap();
        assert_eq!(cfg.notify.ntfy_topic, "meutopico");
    }

    #[test]
    fn an_empty_file_means_everything_on_with_the_two_usual_accounts() {
        // Quem já usava o painel não pode perder nada por não ter config.
        let cfg = parse("").expect("vazio é válido");
        assert_eq!(cfg, Config::default());
        assert!(cfg.panels.email && cfg.panels.jira && cfg.panels.agenda);
        assert!(cfg.panels.pulls && cfg.panels.tasks);
        let ids: Vec<&str> = cfg.accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["work", "personal"]);
        assert_eq!(cfg.accounts[0].label, "W");
    }

    #[test]
    fn one_account_is_a_valid_setup() {
        let cfg = parse(
            r#"
            [[accounts]]
            id = "gmail"
            label = "G"
            email = "eu@exemplo.com"
            calendar = "gmail"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.accounts.len(), 1);
        assert_eq!(cfg.accounts[0].id, "gmail");
    }

    #[test]
    fn turning_one_panel_off_leaves_the_others_alone() {
        let cfg = parse("[panels]\ntasks = false\n").unwrap();
        assert!(!cfg.panels.tasks);
        assert!(cfg.panels.email && cfg.panels.jira && cfg.panels.agenda && cfg.panels.pulls);
    }

    #[test]
    fn a_typo_is_an_error_instead_of_a_silent_default() {
        // `pannels` ligaria tudo em silêncio e a pessoa acharia que o config
        // não funciona.
        let err = parse("[pannels]\nemail = false\n").unwrap_err();
        assert!(err.contains("pannels"), "o erro nomeia o campo: {err}");
    }

    #[test]
    fn turning_every_panel_off_is_refused() {
        let err = parse(
            "[panels]\nemail=false\njira=false\nagenda=false\npulls=false\ntasks=false\n",
        )
        .unwrap_err();
        assert!(err.contains("nenhum painel"), "{err}");
    }

    #[test]
    fn no_account_is_refused_because_two_panels_depend_on_one() {
        let err = parse("accounts = []\n").unwrap_err();
        assert!(err.contains("conta"), "{err}");
    }

    #[test]
    fn three_accounts_are_refused_with_the_reason() {
        let one = "[[accounts]]\nid = \"a\"\nlabel = \"A\"\n";
        let err = parse(&one.repeat(3)).unwrap_err();
        assert!(err.contains("duas"), "{err}");
    }

    #[test]
    fn the_error_of_a_broken_file_is_a_single_line() {
        // O erro sai no stderr antes da TUI abrir; várias linhas viram ruído.
        let err = parse("[panels\nemail = true\n").unwrap_err();
        assert_eq!(err.lines().count(), 1, "{err}");
    }

    #[test]
    fn the_shell_dump_is_consumable_by_the_doctor_script() {
        let cfg = parse("[panels]\njira = false\n").unwrap();
        let dump = cfg.print_shell();
        assert!(dump.contains("PANEL_EMAIL=1"));
        assert!(dump.contains("PANEL_JIRA=0"));
        assert!(dump.contains("ACCOUNT_IDS=\"work personal\""));
    }

    #[test]
    fn a_config_asked_for_by_name_has_to_exist() {
        // Pedir `--config` e receber o default em silêncio é o pior dos mundos.
        let missing = std::env::temp_dir().join("daily-tui-nao-existe-mesmo.toml");
        let _ = std::fs::remove_file(&missing);
        let err = load(Some(&missing)).unwrap_err();
        assert!(err.contains("nao-existe-mesmo"), "diz qual arquivo: {err}");
    }

    #[test]
    fn the_default_path_lands_in_the_user_config_directory() {
        let path = default_path();
        assert_eq!(path.file_name().unwrap(), "config.toml");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "daily-tui");
    }
}
