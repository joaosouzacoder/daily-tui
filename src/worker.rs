//! Thread de fundo que roda os CLIs (himalaya/gcalcli/ghpending) sem bloquear
//! o loop principal nem o relógio.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ratatui_tea::ProgramHandle;

use crate::data::{agenda, email, jira, pulls, tasks, Account};
use crate::msg::Msg;

/// Quantos e-mails buscar por conta.
const EMAIL_LIMIT: u32 = 50;

/// Comandos enviados do loop principal para o worker.
pub enum WorkerCmd {
    /// Recarrega e-mails, agenda, PRs e tarefas.
    RefreshAll,
    /// Busca o corpo de um e-mail para o overlay de detalhe.
    ReadEmail { account: Account, id: String },
    /// Marca/desmarca um e-mail como lido; depois re-busca a lista.
    SetEmailSeen { account: Account, id: String, seen: bool },
    /// Move um e-mail para a pasta `target`; depois re-busca a lista.
    MoveEmail { account: Account, id: String, target: String },
    /// Lista as pastas da conta (para o seletor de "mover").
    ListFolders(Account),
    /// Escrita no Google Tasks; após executar, re-busca a lista.
    TaskComplete(String),
    TaskReopen(String),
    TaskAdd(String),
    TaskEdit { id: String, title: String },
    TaskDelete(String),
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
                Ok(WorkerCmd::SetEmailSeen { account, id, seen }) => {
                    mutate_emails(&ui, email::set_seen(account, &id, seen))
                }
                Ok(WorkerCmd::MoveEmail { account, id, target }) => {
                    mutate_emails(&ui, email::move_to(account, &id, &target))
                }
                Ok(WorkerCmd::ListFolders(account)) => {
                    let _ = ui.send(Msg::FoldersLoaded(email::folders(account)));
                }
                Ok(WorkerCmd::TaskComplete(id)) => mutate_tasks(&ui, tasks::complete(&id)),
                Ok(WorkerCmd::TaskReopen(id)) => mutate_tasks(&ui, tasks::reopen(&id)),
                Ok(WorkerCmd::TaskAdd(title)) => mutate_tasks(&ui, tasks::add(&title)),
                Ok(WorkerCmd::TaskEdit { id, title }) => {
                    mutate_tasks(&ui, tasks::edit(&id, &title))
                }
                Ok(WorkerCmd::TaskDelete(id)) => mutate_tasks(&ui, tasks::delete(&id)),
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
    let _ = ui.send(Msg::JiraLoaded(jira::fetch()));
    let _ = ui.send(Msg::TasksLoaded(tasks::fetch()));
}

/// Aplica uma escrita no Google Tasks e re-busca a lista. Se a escrita falhar,
/// propaga o erro para o painel; senão, manda a lista atualizada.
fn mutate_tasks(ui: &ProgramHandle<Msg>, result: Result<(), String>) {
    let loaded = result.and_then(|()| tasks::fetch());
    let _ = ui.send(Msg::TasksLoaded(loaded));
}

/// Aplica uma escrita de e-mail (flag/move) e re-busca a lista. Se a escrita
/// falhar, propaga o erro para o painel; senão, manda a lista atualizada.
fn mutate_emails(ui: &ProgramHandle<Msg>, result: Result<(), String>) {
    let loaded = result.and_then(|()| fetch_emails());
    let _ = ui.send(Msg::EmailsLoaded(loaded));
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
