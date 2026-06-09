//! Thread de fundo que roda os CLIs (himalaya/gcalcli/ghpending) sem bloquear
//! o loop principal nem o relógio.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ratatui_tea::ProgramHandle;

use crate::data::{agenda, email, pulls, Account};
use crate::msg::Msg;

/// Quantos e-mails buscar por conta.
const EMAIL_LIMIT: u32 = 50;

/// Comandos enviados do loop principal para o worker.
pub enum WorkerCmd {
    /// Recarrega e-mails, agenda e PRs.
    RefreshAll,
    /// Busca o corpo de um e-mail para o overlay de detalhe.
    ReadEmail { account: Account, id: String },
    /// Encerra a thread.
    Quit,
}

/// Sobe a thread do worker. Retorna o canal de comandos e o handle da thread.
///
/// O worker faz um refresh inicial e, depois, refaz tudo a cada `refresh`
/// (ou quando recebe `RefreshAll`/`ReadEmail`).
pub fn spawn(
    ui: ProgramHandle<Msg>,
    refresh: Duration,
) -> (Sender<WorkerCmd>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<WorkerCmd>();

    let handle = thread::spawn(move || {
        refresh_all(&ui);

        loop {
            match rx.recv_timeout(refresh) {
                Ok(WorkerCmd::RefreshAll) | Err(RecvTimeoutError::Timeout) => refresh_all(&ui),
                Ok(WorkerCmd::ReadEmail { account, id }) => {
                    let _ = ui.send(Msg::EmailBody(email::fetch_body(account, &id)));
                }
                Ok(WorkerCmd::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    (tx, handle)
}

/// Busca os três conjuntos de dados e manda cada resultado assim que fica pronto.
fn refresh_all(ui: &ProgramHandle<Msg>) {
    let _ = ui.send(Msg::EmailsLoaded(fetch_emails()));
    let _ = ui.send(Msg::AgendaLoaded(fetch_agenda()));
    let _ = ui.send(Msg::PullsLoaded(pulls::fetch()));
}

/// Agrega e-mails das duas contas e ordena do mais recente para o mais antigo.
fn fetch_emails() -> Result<Vec<email::EmailItem>, String> {
    let mut all = email::fetch(Account::Work, EMAIL_LIMIT)?;
    all.extend(email::fetch(Account::Personal, EMAIL_LIMIT)?);
    email::sort_recent_first(&mut all);
    Ok(all)
}

/// Agrega a agenda das duas contas e ordena cronologicamente.
fn fetch_agenda() -> Result<Vec<agenda::AgendaItem>, String> {
    let mut all = agenda::fetch(Account::Work)?;
    all.extend(agenda::fetch(Account::Personal)?);
    agenda::sort_chronologically(&mut all);
    Ok(all)
}
