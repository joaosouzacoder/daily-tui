//! Estado da aplicação (Model) e o reducer `update`.

use std::cell::Cell;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_bubbletea_components::{Spinner, SpinnerFrames};
use ratatui_bubbletea_theme::BubbleTheme;
use ratatui_tea::{Cmd, Model};

use crate::data::jira::{JiraFilter, JiraItem, JiraView};
use crate::data::{notify, tasks};
use crate::data::{Account, AgendaItem, EmailItem, TaskItem};
use crate::msg::Msg;
use crate::store::Store;
use crate::ui;
use crate::worker::WorkerCmd;

/// Painel atualmente focado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Email,
    Jira,
    Agenda,
    Pulls,
    Tasks,
}

impl Panel {
    /// Próximo painel no ciclo (Tab). Segue a leitura do layout: coluna esquerda
    /// (E-mail → Jira), depois direita (Agenda → PRs → Tarefas).
    pub const fn next(self) -> Self {
        match self {
            Panel::Email => Panel::Jira,
            Panel::Jira => Panel::Agenda,
            Panel::Agenda => Panel::Pulls,
            Panel::Pulls => Panel::Tasks,
            Panel::Tasks => Panel::Email,
        }
    }

    /// Painel anterior no ciclo (Shift+Tab).
    pub const fn prev(self) -> Self {
        match self {
            Panel::Email => Panel::Tasks,
            Panel::Jira => Panel::Email,
            Panel::Agenda => Panel::Jira,
            Panel::Pulls => Panel::Agenda,
            Panel::Tasks => Panel::Pulls,
        }
    }
}

/// Estado de um painel com lista carregável.
pub struct PanelData<T> {
    pub items: Vec<T>,
    pub error: Option<String>,
    pub loaded: bool,
    /// Item selecionado (usado no painel de e-mails).
    pub cursor: usize,
    /// Deslocamento de rolagem ao seguir o cursor; ajustado na renderização.
    pub offset: Cell<usize>,
    /// Deslocamento de rolagem livre (painéis sem seleção: agenda/PRs).
    /// Clampado ao máximo na renderização (que conhece a altura).
    pub scroll: Cell<usize>,
}

impl<T> PanelData<T> {
    fn new() -> Self {
        Self {
            items: Vec::new(),
            error: None,
            loaded: false,
            cursor: 0,
            offset: Cell::new(0),
            scroll: Cell::new(0),
        }
    }

    /// Rola por `delta` linhas (limite inferior 0; o superior é aplicado na
    /// renderização, que reescreve o valor já clampado).
    fn scroll_by(&self, delta: isize) {
        let v = (self.scroll.get() as isize + delta).max(0) as usize;
        self.scroll.set(v);
    }

