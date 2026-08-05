//! Thread de fundo que roda os CLIs (himalaya/gcalcli/ghpending) sem bloquear
//! o loop principal nem o relógio.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ratatui_tea::ProgramHandle;

use crate::data::jira::JiraFilter;
use crate::data::{agenda, email, jira, pulls, tasks, Account};
use crate::msg::Msg;

/// Quantos e-mails buscar por conta.
const EMAIL_LIMIT: u32 = 50;

/// De quanto em quanto tempo relistar as pastas das contas.
///
/// Etiqueta nova é raro, e listar pastas é uma ida ao IMAP por conta — cadência
/// própria, mais folgada que o refresh dos painéis, para não cobrar do usuário
/// uma espera que ele não pediu.
const FOLDERS_TTL: Duration = Duration::from_secs(600);

/// Contas cujas pastas e e-mails são buscados.
const ACCOUNTS: [Account; 2] = [Account::Work, Account::Personal];

/// Comandos enviados do loop principal para o worker.
pub enum WorkerCmd {
    /// Recarrega e-mails, agenda, PRs e tarefas.
    RefreshAll,
    /// Busca as pastas de uma conta (uma vez por sessão; o App cacheia).
    FetchFolders(Account),
    /// Busca o corpo de um e-mail para o overlay de detalhe.
    ReadEmail { account: Account, id: String },
    /// Busca as issues do Jira com o modo de filtro dado.
    FetchJira(JiraFilter),
    /// Busca as menções do Jira (visão própria, buscada sob demanda).
    FetchJiraMentions,
    /// Escritas no e-mail; carregam a lista de alvos para a ação valer em lote
    /// com uma re-busca só, em vez de uma por e-mail. Após executar, re-busca as
    /// duas contas.
    EmailSetSeen {
        items: Vec<(Account, String)>,
        seen: bool,
    },
    EmailMove {
        items: Vec<(Account, String)>,
        folder: String,
    },
    EmailDelete {
        items: Vec<(Account, String)>,
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
        // Última listagem de pastas; `None` força a primeira no arranque, para o
        // seletor de "mover" já abrir preenchido.
        let mut folders_at: Option<Instant> = None;
        refresh_all(&ui, jira_filter, mentions_loaded);
        refresh_folders(&ui, &mut folders_at);

        loop {
            match rx.recv_timeout(refresh) {
                Ok(WorkerCmd::RefreshAll) | Err(RecvTimeoutError::Timeout) => {
                    refresh_all(&ui, jira_filter, mentions_loaded);
                    refresh_folders(&ui, &mut folders_at);
                }
                Ok(WorkerCmd::FetchJira(filter)) => {
                    jira_filter = filter;
                    let _ = ui.send(Msg::JiraLoaded(jira::fetch(filter)));
                }
                Ok(WorkerCmd::FetchJiraMentions) => {
                    mentions_loaded = true;
                    let _ = ui.send(Msg::JiraMentions(jira::fetch_mentions()));
                }
                Ok(WorkerCmd::FetchFolders(account)) => {
                    let _ = ui.send(Msg::FoldersLoaded(account, email::folders(account)));
                }
                Ok(WorkerCmd::ReadEmail { account, id }) => {
                    let body = email::fetch_body(account, &id);
                    let _ = ui.send(Msg::EmailBody(account, id, body));
                }
                Ok(WorkerCmd::EmailSetSeen { items, seen }) => {
                    let done = each(&items, |account, id| email::set_seen(account, id, seen));
                    mutate_emails(&ui, items, done);
                }
                Ok(WorkerCmd::EmailMove { items, folder }) => {
                    let done = each(&items, |account, id| email::move_to(account, id, &folder));
                    mutate_emails(&ui, items, done);
                }
                Ok(WorkerCmd::EmailDelete { items }) => {
                    let done = each(&items, email::delete);
                    mutate_emails(&ui, items, done);
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

/// Relista as pastas de todas as contas se o cache passou do TTL.
///
/// Vem depois dos painéis no arranque: nada na tela depende disso, então não
/// deve atrasar o que o usuário está olhando.
fn refresh_folders(ui: &ProgramHandle<Msg>, at: &mut Option<Instant>) {
    let stale = at.map(|t| t.elapsed() >= FOLDERS_TTL).unwrap_or(true);
    if !stale {
        return;
    }
    for account in ACCOUNTS {
        let _ = ui.send(Msg::FoldersLoaded(account, email::folders(account)));
    }
    *at = Some(Instant::now());
}

/// Aplica a escrita em cada alvo e devolve o primeiro erro, se houver.
///
/// Segue nos demais depois de uma falha: numa ação em lote, o usuário prefere
/// que os que dão para mover sejam movidos, e a re-busca mostra quem sobrou.
fn each<F>(items: &[(Account, String)], op: F) -> Result<(), String>
where
    F: Fn(Account, &str) -> Result<(), String>,
{
    let mut first_err = None;
    for (account, id) in items {
        if let Err(e) = op(*account, id) {
            first_err = first_err.or(Some(e));
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Aplica uma escrita no e-mail e re-busca a lista das duas contas — o painel
/// reflete o servidor, nunca um palpite local.
///
/// A re-busca acontece mesmo quando a escrita falha: é assim que a tela volta à
/// verdade em vez de manter para sempre a exclusão que o servidor recusou. O
/// erro viaja junto para o painel poder dizer o motivo.
fn mutate_emails(
    ui: &ProgramHandle<Msg>,
    targets: Vec<(Account, String)>,
    result: Result<(), String>,
) {
    let _ = ui.send(Msg::EmailWrite {
        targets,
        error: result.err(),
        list: fetch_emails(),
    });
}

/// Agrega e-mails das duas contas e ordena do mais recente para o mais antigo.
fn fetch_emails() -> Result<Vec<email::EmailItem>, String> {
    let mut all = email::fetch(ACCOUNTS[0], EMAIL_LIMIT)?;
    all.extend(email::fetch(ACCOUNTS[1], EMAIL_LIMIT)?);
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
