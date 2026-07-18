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
/// No Windows, `jirapending` e `gtasks` são shims `.cmd` — e o `CreateProcess`
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