    /// Aplica o resultado de uma busca, preservando o cursor dentro dos limites.
    fn set(&mut self, result: Result<Vec<T>, String>) {
        match result {
            Ok(items) => {
                self.items = items;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        self.loaded = true;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        if self.cursor >= self.items.len() {
            self.cursor = self.items.len().saturating_sub(1);
        }
    }

    /// Move o cursor por `delta`, mantendo-o dentro dos limites.
    fn move_cursor(&mut self, delta: isize) {
        if self.items.is_empty() {
            self.cursor = 0;
            return;
        }
        let max = (self.items.len() - 1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
    }

    fn to_first(&mut self) {
        self.cursor = 0;
    }

    fn to_last(&mut self) {
        self.cursor = self.items.len().saturating_sub(1);
    }
}

/// Overlay da central de notificações. A lista em si é derivada das fontes
/// carregadas (`notification_items`), então aqui só vive a seleção.
pub struct NotificationsView {
    pub cursor: usize,
}

/// Overlay de detalhe de um e-mail.
pub struct Detail {
    pub from: String,
    pub subject: String,
    /// `None` enquanto carrega; depois o corpo ou um erro.
    pub body: Option<Result<String, String>>,
    pub scroll: usize,
}

/// O que um prompt de texto está coletando.
pub enum InputKind {
    /// Criar uma nova tarefa.
    AddTask,
    /// Editar o título da tarefa com este id.
    EditTask { id: String },
    /// Criar uma subtarefa na tarefa com este id.
    AddSubtask { task_id: String },
}

/// Overlay modal de interação com tarefas (entrada de texto ou confirmação).
pub enum Prompt {
    /// Campo de texto (criar/editar tarefa).
    Input { kind: InputKind, buffer: String },
    /// Confirmação de exclusão da tarefa selecionada.
    ConfirmDelete { id: String, title: String },
    /// Escolha de pasta para mover os e-mails alvo (marcados, ou o do cursor).
    ///
    /// A lista traz as pastas de **todas** as contas presentes nos alvos, cada
    /// uma marcada com a conta — no Gmail, isso inclui as etiquetas. Mover só
    /// alcança os alvos da conta da pasta escolhida: uma etiqueta do work não
    /// existe na conta pessoal.
    PickFolder {
        items: Vec<(Account, String)>,
        folders: Vec<(Account, String)>,
        cursor: usize,
    },
    /// Confirmação de exclusão dos e-mails alvo (move para a Lixeira).
    ConfirmEmailDelete {
        items: Vec<(Account, String)>,
        /// O que mostrar na pergunta: o assunto, ou a contagem no lote.
        what: String,
    },
}

/// Identidade de um e-mail. O id do himalaya é a UID da pasta, única só dentro
/// da conta — as duas contas repetem números, então a conta faz parte da chave.
pub type EmailKey = (Account, String);

/// Escritas de e-mail já aplicadas na tela e ainda não confirmadas pelo servidor.
///
/// O worker é sequencial e cada escrita termina com uma re-busca, então uma
/// exclusão pedida enquanto outra escrita roda espera na fila. Sem registro do
/// que está pendente, a lista que chega no meio do caminho é a do servidor
/// *antes* da exclusão e o e-mail excluído reaparece — foi o que acontecia ao
/// marcar como lido e excluir em seguida. Aqui a intenção sobrevive a essas
/// listas até o servidor responder sobre ela.
#[derive(Default)]
pub struct EmailPending {
    /// Alvos de exclusão/mudança de pasta que já saíram da tela.
    removed: std::collections::HashSet<EmailKey>,
    /// Alvos de marcar/desmarcar como lido, com o estado pedido.
    seen: std::collections::HashMap<EmailKey, bool>,
}

impl EmailPending {
    /// Reaplica as escritas pendentes sobre uma lista vinda do servidor.
    fn apply(&self, items: &mut Vec<EmailItem>) {
        if self.removed.is_empty() && self.seen.is_empty() {
            return;
        }
        items.retain(|e| !self.removed.contains(&(e.account, e.id.clone())));
        for e in items.iter_mut() {
            if let Some(seen) = self.seen.get(&(e.account, e.id.clone())) {
                e.unread = !seen;
            }
        }
    }

    /// Encerra a pendência dos alvos: o servidor já disse o que aconteceu com
    /// eles, e a lista que vem com essa resposta passa a ser a verdade.
    fn settle(&mut self, targets: &[EmailKey]) {
        for key in targets {
            self.removed.remove(key);
            self.seen.remove(key);
        }
    }
}

/// Modelo principal da aplicação.
pub struct App {
    pub theme: BubbleTheme,
    pub should_quit: bool,
    pub now: DateTime<Local>,
    pub focus: Panel,
    pub emails: PanelData<EmailItem>,
    pub jira: PanelData<JiraItem>,
    /// Modo de filtro do painel de Jira, circulado pela tecla `f`.
    pub jira_filter: JiraFilter,
    /// Ids das tarefas com as subtarefas expandidas no painel.
    pub tasks_expanded: std::collections::HashSet<String>,
    /// E-mails marcados para ação em lote (`Shift`+setas marca).
    pub emails_marked: std::collections::HashSet<EmailKey>,
    /// Escritas de e-mail ainda não confirmadas pelo servidor.
    emails_pending: EmailPending,
    /// Pastas por conta, buscadas uma vez por sessão para o seletor de "mover".
    pub folders: std::collections::HashMap<Account, Vec<String>>,
    /// Corpos já buscados, por (conta, id). O corpo do e-mail sob o cursor é
    /// buscado em segundo plano, então abrir com `Enter` costuma ser instantâneo.
    pub bodies: std::collections::HashMap<(Account, String), String>,
    /// Corpos pedidos e ainda não respondidos, para não pedir duas vezes.
    pending_bodies: std::collections::HashSet<(Account, String)>,
    /// Visão ativa do painel de Jira (issues/por-pai/menções), circulada por
    /// `p`/`n`/`Esc`.
    pub jira_view: JiraView,
    /// Issues onde fui mencionado; painel próprio, buscado só quando a visão
    /// de menções é aberta pela primeira vez.
    pub jira_mentions: PanelData<JiraItem>,
    pub agenda: PanelData<AgendaItem>,
    pub pulls: PanelData<String>,
    pub tasks: PanelData<TaskItem>,
    pub spinner: Spinner,
    pub last_refresh: Option<DateTime<Local>>,
    pub detail: Option<Detail>,
    pub prompt: Option<Prompt>,
    /// Central de notificações aberta (overlay global, tecla `n`).
    pub notifications: Option<NotificationsView>,
    /// Notificações que você já leu. Vem do banco no arranque e cresce com
    /// `Espaço` no overlay; usada para filtrar a lista.
    notifications_read: std::collections::HashSet<String>,
    /// Banco local (notificações lidas, cache de pastas). `None` quando não
    /// abriu — o painel funciona sem ele, só sem memória entre execuções.
    store: Option<Store>,
    /// Por que o banco não abriu, mostrado no overlay de notificações.
    pub store_error: Option<String>,
    cmd_tx: Sender<WorkerCmd>,
}

impl App {
    /// Cria o app. `cmd_tx` envia comandos para a thread do worker.
    pub fn new(theme: BubbleTheme, cmd_tx: Sender<WorkerCmd>) -> Self {
        Self {
            theme,
            should_quit: false,
            now: Local::now(),
            focus: Panel::Email,
            emails: PanelData::new(),
            jira: PanelData::new(),
            jira_filter: JiraFilter::default(),
            jira_view: JiraView::default(),
            tasks_expanded: std::collections::HashSet::new(),
            emails_marked: std::collections::HashSet::new(),
            emails_pending: EmailPending::default(),
            folders: std::collections::HashMap::new(),
            bodies: std::collections::HashMap::new(),
            pending_bodies: std::collections::HashSet::new(),
            jira_mentions: PanelData::new(),
            agenda: PanelData::new(),
            pulls: PanelData::new(),
            tasks: PanelData::new(),
            spinner: Spinner::new()
                .frames(SpinnerFrames::DOTS)
                .label("carregando"),
            last_refresh: None,
            detail: None,
            prompt: None,
            notifications: None,
            notifications_read: std::collections::HashSet::new(),
            store: None,
            store_error: None,
            cmd_tx,
        }
    }

    /// Rola/move o painel focado. E-mail move a seleção; agenda/PRs rolam livre.
    fn focused_scroll(&mut self, delta: isize) {
        match self.focus {
            Panel::Email => self.emails.move_cursor(delta),
            Panel::Jira => self.jira.move_cursor(delta),
            Panel::Agenda => self.agenda.scroll_by(delta),
            Panel::Pulls => self.pulls.scroll_by(delta),
            // Tarefas é o único painel onde o cursor indexa **linhas**, não
            // itens: com subtarefas expandidas há mais linhas que tarefas, e
            // `move_cursor` limitaria ao número de tarefas — deixando as
            // subtarefas do fim inalcançáveis.
            Panel::Tasks => self.move_task_cursor(delta),
        }
    }

    /// Move o cursor do painel de Tarefas sobre as linhas renderizadas.
    fn move_task_cursor(&mut self, delta: isize) {
        let total = tasks::rows(&self.tasks.items, &self.tasks_expanded).len();
        if total == 0 {
            self.tasks.cursor = 0;
            return;
        }
        let max = (total - 1) as isize;
        self.tasks.cursor = (self.tasks.cursor as isize + delta).clamp(0, max) as usize;
    }

    fn focused_to_first(&mut self) {
        match self.focus {
            Panel::Email => self.emails.to_first(),
            Panel::Jira => self.jira.to_first(),
            Panel::Agenda => self.agenda.scroll.set(0),
            Panel::Pulls => self.pulls.scroll.set(0),
            Panel::Tasks => self.tasks.to_first(),
        }
    }

    fn focused_to_last(&mut self) {
        // Para listas roláveis, o valor grande é clampado ao máximo na render.
        match self.focus {
            Panel::Email => self.emails.to_last(),
            Panel::Jira => self.jira.to_last(),
            Panel::Agenda => self.agenda.scroll.set(usize::MAX),
            Panel::Pulls => self.pulls.scroll.set(usize::MAX),
            // Idem: o fim da lista de Tarefas é a última linha, não a última tarefa.
            Panel::Tasks => {
                let total = tasks::rows(&self.tasks.items, &self.tasks_expanded).len();
                self.tasks.cursor = total.saturating_sub(1);
            }
        }
    }

    fn open_detail(&mut self) {
        if self.focus != Panel::Email {
            return;
        }
        let Some(item) = self.emails.items.get(self.emails.cursor) else {
            return;
        };
        let key = (item.account, item.id.clone());
        self.detail = Some(Detail {
            from: item.from.clone(),
            subject: item.subject.clone(),
            // Se o prefetch já trouxe, abre preenchido em vez de "carregando".
            body: self.bodies.get(&key).cloned().map(Ok),
            scroll: 0,
        });
        if self.detail.as_ref().is_some_and(|d| d.body.is_none()) {
            self.request_body(key);
        }
    }

    /// Pede o corpo ao worker, a menos que já esteja em cache ou a caminho.
    fn request_body(&mut self, key: (Account, String)) {
        if self.bodies.contains_key(&key) || self.pending_bodies.contains(&key) {
            return;
        }
        self.pending_bodies.insert(key.clone());
        let _ = self.cmd_tx.send(WorkerCmd::ReadEmail {
            account: key.0,
            id: key.1,
        });
    }

    /// Busca em segundo plano o corpo do e-mail sob o cursor.
    ///
    /// Chamado no tique de 1s em vez de a cada movimento do cursor: rolar a lista
    /// depressa não deve enfileirar uma ida ao IMAP por linha, e um segundo de
    /// pausa é bom sinal de que é esse o e-mail que interessa.
    fn prefetch_cursor_body(&mut self) {
        if self.focus != Panel::Email {
            return;
        }
        let Some(item) = self.emails.items.get(self.emails.cursor) else {
            return;
        };
        self.request_body((item.account, item.id.clone()));
    }

    /// Abre no navegador a issue sob o cursor. O erro vai para o painel, como
    /// qualquer outra falha de busca. Na visão de menções, a issue e o cursor
    /// vêm de `jira_mentions`, não de `jira` — são conjuntos de dados diferentes.
    fn open_selected_issue(&mut self) {
        let Some(url) = self.jira.items.get(self.jira.cursor).map(|i| i.url.clone()) else {
            return;
        };
        if let Err(e) = crate::data::jira::open_url(&url) {
            self.jira.error = Some(e);
        }
    }

    /// Alterna lido/não lido do e-mail sob o cursor.
    ///
    /// Aplica o efeito na tela na hora e só depois manda a escrita: reler as duas
    /// contas por IMAP leva segundos, e esperar por isso faz a tecla parecer
    /// travada. A re-busca que vem depois reconcilia — e se a escrita falhar, o
    /// erro aparece no painel e a lista volta ao que o servidor diz.
    fn toggle_email_seen(&mut self) {
        let items = self.email_targets();
        if items.is_empty() {
            return;
        }
        // No lote com estados mistos, "marcar como lido" é o que se espera:
        // basta um não lido para a ação virar marcar todos.
        let keys: std::collections::HashSet<EmailKey> = items.iter().cloned().collect();
        let seen = self
            .emails
            .items
            .iter()
            .any(|e| keys.contains(&(e.account, e.id.clone())) && e.unread);
        for e in self
            .emails
            .items
            .iter_mut()
            .filter(|e| keys.contains(&(e.account, e.id.clone())))
        {
            e.unread = !seen;
        }
        for key in keys {
            self.emails_pending.seen.insert(key, seen);
        }
        let _ = self.cmd_tx.send(WorkerCmd::EmailSetSeen { items, seen });
    }

    /// Liga o banco ao app, já carregando o que ele guardou.
    ///
    /// Fica fora do `new` para os testes não tocarem no banco de verdade, e
    /// porque abrir um arquivo pode falhar — e falhar aqui não pode derrubar o
    /// painel: sem banco, o programa roda sem memória entre execuções.
    pub fn attach_store(&mut self, opened: Result<Store, String>) {
        match opened {
            Ok(store) => {
                match store.read_notifications() {
                    Ok(read) => self.notifications_read = read,
                    Err(e) => self.store_error = Some(e),
                }
                // Pastas em cache já valem para o seletor de "mover" abrir cheio
                // na primeira vez; o worker relista em segundo plano e corrige.
                match store.folders() {
                    Ok(folders) => self.folders.extend(folders),
                    Err(e) => self.store_error = Some(e),
                }
                self.store = Some(store);
            }
            Err(e) => self.store_error = Some(e),
        }
    }

    /// Marca como lida a notificação sob o cursor: sai da lista e não volta.
    fn read_notification(&mut self) {
        let Some(view) = &self.notifications else {
            return;
        };
        let Some(note) = self.notification_items().into_iter().nth(view.cursor) else {
            return;
        };
        self.notifications_read.insert(note.id.clone());
        if let Some(store) = &self.store
            && let Err(e) = store.mark_notification_read(&note.id, &self.now.to_rfc3339())
        {
            self.store_error = Some(e);
        }
        // A lista encurtou: o cursor não pode ficar depois do fim dela.
        let total = self.notification_items().len();
        if let Some(view) = &mut self.notifications {
            view.cursor = view.cursor.min(total.saturating_sub(1));
        }
    }

    /// Abre a central de notificações. Busca as fontes que ainda não carregaram —
    /// hoje só o Jira — para não pagar a consulta de quem nunca abre o overlay.
    fn open_notifications(&mut self) {
        self.notifications = Some(NotificationsView { cursor: 0 });
        if !self.jira_mentions.loaded {
            let _ = self.cmd_tx.send(WorkerCmd::FetchJiraMentions);
        }
    }

    /// Notificações de todas as fontes carregadas, na ordem em que aparecem.
    pub fn notification_items(&self) -> Vec<notify::Notification> {
        let mut items = notify::from_jira_mentions(&self.jira_mentions.items);
        items.retain(|n| !self.notifications_read.contains(&n.id));
        items
    }

    /// Trata teclas com a central de notificações aberta.
    fn handle_notifications_key(&mut self, key: KeyEvent) {
        let total = self.notification_items().len();
        let Some(view) = &mut self.notifications else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('n') => self.notifications = None,
            KeyCode::Char('j') | KeyCode::Down => {
                if total > 0 {
                    view.cursor = (view.cursor + 1).min(total - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => view.cursor = view.cursor.saturating_sub(1),
            KeyCode::Char(' ') => self.read_notification(),
            KeyCode::Enter => {
                let cursor = view.cursor;
                let url = self
                    .notification_items()
                    .get(cursor)
                    .map(|n| n.url.clone())
                    .filter(|u| !u.is_empty());
                if let Some(Err(e)) = url.map(|u| crate::data::jira::open_url(&u)) {
                    self.jira_mentions.error = Some(e);
                }
            }
            _ => {}
        }
    }

    /// Alterna a marcação do e-mail sob o cursor.
    ///
    /// O `Shift`+setas só marca — precisa existir uma forma de desmarcar um item
    /// sem limpar a faixa inteira, e é esta.
    fn toggle_mark(&mut self) {
        let Some(item) = self.emails.items.get(self.emails.cursor) else {
            return;
        };
        let key = (item.account, item.id.clone());
        if !self.emails_marked.remove(&key) {
            self.emails_marked.insert(key);
        }
    }

    /// Estende a marcação em faixa: marca o e-mail sob o cursor, move, e marca o
    /// novo. Assim segurar `Shift` e andar deixa marcado tudo por onde passou.
    fn extend_mark(&mut self, delta: isize) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.emails_marked.insert((item.account, item.id.clone()));
        }
        self.emails.move_cursor(delta);
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.emails_marked.insert((item.account, item.id.clone()));
        }
    }

    /// Alvos de uma ação de e-mail: os marcados, ou o que está sob o cursor.
    ///
    /// Preserva a ordem da lista exibida, para o lote ser previsível.
    fn email_targets(&self) -> Vec<(Account, String)> {
        if self.emails_marked.is_empty() {
            return self
                .emails
                .items
                .get(self.emails.cursor)
                .map(|e| vec![(e.account, e.id.clone())])
                .unwrap_or_default();
        }
        self.emails
            .items
            .iter()
            .filter(|e| self.emails_marked.contains(&(e.account, e.id.clone())))
            .map(|e| (e.account, e.id.clone()))
            .collect()
    }

    /// Troca a lista de e-mails por uma vinda do servidor, sem perder as
    /// escritas que ainda estão na fila do worker.
    fn set_emails(&mut self, mut res: Result<Vec<EmailItem>, String>) {
        if let Ok(items) = &mut res {
            self.emails_pending.apply(items);
        }
        self.emails.set(res);
    }

    /// Remove os e-mails da lista exibida e limpa a marcação, mantendo o cursor
    /// dentro dos limites. A re-busca que vem depois é quem diz a verdade.
    fn drop_emails(&mut self, items: &[EmailKey]) {
        for key in items {
            self.emails_pending.removed.insert(key.clone());
        }
        self.emails
            .items
            .retain(|e| !items.contains(&(e.account, e.id.clone())));
        self.emails_marked.clear();
        self.emails.clamp_cursor();
    }

    /// Abre o seletor de pasta para os alvos (marcados, ou o do cursor).
    fn open_move_email(&mut self) {
        let items = self.email_targets();
        if items.is_empty() {
            return;
        }
        // Pede ao worker o que ainda não está em cache; o prefetch do arranque
        // normalmente já resolveu, isto é a rede de segurança.
        for account in self.target_accounts(&items) {
            if !self.folders.contains_key(&account) {
                let _ = self.cmd_tx.send(WorkerCmd::FetchFolders(account));
            }
        }
        let folders = self.folder_entries(&items);
        self.prompt = Some(Prompt::PickFolder {
            items,
            folders,
            cursor: 0,
        });
    }

    /// Contas presentes nos alvos, na ordem em que aparecem.
    fn target_accounts(&self, items: &[(Account, String)]) -> Vec<Account> {
        let mut out: Vec<Account> = Vec::new();
        for (account, _) in items {
            if !out.contains(account) {
                out.push(*account);
            }
        }
        out
    }

    /// Pastas de todas as contas dos alvos, cada uma com a sua conta.
    fn folder_entries(&self, items: &[(Account, String)]) -> Vec<(Account, String)> {
        self.target_accounts(items)
            .into_iter()
            .flat_map(|account| {
                self.folders
                    .get(&account)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |name| (account, name))
            })
            .collect()
    }

    /// Pede confirmação antes de mover os alvos para a Lixeira.
    fn open_delete_email(&mut self) {
        let items = self.email_targets();
        if items.is_empty() {
            return;
        }
        let what = if items.len() == 1 {
            self.emails
                .items
                .get(self.emails.cursor)
                .map(|e| e.subject.clone())
                .unwrap_or_default()
        } else {
            format!("{} e-mails marcados", items.len())
        };
        self.prompt = Some(Prompt::ConfirmEmailDelete { items, what });
    }

    /// Trata teclas quando o overlay de detalhe está aberto.
    fn handle_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter | KeyCode::Backspace => {
                self.detail = None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(d) = &mut self.detail {
                    d.scroll = d.scroll.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(d) = &mut self.detail {
                    d.scroll = d.scroll.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    /// Tarefa a que a linha sob o cursor pertence.
    ///
    /// O cursor anda por *linhas*, e com uma tarefa expandida as subtarefas
    /// entram na contagem — indexar `items` pelo cursor apontava para outra
    /// tarefa. `Sub` devolve a tarefa mãe: criar subtarefa a partir de uma irmã
    /// é o que se espera.
    fn cursor_task(&self) -> Option<&TaskItem> {
        match self.selected_row()? {
            tasks::TaskRow::Task(t) | tasks::TaskRow::Sub { task: t, .. } => {
                self.tasks.items.get(t)
            }
        }
    }

    /// Tarefa sob o cursor, e só quando a linha é a dela — `None` numa linha de
    /// subtarefa. Editar e apagar valem para a tarefa; a subtarefa não tem essas
    /// ações, e agir na mãe sem o usuário pedir apagaria a tarefa inteira.
    fn selected_task(&self) -> Option<&TaskItem> {
        match self.selected_row()? {
            tasks::TaskRow::Task(t) => self.tasks.items.get(t),
            tasks::TaskRow::Sub { .. } => None,
        }
    }

    /// Alterna a tarefa selecionada entre concluída e pendente.
    /// Linha sob o cursor no painel de Tarefas: uma tarefa ou uma subtarefa.
    fn selected_row(&self) -> Option<tasks::TaskRow> {
        tasks::rows(&self.tasks.items, &self.tasks_expanded)
            .get(self.tasks.cursor)
            .cloned()
    }

    /// Alterna o estado da linha sob o cursor — tarefa ou subtarefa.
    fn toggle_task(&mut self) {
        // Como no e-mail: vira o estado na tela agora e deixa a re-busca
        // reconciliar, senão a tecla parece travada enquanto o Graph responde.
        let cmd = match self.selected_row() {
            Some(tasks::TaskRow::Task(t)) => {
                let Some(item) = self.tasks.items.get_mut(t) else {
                    return;
                };
                let was_done = item.completed;
                item.completed = !was_done;
                if was_done {
                    WorkerCmd::TaskReopen(item.id.clone())
                } else {
                    WorkerCmd::TaskComplete(item.id.clone())
                }
            }
            Some(tasks::TaskRow::Sub { task, sub }) => {
                let Some(item) = self.tasks.items.get_mut(task) else {
                    return;
                };
                let task_id = item.id.clone();
                let Some(s) = item.subtasks.get_mut(sub) else {
                    return;
                };
                let check = !s.completed;
                s.completed = check;
                WorkerCmd::SubTaskToggle {
                    task_id,
                    item_id: s.id.clone(),
                    check,
                }
            }
            None => return,
        };
        let _ = self.cmd_tx.send(cmd);
    }

    /// Expande ou recolhe as subtarefas da tarefa sob o cursor.
    ///
    /// Reancora o cursor na própria tarefa depois: recolher encurta a lista de
    /// linhas, e manter o índice antigo apontaria para outra coisa.
    fn toggle_expand(&mut self) {
        let t = match self.selected_row() {
            Some(tasks::TaskRow::Task(t)) => t,
            Some(tasks::TaskRow::Sub { task, .. }) => task,
            None => return,
        };
        let Some(item) = self.tasks.items.get(t) else {
            return;
        };
        if item.subtasks.is_empty() {
            return; // nada a expandir; não pisca nem abre linha vazia
        }
        let id = item.id.clone();
        if !self.tasks_expanded.remove(&id) {
            self.tasks_expanded.insert(id);
        }
        let rows = tasks::rows(&self.tasks.items, &self.tasks_expanded);
        if let Some(pos) = rows.iter().position(|r| *r == tasks::TaskRow::Task(t)) {
            self.tasks.cursor = pos;
        }
    }

    /// Abre o prompt de criação de tarefa.
    fn open_add_task(&mut self) {
        self.prompt = Some(Prompt::Input {
            kind: InputKind::AddTask,
            buffer: String::new(),
        });
    }

    /// Abre o prompt de criação de subtarefa na tarefa da linha sob o cursor.
    fn open_add_subtask(&mut self) {
        if let Some(t) = self.cursor_task() {
            self.prompt = Some(Prompt::Input {
                kind: InputKind::AddSubtask {
                    task_id: t.id.clone(),
                },
                buffer: String::new(),
            });
        }
    }

    /// Abre o prompt de edição com o título atual da tarefa selecionada.
    fn open_edit_task(&mut self) {
        if let Some(t) = self.selected_task() {
            self.prompt = Some(Prompt::Input {
                kind: InputKind::EditTask { id: t.id.clone() },
                buffer: t.title.clone(),
            });
        }
    }

    /// Abre a confirmação de exclusão da tarefa selecionada.
    fn open_delete_task(&mut self) {
        if let Some(t) = self.selected_task() {
            self.prompt = Some(Prompt::ConfirmDelete {
                id: t.id.clone(),
                title: t.title.clone(),
            });
        }
    }

    /// Trata teclas quando um prompt de tarefa está aberto.
    fn handle_prompt_key(&mut self, key: KeyEvent) {
        match &mut self.prompt {
            Some(Prompt::Input { buffer, .. }) => match key.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Char(c) => buffer.push(c),
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Enter => self.submit_prompt(),
                _ => {}
            },
            Some(Prompt::ConfirmDelete { .. }) | Some(Prompt::ConfirmEmailDelete { .. }) => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => self.submit_prompt(),
                    KeyCode::Char('n') | KeyCode::Esc => self.prompt = None,
                    _ => {}
                }
            }
            Some(Prompt::PickFolder {
                folders, cursor, ..
            }) => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    *cursor = (*cursor + 1).min(folders.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *cursor = cursor.saturating_sub(1);
                }
                KeyCode::Enter => self.submit_prompt(),
                KeyCode::Esc => self.prompt = None,
                _ => {}
            },
            None => {}
        }
    }

