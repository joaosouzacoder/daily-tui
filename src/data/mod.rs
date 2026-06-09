//! Modelos de dados e busca (via CLIs externas) para os painéis.

pub mod agenda;
pub mod email;
pub mod pulls;

pub use agenda::AgendaItem;
pub use email::EmailItem;

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

    /// Título da calendar primária (igual ao e-mail da conta). Usado para
    /// filtrar só a agenda do próprio João, sem salas nem colegas assinados.
    pub const fn primary_calendar(self) -> &'static str {
        match self {
            Account::Work => "you-work@example.com",
            Account::Personal => "you@example.com",
        }
    }
}
