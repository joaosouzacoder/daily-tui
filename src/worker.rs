//! Thread de fundo que roda os CLIs (himalaya/gcalcli/ghpending) sem bloquear
//! o loop principal nem o relógio.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ratatui_tea::ProgramHandle;

use crate::data::jira::JiraFilter;
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
    /// Busca as issues do Jira com o modo de filtro dado.
    FetchJira(JiraFilter),
    /// Busca as menções do Jira (visão própria, buscada sob demanda).
    FetchJiraMentions,
    /// Escritas no e-mail; após executar, re-busca a lista das duas contas.
    EmailSetSeen {
        account: Account,
        id: String,
        seen: bool,
    },
    EmailMove {
        account: Account,
        id: String,
        folder: String,
    },
    EmailDelete {
        account: Account,
        id: String,
    },
    /// Escrita no Microsoft To Do; após executar, re-busca a lista.
    TaskComplete(String),
    TaskReopen(String),
    TaskAdd(String),
    TaskEdit { id: String, title: String },
    TaskDelete(String),
    /// Marca ou desmarca uma subtarefa; re-busca a lista depois.
    SubTaskToggle {
        task_id: String,
        item_id: String,
        check: bool,
    },
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
        // O worker não guarda o resto do estado do app, mas precisa lembrar o
        // filtro de Jira ativo e se a visão de menções já foi aberta, para que
        // o refresh periódico (sem participação do App) refaça as mesmas
        // buscas de antes, não sempre o padrão.
        let mut jira_filter = JiraFilter::default();
        let mut mentions_loaded = false;
        refresh_all(&ui, jira_filter, mentions_loaded);

        loop {
            match rx.recv_timeout(refresh) {
                Ok(WorkerCmd::RefreshAll) | Err(RecvTimeoutError::Timeout) => {
                    refresh_all(&ui, jira_filter, mentions_loaded)
                }
                Ok(WorkerCmd::FetchJira(filter)) => {
                    jira_filter = filter;
                    let _ = ui.send(Msg::JiraLoaded(jira::fetch(filter)));
                }
                Ok(WorkerCmd::FetchJiraMentions) => {
                    mentions_loaded = true;
                    let _ = ui.send(Msg::JiraMentions(jira::fetch_mentions()));
                }
                Ok(WorkerCmd::ReadEmail { account, id }) => {
                    let _ = ui.send(Msg::EmailBody(email::fetch_body(account, &id)));
                }
                Ok(WorkerCmd::EmailSetSeen { account, id, seen }) => {
                    mutate_emails(&ui, email::set_seen(account, &id, seen))
                }
                Ok(WorkerCmd::EmailMove {
                    account,
                    id,
                    folder,
                }) => mutate_emails(&ui, email::move_to(account, &id, &folder)),
                Ok(WorkerCmd::EmailDelete { account, id }) => {
                    mutate_emails(&ui, email::delete(account, &id))
                }
                Ok(WorkerCmd::TaskComplete(id)) => mutate_tasks(&ui, tasks::complete(&id)),
                Ok(WorkerCmd::TaskReopen(id)) => mutate_tasks(&ui, tasks::reopen(&id)),
                Ok(WorkerCmd::TaskAdd(title)) => mutate_tasks(&ui, tasks::add(&title)),
                Ok(WorkerCmd::TaskEdit { id, title }) => {
                    mutate_tasks(&ui, tasks::edit(&id, &title))
                }
                Ok(WorkerCmd::TaskDelete(id)) => mutate_tasks(&ui, tasks::delete(&id)),
                Ok(WorkerCmd::SubTaskToggle {
                    task_id,
                    item_id,
                    check,
                }) => mutate_tasks(
                    &ui,
                    if check {
                        tasks::check(&task_id, &item_id)
                    } else {
                        tasks::uncheck(&task_id, &item_id)
                    },
                ),
                Ok(WorkerCmd::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    (tx, handle)
}

/// Busca os conjuntos de dados e manda cada resultado assim que fica pronto.
/// Menções só entram no refresh se `mentions_loaded` — não vale pagar a
/// consulta para quem nunca abriu a visão.
fn refresh_all(ui: &ProgramHandle<Msg>, jira_filter: JiraFilter, mentions_loaded: bool) {
    let _ = ui.send(Msg::EmailsLoaded(fetch_emails()));
    let _ = ui.send(Msg::AgendaLoaded(fetch_agenda()));
    let _ = ui.send(Msg::PullsLoaded(pulls::fetch()));
    let _ = ui.send(Msg::JiraLoaded(jira::fetch(jira_filter)));
    let _ = ui.send(Msg::TasksLoaded(tasks::fetch()));
    if mentions_loaded {
        let _ = ui.send(Msg::JiraMentions(jira::fetch_mentions()));
    }
}

/// Aplica uma escrita no Microsoft To Do e re-busca a lista. Se a escrita falhar,
/// propaga o erro para o painel; senão, manda a lista atualizada.
fn mutate_tasks(ui: &ProgramHandle<Msg>, result: Result<(), String>) {
    let loaded = result.and_then(|()| tasks::fetch());
    let _ = ui.send(Msg::TasksLoaded(loaded));
}

/// Aplica uma escrita no e-mail e re-busca a lista das duas contas — o painel
/// reflete o servidor, nunca um palpite local. Erro na escrita vai para o painel
/// e a lista fica como estava.
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