    /// Confirma o prompt atual: dispara o comando ao worker e fecha o overlay.
    fn submit_prompt(&mut self) {
        let cmd = match self.prompt.take() {
            Some(Prompt::Input { kind, buffer }) => {
                let title = buffer.trim().to_string();
                if title.is_empty() {
                    return; // nada a fazer; prompt já foi fechado
                }
                match kind {
                    InputKind::AddTask => WorkerCmd::TaskAdd(title),
                    InputKind::EditTask { id } => WorkerCmd::TaskEdit { id, title },
                    InputKind::AddSubtask { task_id } => {
                        // Expande a mãe: a subtarefa nova chega com a re-busca e
                        // ficaria escondida numa tarefa recolhida.
                        self.tasks_expanded.insert(task_id.clone());
                        WorkerCmd::SubTaskAdd { task_id, title }
                    }
                }
            }
            Some(Prompt::ConfirmDelete { id, .. }) => WorkerCmd::TaskDelete(id),
            Some(Prompt::PickFolder {
                items,
                folders,
                cursor,
            }) => {
                let Some((account, folder)) = folders.get(cursor).cloned() else {
                    return; // lista ainda vazia: nada a mover
                };
                // Só os alvos da conta da pasta: o resto do lote fica onde está.
                let items: Vec<(Account, String)> =
                    items.into_iter().filter(|(a, _)| *a == account).collect();
                // Saem da lista na hora: deixaram a pasta atual, e esperar o IMAP
                // para refletir isso faz a ação parecer ignorada.
                self.drop_emails(&items);
                WorkerCmd::EmailMove { items, folder }
            }
            Some(Prompt::ConfirmEmailDelete { items, .. }) => {
                self.drop_emails(&items);
                WorkerCmd::EmailDelete { items }
            }
            None => return,
        };
        let _ = self.cmd_tx.send(cmd);
    }

    /// Trata teclas no modo painel (dashboard).
    fn handle_panel_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
            // `Shift`+setas marca em faixa no painel de e-mails. Não existe
            // `Shift+j`: o terminal entrega isso como `J`, sem modificador —
            // com as setas o modificador chega de verdade.
            KeyCode::Down if shift && self.focus == Panel::Email => self.extend_mark(1),
            KeyCode::Up if shift && self.focus == Panel::Email => self.extend_mark(-1),
            KeyCode::Char('j') | KeyCode::Down => self.focused_scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.focused_scroll(-1),
            KeyCode::Char('g') | KeyCode::Home => self.focused_to_first(),
            KeyCode::Char('G') | KeyCode::End => self.focused_to_last(),
            KeyCode::Enter => match self.focus {
                Panel::Email => self.open_detail(),
                Panel::Jira => self.open_selected_issue(),
                Panel::Tasks => self.toggle_expand(),
                _ => {}
            },
            KeyCode::Char('r') => {
                let _ = self.cmd_tx.send(WorkerCmd::RefreshAll);
            }
            // Ações do painel de tarefas (só quando ele está focado).
            KeyCode::Char('x') if self.focus == Panel::Email => self.toggle_mark(),
            KeyCode::Char(' ') if self.focus == Panel::Email => self.toggle_email_seen(),
            KeyCode::Char('m') if self.focus == Panel::Email => self.open_move_email(),
            KeyCode::Char('d') if self.focus == Panel::Email => self.open_delete_email(),
            KeyCode::Char(' ') if self.focus == Panel::Tasks => self.toggle_task(),
            KeyCode::Char('a') if self.focus == Panel::Tasks => self.open_add_task(),
            KeyCode::Char('A') if self.focus == Panel::Tasks => self.open_add_subtask(),
            KeyCode::Char('e') if self.focus == Panel::Tasks => self.open_edit_task(),
            KeyCode::Char('d') if self.focus == Panel::Tasks => self.open_delete_task(),
            // Ação do painel de Jira: circula o filtro e recarrega.
            KeyCode::Char('f') if self.focus == Panel::Jira => {
                self.jira_filter = self.jira_filter.next();
                let _ = self.cmd_tx.send(WorkerCmd::FetchJira(self.jira_filter));
            }
            // Visões do painel de Jira: issues (padrão) / por pai / menções.
            KeyCode::Char('p') if self.focus == Panel::Jira => {
                self.jira_view = JiraView::ByParent;
                self.jira.cursor = 0;
            }
            // `n` é global: notificação é coisa que se quer ver de onde você
            // estiver, sem precisar focar o painel primeiro. Leva o foco para o
            // Jira junto, senão as teclas de navegação agiriam no painel errado.
            // A central de notificações é um overlay global: se abre de qualquer
            // painel, e vai receber outras fontes além do Jira.
            KeyCode::Char('n') => self.open_notifications(),
            // No e-mail, `Esc` desfaz a marcação — é a saída do modo lote.
            KeyCode::Esc if self.focus == Panel::Email => self.emails_marked.clear(),
            KeyCode::Esc if self.focus == Panel::Jira => {
                self.jira_view = JiraView::Issues;
            }
            _ => {}
        }
    }
}

impl Model for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg) -> Cmd<Msg> {
        match msg {
            Msg::ClockTick => {
                self.prefetch_cursor_body();
                self.now = Local::now();
                self.spinner.tick();
            }
            Msg::Key(key) => {
                if self.detail.is_some() {
                    self.handle_detail_key(key);
                } else if self.prompt.is_some() {
                    self.handle_prompt_key(key);
                } else if self.notifications.is_some() {
                    self.handle_notifications_key(key);
                } else {
                    self.handle_panel_key(key);
                }
            }
            Msg::EmailsLoaded(res) => {
                self.set_emails(res);
                self.last_refresh = Some(Local::now());
            }
            Msg::EmailWrite {
                targets,
                error,
                list,
            } => {
                // A resposta chegou sobre estes alvos: a lista que vem com ela
                // já os reflete, então a intenção local sai de cena aqui.
                self.emails_pending.settle(&targets);
                self.set_emails(list);
                if let Some(e) = error {
                    // Escrita falhou: os alvos voltam do servidor de propósito —
                    // some da tela o que não aconteceu de verdade — e o motivo
                    // aparece no painel em vez de sumir junto.
                    self.emails.error = Some(e);
                }
                self.last_refresh = Some(Local::now());
            }
            Msg::AgendaLoaded(res) => self.agenda.set(res),
            Msg::PullsLoaded(res) => self.pulls.set(res),
            Msg::JiraLoaded(res) => self.jira.set(res),
            Msg::JiraMentions(res) => self.jira_mentions.set(res),
            Msg::TasksLoaded(res) => self.tasks.set(res),
            Msg::FoldersLoaded(account, res) => {
                match res {
                    Ok(names) => {
                        if let Some(store) = &self.store
                            && let Err(e) =
                                store.save_folders(account, &names, &self.now.to_rfc3339())
                        {
                            self.store_error = Some(e);
                        }
                        self.folders.insert(account, names);
                        // Reconstrói a lista do prompt aberto, para o seletor sair
                        // do "buscando…" sem o usuário reabrir.
                        if let Some(Prompt::PickFolder { items, .. }) = &self.prompt {
                            let rebuilt = self.folder_entries(&items.clone());
                            if let Some(Prompt::PickFolder { folders, .. }) = &mut self.prompt {
                                *folders = rebuilt;
                            }
                        }
                    }
                    Err(e) => self.emails.error = Some(e),
                }
            }
            Msg::EmailBody(account, id, res) => {
                let key = (account, id);
                self.pending_bodies.remove(&key);
                if let Ok(body) = &res {
                    self.bodies.insert(key.clone(), body.clone());
                }
                // Só preenche o overlay se ainda for o e-mail aberto.
                let open_is_this = self
                    .emails
                    .items
                    .get(self.emails.cursor)
                    .is_some_and(|e| (e.account, e.id.clone()) == key);
                if let (true, Some(detail)) = (open_is_this, &mut self.detail) {
                    detail.body = Some(res);
                }
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame<'_>) {
        ui::render(self, frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Account;
    use std::sync::mpsc;

    fn test_app() -> App {
        let (tx, _rx) = mpsc::channel();
        App::new(BubbleTheme::default(), tx)
    }

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    fn email(id: &str) -> EmailItem {
        EmailItem {
            id: id.into(),
            account: Account::Work,
            from: "x".into(),
            subject: format!("s{id}"),
            unread: true,
            date: "2026-06-09 10:00+00:00".into(),
        }
    }

    #[test]
    fn tab_cycles_focus_forward_and_back() {
        let mut app = test_app();
        assert_eq!(app.focus, Panel::Email);
        app.update(key(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Jira);
        app.update(key(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Agenda);
        app.update(key(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Pulls);
        app.update(key(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Tasks);
        app.update(key(KeyCode::Tab));
        assert_eq!(app.focus, Panel::Email);
        app.update(key(KeyCode::BackTab));
        assert_eq!(app.focus, Panel::Tasks);
    }

    #[test]
    fn p_switches_to_the_by_parent_view_and_esc_returns() {
        let mut app = test_app();
        app.update(key(KeyCode::Tab)); // Email -> Jira
        assert_eq!(app.jira_view, JiraView::Issues);
        app.jira.cursor = 2;

        app.update(key(KeyCode::Char('p')));
        assert_eq!(app.jira_view, JiraView::ByParent);
        assert_eq!(app.jira.cursor, 0, "troca de visão reancora o cursor");
        app.update(key(KeyCode::Esc));
        assert_eq!(app.jira_view, JiraView::Issues);
    }

    /// Um envelope de teste; o `unread` é o que decide o `Space`.
    fn email_item(id: &str, unread: bool) -> EmailItem {
        EmailItem {
            id: id.into(),
            account: Account::Personal,
            from: "alguem".into(),
            subject: "assunto".into(),
            unread,
            date: "2026-08-04 10:00+00:00".into(),
        }
    }

    /// Tarefa com duas subtarefas, a primeira concluída.
    fn task_with_subs(id: &str) -> TaskItem {
        let mut t = task(id, "com etapas", false);
        t.subtasks = vec![
            crate::data::tasks::SubTask {
                id: format!("{id}-s1"),
                title: "primeira".into(),
                completed: true,
            },
            crate::data::tasks::SubTask {
                id: format!("{id}-s2"),
                title: "segunda".into(),
                completed: false,
            },
        ];
        t
    }

    #[test]
    fn enter_expands_only_tasks_that_have_subtasks() {
        let (mut app, _rx) = task_app(vec![task_with_subs("T1"), task("T2", "sem etapas", false)]);

        app.update(key(KeyCode::Enter));
        assert!(app.tasks_expanded.contains("T1"), "expande a que tem etapas");
        app.update(key(KeyCode::Enter));
        assert!(!app.tasks_expanded.contains("T1"), "Enter de novo recolhe");

        // Cursor na segunda tarefa, que não tem etapas: nada acontece.
        app.tasks.cursor = 1;
        app.update(key(KeyCode::Enter));
        assert!(app.tasks_expanded.is_empty());
    }

    #[test]
    fn expanding_reanchors_the_cursor_on_the_task_not_the_index() {
        // Com a primeira expandida, a segunda tarefa desce duas linhas. Recolher
        // pelo cursor na própria tarefa tem de voltar o cursor para ela.
        let (mut app, _rx) = task_app(vec![task_with_subs("T1"), task("T2", "outra", false)]);
        app.update(key(KeyCode::Enter)); // expande T1, cursor volta para a linha de T1
        assert_eq!(app.tasks.cursor, 0);
        app.tasks.cursor = 3; // linha de T2 (T1, S1, S2, T2)
        app.update(key(KeyCode::Enter)); // T2 não tem etapas: nada muda
        assert_eq!(app.tasks.cursor, 3);
    }

    #[test]
    fn cursor_reaches_the_subtask_rows_of_the_last_task() {
        // Com uma tarefa só e duas etapas há 3 linhas. Se o cursor limitasse ao
        // número de tarefas (1), as etapas seriam inalcançáveis por `j`.
        let (mut app, _rx) = task_app(vec![task_with_subs("T1")]);
        app.update(key(KeyCode::Enter)); // expande
        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.tasks.cursor, 1);
        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.tasks.cursor, 2, "chega na última etapa");
        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.tasks.cursor, 2, "e não passa dela");
        app.update(key(KeyCode::Char('G')));
        assert_eq!(app.tasks.cursor, 2, "G vai para a última linha, não a última tarefa");
    }

    #[test]
    fn shift_a_creates_a_subtask_in_the_task_under_the_cursor() {
        let (mut app, rx) = task_app(vec![task("t1", "sozinha", false)]);
        app.update(key(KeyCode::Char('A')));
        match &app.prompt {
            Some(Prompt::Input {
                kind: InputKind::AddSubtask { task_id },
                buffer,
            }) => {
                assert_eq!(task_id, "t1");
                assert!(buffer.is_empty(), "começa em branco");
            }
            _ => panic!("esperava prompt de subtarefa"),
        }
        for c in "etapa".chars() {
            app.update(key(KeyCode::Char(c)));
        }
        app.update(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            WorkerCmd::SubTaskAdd { task_id, title } => {
                assert_eq!(task_id, "t1");
                assert_eq!(title, "etapa");
            }
            _ => panic!("esperava SubTaskAdd"),
        }
        assert!(
            app.tasks_expanded.contains("t1"),
            "a mãe abre: senão a subtarefa nova chega escondida"
        );
    }

    #[test]
    fn shift_a_on_a_subtask_row_creates_a_sibling_in_the_parent() {
        let (mut app, rx) = task_app(vec![task("t0", "outra", false), task_with_subs("t1")]);
        app.tasks.cursor = 1; // t1
        app.update(key(KeyCode::Enter)); // expande
        app.update(key(KeyCode::Char('j'))); // primeira subtarefa de t1
        app.update(key(KeyCode::Char('A')));
        for c in "irmã".chars() {
            app.update(key(KeyCode::Char(c)));
        }
        app.update(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            WorkerCmd::SubTaskAdd { task_id, title } => {
                assert_eq!(task_id, "t1", "vai para a mãe, não para a tarefa de índice 1");
                assert_eq!(title, "irmã");
            }
            _ => panic!("esperava SubTaskAdd"),
        }
    }

    #[test]
    fn edit_and_delete_do_not_fire_from_a_subtask_row() {
        // O cursor anda por linhas: com t1 expandida, indexar as tarefas pelo
        // cursor apontava para OUTRA tarefa — `d` apagava a errada.
        let (mut app, rx) = task_app(vec![task_with_subs("t1"), task("t2", "vizinha", false)]);
        app.update(key(KeyCode::Enter)); // expande t1
        app.update(key(KeyCode::Char('j'))); // primeira subtarefa

        app.update(key(KeyCode::Char('e')));
        assert!(app.prompt.is_none(), "subtarefa não tem edição de título");
        app.update(key(KeyCode::Char('d')));
        assert!(app.prompt.is_none(), "e muito menos exclusão da tarefa mãe");
        assert!(rx.try_recv().is_err(), "nenhum comando sai daí");
    }

    #[test]
    fn space_on_a_subtask_row_toggles_the_subtask_not_the_task() {
        let (mut app, rx) = task_app(vec![task_with_subs("T1")]);
        app.update(key(KeyCode::Enter)); // expande
        app.tasks.cursor = 2; // segunda subtarefa, ainda não concluída
        app.update(key(KeyCode::Char(' ')));
        match rx.try_recv() {
            Ok(WorkerCmd::SubTaskToggle {
                task_id,
                item_id,
                check,
            }) => {
                assert_eq!(task_id, "T1");
                assert_eq!(item_id, "T1-s2");
                assert!(check, "estava desmarcada, então marca");
            }
            other => panic!("esperava SubTaskToggle, veio outro comando: {}", other.is_ok()),
        }
    }

    #[test]
    fn picker_lists_folders_of_every_account_in_the_batch() {
        let mut app = test_app();
        let mut work = email_item("1", false);
        work.account = Account::Work;
        app.emails.items = vec![work, email_item("2", false)]; // 2 é Personal
        app.emails.loaded = true;
        app.emails_marked = [(Account::Work, "1".to_string()), (Account::Personal, "2".to_string())]
            .into_iter()
            .collect();
        app.folders
            .insert(Account::Work, vec!["Clientes".into()]);
        app.folders
            .insert(Account::Personal, vec!["Faturas".into()]);

        app.update(key(KeyCode::Char('m')));
        match &app.prompt {
            Some(Prompt::PickFolder { folders, .. }) => {
                assert_eq!(folders.len(), 2, "as duas contas do lote entram");
                assert!(folders.contains(&(Account::Work, "Clientes".to_string())));
                assert!(folders.contains(&(Account::Personal, "Faturas".to_string())));
            }
            _ => panic!("esperava PickFolder"),
        }
    }

    #[test]
    fn moving_only_touches_the_targets_of_the_chosen_folders_account() {
        let mut app = test_app();
        let mut work = email_item("1", false);
        work.account = Account::Work;
        app.emails.items = vec![work, email_item("2", false)];
        app.emails.loaded = true;
        app.emails_marked = [(Account::Work, "1".to_string()), (Account::Personal, "2".to_string())]
            .into_iter()
            .collect();
        app.prompt = Some(Prompt::PickFolder {
            items: vec![
                (Account::Work, "1".to_string()),
                (Account::Personal, "2".to_string()),
            ],
            // Etiqueta que só existe no work: a pessoal não pode ir para lá.
            folders: vec![(Account::Work, "Clientes".to_string())],
            cursor: 0,
        });

        app.update(key(KeyCode::Enter));
        assert_eq!(app.emails.items.len(), 1, "só o do work sai da tela");
        assert_eq!(app.emails.items[0].id, "2");
    }

    #[test]
    fn x_toggles_the_mark_of_a_single_email() {
        // Shift+setas só marca; desmarcar um item sem perder a faixa é o `x`.
        let mut app = test_app();
        app.emails.items = vec![email_item("1", true), email_item("2", true)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('x')));
        assert!(app.emails_marked.contains(&(Account::Personal, "1".to_string())));
        app.update(key(KeyCode::Char('x')));
        assert!(app.emails_marked.is_empty(), "o mesmo `x` desmarca");

        // Marca a faixa e desmarca só um, sem mexer no outro.
        let shift_down = KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT);
        app.emails.cursor = 0;
        app.update(Msg::Key(shift_down));
        assert_eq!(app.emails_marked.len(), 2);
        app.update(key(KeyCode::Char('x')));
        assert_eq!(app.emails_marked.len(), 1, "sai só o do cursor");
        assert!(
            app.emails_marked.contains(&(Account::Personal, "1".to_string())),
            "o outro fica marcado"
        );
    }

    #[test]
    fn shift_arrows_mark_a_run_and_esc_clears_it() {
        let mut app = test_app();
        app.emails.items = vec![
            email_item("1", true),
            email_item("2", true),
            email_item("3", true),
        ];
        app.emails.loaded = true;

        let shift_down = KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT);
        app.update(Msg::Key(shift_down));
        assert_eq!(app.emails_marked.len(), 2, "marca o de origem e o de destino");
        assert_eq!(app.emails.cursor, 1);
        app.update(Msg::Key(shift_down));
        assert_eq!(app.emails_marked.len(), 3, "a faixa cresce por onde passa");

        app.update(key(KeyCode::Esc));
        assert!(app.emails_marked.is_empty(), "Esc sai do modo lote");
    }

    #[test]
    fn actions_apply_to_the_marked_batch_not_just_the_cursor() {
        let mut app = test_app();
        app.emails.items = vec![
            email_item("1", true),
            email_item("2", false),
            email_item("3", true),
        ];
        app.emails.loaded = true;
        app.emails_marked = [
            (Account::Personal, "1".to_string()),
            (Account::Personal, "3".to_string()),
        ]
        .into_iter()
        .collect();

        app.update(key(KeyCode::Char('d')));
        match &app.prompt {
            Some(Prompt::ConfirmEmailDelete { items, what }) => {
                assert_eq!(items.len(), 2, "os dois marcados, não o do cursor");
                assert_eq!(what, "2 e-mails marcados");
            }
            _ => panic!("esperava ConfirmEmailDelete"),
        }
        app.update(key(KeyCode::Char('y')));
        assert_eq!(app.emails.items.len(), 1, "saem os dois da tela na hora");
        assert_eq!(app.emails.items[0].id, "2");
        assert!(app.emails_marked.is_empty(), "a marcação é consumida");
    }

    #[test]
    fn batch_seen_marks_everything_read_when_any_is_unread() {
        let mut app = test_app();
        app.emails.items = vec![email_item("1", true), email_item("2", false)];
        app.emails.loaded = true;
        app.emails_marked = [
            (Account::Personal, "1".to_string()),
            (Account::Personal, "2".to_string()),
        ]
        .into_iter()
        .collect();

        app.update(key(KeyCode::Char(' ')));
        assert!(
            app.emails.items.iter().all(|e| !e.unread),
            "basta um não lido para a ação virar marcar todos como lidos"
        );
    }

    #[test]
    fn space_on_email_flips_the_state_on_screen_immediately() {
        // A tela não espera o IMAP: o efeito aparece no mesmo frame.
        let mut app = test_app();
        app.emails.items = vec![email_item("1", true)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char(' ')));
        assert!(!app.emails.items[0].unread, "virou lido na hora");
        app.update(key(KeyCode::Char(' ')));
        assert!(app.emails.items[0].unread, "e volta a não lido");
    }

    #[test]
    fn deleting_an_email_removes_it_from_the_list_immediately() {
        let mut app = test_app();
        app.emails.items = vec![email_item("1", false), email_item("2", false)];
        app.emails.loaded = true;
        app.emails.cursor = 1;

        app.update(key(KeyCode::Char('d')));
        app.update(key(KeyCode::Char('y'))); // confirma
        assert_eq!(app.emails.items.len(), 1);
        assert_eq!(app.emails.items[0].id, "1");
        assert_eq!(app.emails.cursor, 0, "o cursor não fica fora dos limites");
    }

    #[test]
    fn a_deleted_email_does_not_come_back_with_a_list_from_before_the_delete() {
        // O bug: marcar como lido e excluir em seguida. A escrita do "lido"
        // termina com uma re-busca, e essa lista — pedida antes da exclusão
        // chegar ao servidor — trazia o e-mail de volta. O usuário excluía de
        // novo, e a segunda tentativa falhava porque o e-mail já tinha saído.
        let mut app = test_app();
        app.emails.items = vec![email_item("1", false), email_item("2", false)];
        app.emails.loaded = true;
        app.emails.cursor = 1;

        app.update(key(KeyCode::Char('d')));
        app.update(key(KeyCode::Char('y')));

        // Lista do servidor de antes da exclusão (a re-busca da escrita anterior).
        app.update(Msg::EmailsLoaded(Ok(vec![
            email_item("1", false),
            email_item("2", false),
        ])));
        assert_eq!(
            app.emails.items.iter().map(|e| &e.id).collect::<Vec<_>>(),
            vec!["1"],
            "o excluído não reaparece enquanto a exclusão está na fila"
        );

        // Resposta da própria exclusão: daqui em diante a lista do servidor vale.
        app.update(Msg::EmailWrite {
            targets: vec![(Account::Personal, "2".to_string())],
            error: None,
            list: Ok(vec![email_item("1", false)]),
        });
        app.update(Msg::EmailsLoaded(Ok(vec![
            email_item("1", false),
            email_item("2", false),
        ])));
        assert_eq!(
            app.emails.items.len(),
            2,
            "encerrada a pendência, o servidor volta a mandar na lista"
        );
    }

    #[test]
    fn a_failed_delete_brings_the_email_back_and_says_why() {
        // Falhar em silêncio era pior: sumia da tela, voltava no reload seguinte
        // e o motivo nunca aparecia.
        let mut app = test_app();
        app.emails.items = vec![email_item("1", false)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('d')));
        app.update(key(KeyCode::Char('y')));
        assert!(app.emails.items.is_empty(), "sai da tela na hora");

        app.update(Msg::EmailWrite {
            targets: vec![(Account::Personal, "1".to_string())],
            error: Some("himalaya falhou: pasta não encontrada".into()),
            list: Ok(vec![email_item("1", false)]),
        });
        assert_eq!(app.emails.items.len(), 1, "volta: não foi excluído mesmo");
        assert_eq!(
            app.emails.error.as_deref(),
            Some("himalaya falhou: pasta não encontrada")
        );
    }

    #[test]
    fn the_same_id_in_two_accounts_is_two_different_emails() {
        // O id do himalaya é a UID da pasta: as duas contas repetem números.
        let mut app = test_app();
        let mut work = email_item("7", false);
        work.account = Account::Work;
        app.emails.items = vec![work, email_item("7", false)]; // o segundo é Personal
        app.emails.loaded = true;
        app.emails.cursor = 0; // o do work

        app.update(key(KeyCode::Char('x')));
        assert_eq!(app.emails_marked.len(), 1, "marca só o do work");

        app.update(key(KeyCode::Char('d')));
        app.update(key(KeyCode::Char('y')));
        assert_eq!(app.emails.items.len(), 1, "o da conta pessoal fica");
        assert_eq!(app.emails.items[0].account, Account::Personal);
    }

    #[test]
    fn n_opens_the_notifications_overlay_from_any_panel() {
        // Notificação se vê de onde estiver, e é overlay: não rouba o painel.
        let mut app = test_app();
        assert_eq!(app.focus, Panel::Email);
        app.update(key(KeyCode::Char('n')));
        assert!(app.notifications.is_some());
        assert_eq!(app.focus, Panel::Email, "o painel focado não muda");
        app.update(key(KeyCode::Esc));
        assert!(app.notifications.is_none(), "Esc fecha a central");
    }

    /// Menção do Jira, o suficiente para virar notificação.
    fn mention(key: &str) -> JiraItem {
        JiraItem {
            key: key.into(),
            summary: format!("menção em {key}"),
            status: "Em andamento".into(),
            project: "ENG".into(),
            url: format!("https://example.atlassian.net/browse/{key}"),
            parent: None,
            role: Default::default(),
        }
    }

    /// Banco novo num arquivo só deste teste.
    fn temp_store(name: &str) -> (Store, std::path::PathBuf) {
        let path = std::env::temp_dir()
            .join("daily-tui-tests")
            .join(format!("app-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        (Store::open_at(&path).expect("abre o banco"), path)
    }

    #[test]
    fn a_notification_marked_read_is_gone_on_the_next_run() {
        let (store, path) = temp_store("read-notifications");
        let mut app = test_app();
        app.attach_store(Ok(store));
        app.jira_mentions.set(Ok(vec![mention("ENG-1"), mention("ENG-2")]));

        app.update(key(KeyCode::Char('n')));
        app.update(key(KeyCode::Char(' '))); // marca a primeira como lida
        assert_eq!(
            app.notification_items()
                .iter()
                .map(|n| n.id.clone())
                .collect::<Vec<_>>(),
            vec!["jira:ENG-2"],
            "sai da lista na hora"
        );

        // Execução seguinte: mesmo banco, mesmas menções vindas do Jira.
        let mut next = test_app();
        next.attach_store(Store::open_at(&path));
        next.jira_mentions.set(Ok(vec![mention("ENG-1"), mention("ENG-2")]));
        assert_eq!(
            next.notification_items().len(),
            1,
            "a lida não volta ao abrir o programa de novo"
        );
        assert!(next.store_error.is_none());
    }

    #[test]
    fn reading_the_last_notification_keeps_the_cursor_in_the_list() {
        let (store, _) = temp_store("read-cursor");
        let mut app = test_app();
        app.attach_store(Ok(store));
        app.jira_mentions.set(Ok(vec![mention("ENG-1"), mention("ENG-2")]));

        app.update(key(KeyCode::Char('n')));
        app.update(key(KeyCode::Char('j'))); // cursor na última
        app.update(key(KeyCode::Char(' ')));
        assert_eq!(app.notification_items().len(), 1);
        assert_eq!(
            app.notifications.as_ref().unwrap().cursor,
            0,
            "o cursor não fica apontando para fora da lista"
        );
    }

    #[test]
    fn cached_folders_fill_the_picker_before_the_server_answers() {
        let (store, path) = temp_store("folder-cache");
        let at = "2026-08-04T10:00:00-03:00";
        store
            .save_folders(
                Account::Personal,
                &["INBOX".to_string(), "Faturas".to_string()],
                at,
            )
            .unwrap();
        drop(store);

        let mut app = test_app();
        app.attach_store(Store::open_at(&path));
        app.emails.items = vec![email_item("42", false)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('m')));
        match &app.prompt {
            Some(Prompt::PickFolder { folders, .. }) => assert_eq!(
                folders.len(),
                2,
                "abre com o que estava em cache, sem esperar o IMAP"
            ),
            _ => panic!("esperava PickFolder"),
        }
    }

    #[test]
    fn folders_from_the_server_are_written_to_the_cache() {
        let (store, path) = temp_store("folder-save");
        let mut app = test_app();
        app.attach_store(Ok(store));

        app.update(Msg::FoldersLoaded(
            Account::Work,
            Ok(vec!["INBOX".into(), "Clientes".into()]),
        ));

        let saved = Store::open_at(&path).unwrap().folders().unwrap();
        assert_eq!(
            saved.get(&Account::Work),
            Some(&vec!["INBOX".to_string(), "Clientes".to_string()])
        );
    }

    #[test]
    fn without_a_store_the_center_still_works_and_says_why() {
        // Banco é conveniência: sem ele o programa não pode parar de funcionar.
        let mut app = test_app();
        app.attach_store(Err("disco cheio".into()));
        app.jira_mentions.set(Ok(vec![mention("ENG-1")]));

        app.update(key(KeyCode::Char('n')));
        app.update(key(KeyCode::Char(' ')));
        assert!(
            app.notification_items().is_empty(),
            "marcar como lida vale nesta sessão mesmo sem banco"
        );
        assert_eq!(app.store_error.as_deref(), Some("disco cheio"));
    }

    #[test]
    fn m_opens_the_folder_picker_and_j_k_walk_the_real_folders() {
        let mut app = test_app();
        app.emails.items = vec![email_item("42", false)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('m')));
        match &app.prompt {
            Some(Prompt::PickFolder { items, folders, .. }) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].1, "42");
                assert!(folders.is_empty(), "abre vazio: a lista vem do servidor");
            }
            _ => panic!("esperava PickFolder"),
        }

        // As pastas da conta chegam pelo worker e preenchem o prompt aberto.
        app.update(Msg::FoldersLoaded(
            Account::Personal,
            Ok(vec!["inbox".into(), "Clientes".into(), "Faturas".into()]),
        ));
        assert!(matches!(&app.prompt, Some(Prompt::PickFolder { folders, .. }) if folders.len() == 3));

        app.update(key(KeyCode::Char('j')));
        assert!(matches!(&app.prompt, Some(Prompt::PickFolder { cursor: 1, .. })));
        app.update(key(KeyCode::Char('j')));
        app.update(key(KeyCode::Char('j')));
        assert!(
            matches!(&app.prompt, Some(Prompt::PickFolder { cursor: 2, .. })),
            "não passa do fim da lista"
        );
        app.update(key(KeyCode::Esc));
        assert!(app.prompt.is_none());
    }

    #[test]
    fn d_on_email_asks_before_deleting_and_n_cancels() {
        let mut app = test_app();
        app.emails.items = vec![email_item("9", false)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('d')));
        assert!(
            matches!(&app.prompt, Some(Prompt::ConfirmEmailDelete { what, .. }) if what == "assunto"),
            "excluir e-mail sempre pede confirmação"
        );
        app.update(key(KeyCode::Char('n')));
        assert!(app.prompt.is_none());
    }

    #[test]
    fn email_keys_do_not_leak_into_the_tasks_panel() {
        let mut app = test_app();
        app.emails.items = vec![email_item("1", true)];
        app.emails.loaded = true;
        // Tab até Tarefas: `m` e `d` ali não podem abrir prompt de e-mail.
        for _ in 0..4 {
            app.update(key(KeyCode::Tab));
        }
        assert_eq!(app.focus, Panel::Tasks);
        app.update(key(KeyCode::Char('m')));
        assert!(app.prompt.is_none(), "`m` não faz nada fora do e-mail");
    }

    #[test]
    fn f_cycles_the_jira_filter_only_when_jira_is_focused() {
        let mut app = test_app();
        assert_eq!(app.jira_filter, JiraFilter::Assignee);

        // Sem foco no Jira, `f` não faz nada.
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Assignee);

        app.update(key(KeyCode::Tab)); // Email -> Jira
        assert_eq!(app.focus, Panel::Jira);
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Reporter);
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Both);
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Assignee, "o ciclo volta ao início");
    }

    #[test]
    fn q_sets_quit() {
        let mut app = test_app();
        app.update(key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn cursor_is_clamped_between_zero_and_last() {
        let mut app = test_app();
        app.emails.set(Ok(vec![email("1"), email("2"), email("3")]));
        // sobe além do topo
        app.update(key(KeyCode::Char('k')));
        assert_eq!(app.emails.cursor, 0);
        app.update(key(KeyCode::Char('j')));
        app.update(key(KeyCode::Char('j')));
        app.update(key(KeyCode::Char('j'))); // tenta passar do fim
        assert_eq!(app.emails.cursor, 2);
        app.update(key(KeyCode::Char('g')));
        assert_eq!(app.emails.cursor, 0);
        app.update(key(KeyCode::Char('G')));
        assert_eq!(app.emails.cursor, 2);
    }

    #[test]
    fn load_keeps_cursor_in_bounds_when_list_shrinks() {
        let mut app = test_app();
        app.emails.set(Ok(vec![email("1"), email("2"), email("3")]));
        app.update(key(KeyCode::Char('G')));
        assert_eq!(app.emails.cursor, 2);
        app.emails.set(Ok(vec![email("1")])); // lista encolheu
        assert_eq!(app.emails.cursor, 0);
    }

    #[test]
    fn enter_opens_detail_only_on_email_panel() {
        let mut app = test_app();
        app.emails.set(Ok(vec![email("1")]));
        app.focus = Panel::Agenda;
        app.update(key(KeyCode::Enter));
        assert!(app.detail.is_none(), "agenda não abre detalhe");
        app.focus = Panel::Email;
        app.update(key(KeyCode::Enter));
        assert!(app.detail.is_some(), "email abre detalhe");
        assert_eq!(app.detail.as_ref().unwrap().subject, "s1");
    }

    #[test]
    fn esc_closes_detail_and_does_not_quit() {
        let mut app = test_app();
        app.emails.set(Ok(vec![email("1")]));
        app.update(key(KeyCode::Enter));
        assert!(app.detail.is_some());
        app.update(key(KeyCode::Esc));
        assert!(app.detail.is_none());
        assert!(!app.should_quit);
    }

    #[test]
    fn email_body_message_fills_open_detail() {
        let mut app = test_app();
        app.emails.set(Ok(vec![email("1")]));
        app.update(key(KeyCode::Enter));
        app.update(Msg::EmailBody(Account::Work, "1".into(), Ok("corpo".into())));
        let d = app.detail.as_ref().unwrap();
        assert_eq!(d.body.as_ref().unwrap().as_ref().unwrap(), "corpo");
    }

    #[test]
    fn a_prefetched_body_opens_instantly_without_asking_again() {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(BubbleTheme::default(), tx);
        app.emails.set(Ok(vec![email("1")]));

        // O tique de 1s pede o corpo do e-mail sob o cursor.
        app.update(Msg::ClockTick);
        assert!(matches!(rx.try_recv(), Ok(WorkerCmd::ReadEmail { .. })));
        // Um segundo tique não repete o pedido.
        app.update(Msg::ClockTick);
        assert!(rx.try_recv().is_err(), "não pede duas vezes o mesmo corpo");

        app.update(Msg::EmailBody(
            Account::Work,
            "1".into(),
            Ok("já em cache".into()),
        ));
        app.update(key(KeyCode::Enter));
        let d = app.detail.as_ref().unwrap();
        assert_eq!(
            d.body.as_ref().unwrap().as_ref().unwrap(),
            "já em cache",
            "abre preenchido, sem passar por carregando"
        );
        assert!(rx.try_recv().is_err(), "e sem pedir de novo ao abrir");
    }

    #[test]
    fn agenda_scroll_clamps_at_zero_and_increments() {
        let mut app = test_app();
        app.focus = Panel::Agenda;
        app.update(key(KeyCode::Char('k'))); // não passa de 0
        assert_eq!(app.agenda.scroll.get(), 0);
        app.update(key(KeyCode::Char('j')));
        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.agenda.scroll.get(), 2);
        app.update(key(KeyCode::Char('g')));
        assert_eq!(app.agenda.scroll.get(), 0);
    }

    #[test]
    fn error_result_is_stored_and_marks_loaded() {
        let mut app = test_app();
        app.update(Msg::PullsLoaded(Err("falhou".into())));
        assert!(app.pulls.loaded);
        assert_eq!(app.pulls.error.as_deref(), Some("falhou"));
    }

    // --- Painel de tarefas (interativo) ---

    /// App que preserva o receiver do worker, para inspecionar os comandos.
    fn task_app(items: Vec<TaskItem>) -> (App, mpsc::Receiver<WorkerCmd>) {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(BubbleTheme::default(), tx);
        app.tasks.set(Ok(items));
        app.focus = Panel::Tasks;
        (app, rx)
    }

    fn task(id: &str, title: &str, completed: bool) -> TaskItem {
        TaskItem {
            id: id.into(),
            title: title.into(),
            completed,
            subtasks: Vec::new(),
            due: String::new(),
            notes: String::new(),
        }
    }

    #[test]
    fn space_toggles_complete_for_pending_and_reopen_for_completed() {
        let (mut app, rx) = task_app(vec![task("t1", "pendente", false), task("t2", "feita", true)]);
        // Cursor em t1 (pendente) -> Complete.
        app.update(key(KeyCode::Char(' ')));
        match rx.try_recv().unwrap() {
            WorkerCmd::TaskComplete(id) => assert_eq!(id, "t1"),
            _ => panic!("esperava TaskComplete"),
        }
        // Move para t2 (concluída) -> Reopen.
        app.update(key(KeyCode::Char('j')));
        app.update(key(KeyCode::Char(' ')));
        match rx.try_recv().unwrap() {
            WorkerCmd::TaskReopen(id) => assert_eq!(id, "t2"),
            _ => panic!("esperava TaskReopen"),
        }
    }

    #[test]
    fn add_prompt_collects_text_and_submits_task_add() {
        let (mut app, rx) = task_app(vec![]);
        app.update(key(KeyCode::Char('a')));
        assert!(app.prompt.is_some());
        for c in "oi".chars() {
            app.update(key(KeyCode::Char(c)));
        }
        app.update(key(KeyCode::Enter));
        assert!(app.prompt.is_none(), "prompt fecha ao enviar");
        match rx.try_recv().unwrap() {
            WorkerCmd::TaskAdd(title) => assert_eq!(title, "oi"),
            _ => panic!("esperava TaskAdd"),
        }
    }

    #[test]
    fn esc_cancels_prompt_without_command() {
        let (mut app, rx) = task_app(vec![]);
        app.update(key(KeyCode::Char('a')));
        app.update(key(KeyCode::Char('x')));
        app.update(key(KeyCode::Esc));
        assert!(app.prompt.is_none());
        assert!(rx.try_recv().is_err(), "nenhum comando deve ser enviado");
    }

    #[test]
    fn empty_title_submits_nothing() {
        let (mut app, rx) = task_app(vec![]);
        app.update(key(KeyCode::Char('a')));
        app.update(key(KeyCode::Enter)); // buffer vazio
        assert!(app.prompt.is_none());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn edit_prompt_prefills_title_and_submits_edit() {
        let (mut app, rx) = task_app(vec![task("t9", "antigo", false)]);
        app.update(key(KeyCode::Char('e')));
        match &app.prompt {
            Some(Prompt::Input { buffer, .. }) => assert_eq!(buffer, "antigo"),
            _ => panic!("esperava prompt de edição preenchido"),
        }
        app.update(key(KeyCode::Char('!')));
        app.update(key(KeyCode::Enter));
        match rx.try_recv().unwrap() {
            WorkerCmd::TaskEdit { id, title } => {
                assert_eq!(id, "t9");
                assert_eq!(title, "antigo!");
            }
            _ => panic!("esperava TaskEdit"),
        }
    }

    #[test]
    fn delete_confirms_then_sends_delete() {
        let (mut app, rx) = task_app(vec![task("t5", "apagar", false)]);
        app.update(key(KeyCode::Char('d')));
        assert!(matches!(app.prompt, Some(Prompt::ConfirmDelete { .. })));
        // 'n' cancela.
        app.update(key(KeyCode::Char('n')));
        assert!(app.prompt.is_none());
        assert!(rx.try_recv().is_err());
        // 'd' de novo e 'y' confirma.
        app.update(key(KeyCode::Char('d')));
        app.update(key(KeyCode::Char('y')));
        match rx.try_recv().unwrap() {
            WorkerCmd::TaskDelete(id) => assert_eq!(id, "t5"),
            _ => panic!("esperava TaskDelete"),
        }
    }

    #[test]
    fn task_keys_do_nothing_when_panel_not_focused() {
        let (mut app, rx) = task_app(vec![task("t1", "x", false)]);
        app.focus = Panel::Email;
        app.update(key(KeyCode::Char('a')));
        assert!(app.prompt.is_none());
        app.update(key(KeyCode::Char(' ')));
        assert!(rx.try_recv().is_err());
    }
}
