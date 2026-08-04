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
use crate::data::{email, tasks};
use crate::data::{Account, AgendaItem, EmailItem, TaskItem};
use crate::msg::Msg;
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
}

/// Overlay modal de interação com tarefas (entrada de texto ou confirmação).
pub enum Prompt {
    /// Campo de texto (criar/editar tarefa).
    Input { kind: InputKind, buffer: String },
    /// Confirmação de exclusão da tarefa selecionada.
    ConfirmDelete { id: String, title: String },
    /// Escolha de pasta para mover o e-mail sob o cursor.
    PickFolder {
        account: Account,
        id: String,
        cursor: usize,
    },
    /// Confirmação de exclusão do e-mail (move para a Lixeira).
    ConfirmEmailDelete {
        account: Account,
        id: String,
        subject: String,
    },
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
            cmd_tx,
        }
    }

    /// `PanelData` de Jira sob o cursor na visão ativa: `jira_mentions` na
    /// visão de menções (dados próprios), `jira` nas outras duas (issues e
    /// por-pai só reagrupam o mesmo conjunto).
    fn jira_panel_data(&mut self) -> &mut PanelData<JiraItem> {
        match self.jira_view {
            JiraView::Mentions => &mut self.jira_mentions,
            JiraView::Issues | JiraView::ByParent => &mut self.jira,
        }
    }

    /// Rola/move o painel focado. E-mail move a seleção; agenda/PRs rolam livre.
    fn focused_scroll(&mut self, delta: isize) {
        match self.focus {
            Panel::Email => self.emails.move_cursor(delta),
            Panel::Jira => self.jira_panel_data().move_cursor(delta),
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
            Panel::Jira => self.jira_panel_data().to_first(),
            Panel::Agenda => self.agenda.scroll.set(0),
            Panel::Pulls => self.pulls.scroll.set(0),
            Panel::Tasks => self.tasks.to_first(),
        }
    }

    fn focused_to_last(&mut self) {
        // Para listas roláveis, o valor grande é clampado ao máximo na render.
        match self.focus {
            Panel::Email => self.emails.to_last(),
            Panel::Jira => self.jira_panel_data().to_last(),
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
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.detail = Some(Detail {
                from: item.from.clone(),
                subject: item.subject.clone(),
                body: None,
                scroll: 0,
            });
            let _ = self.cmd_tx.send(WorkerCmd::ReadEmail {
                account: item.account,
                id: item.id.clone(),
            });
        }
    }

    /// Abre no navegador a issue sob o cursor. O erro vai para o painel, como
    /// qualquer outra falha de busca. Na visão de menções, a issue e o cursor
    /// vêm de `jira_mentions`, não de `jira` — são conjuntos de dados diferentes.
    fn open_selected_issue(&mut self) {
        let url = match self.jira_view {
            JiraView::Mentions => self.jira_mentions.items.get(self.jira_mentions.cursor).map(|i| i.url.clone()),
            JiraView::Issues | JiraView::ByParent => self.jira.items.get(self.jira.cursor).map(|i| i.url.clone()),
        };
        let Some(url) = url else {
            return;
        };
        if let Err(e) = crate::data::jira::open_url(&url) {
            match self.jira_view {
                JiraView::Mentions => self.jira_mentions.error = Some(e),
                JiraView::Issues | JiraView::ByParent => self.jira.error = Some(e),
            }
        }
    }

    /// Alterna lido/não lido do e-mail sob o cursor.
    fn toggle_email_seen(&mut self) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            let _ = self.cmd_tx.send(WorkerCmd::EmailSetSeen {
                account: item.account,
                id: item.id.clone(),
                // `unread` verdadeiro significa que falta a flag Seen: marcar.
                seen: item.unread,
            });
        }
    }

    /// Abre o seletor de pasta para o e-mail sob o cursor.
    fn open_move_email(&mut self) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.prompt = Some(Prompt::PickFolder {
                account: item.account,
                id: item.id.clone(),
                cursor: 0,
            });
        }
    }

    /// Pede confirmação antes de mover o e-mail para a Lixeira.
    fn open_delete_email(&mut self) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.prompt = Some(Prompt::ConfirmEmailDelete {
                account: item.account,
                id: item.id.clone(),
                subject: item.subject.clone(),
            });
        }
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

    /// Tarefa atualmente selecionada no painel de tarefas.
    fn selected_task(&self) -> Option<&TaskItem> {
        self.tasks.items.get(self.tasks.cursor)
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
        let cmd = match self.selected_row() {
            Some(tasks::TaskRow::Task(t)) => match self.tasks.items.get(t) {
                Some(item) if item.completed => WorkerCmd::TaskReopen(item.id.clone()),
                Some(item) => WorkerCmd::TaskComplete(item.id.clone()),
                None => return,
            },
            Some(tasks::TaskRow::Sub { task, sub }) => {
                let pair = self
                    .tasks
                    .items
                    .get(task)
                    .and_then(|item| item.subtasks.get(sub).map(|s| (item, s)));
                match pair {
                    Some((item, s)) => WorkerCmd::SubTaskToggle {
                        task_id: item.id.clone(),
                        item_id: s.id.clone(),
                        check: !s.completed,
                    },
                    None => return,
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
            Some(Prompt::PickFolder { cursor, .. }) => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    *cursor = (*cursor + 1).min(email::FOLDERS.len() - 1);
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
                }
            }
            Some(Prompt::ConfirmDelete { id, .. }) => WorkerCmd::TaskDelete(id),
            Some(Prompt::PickFolder {
                account,
                id,
                cursor,
            }) => WorkerCmd::EmailMove {
                account,
                id,
                folder: email::FOLDERS[cursor.min(email::FOLDERS.len() - 1)].to_string(),
            },
            Some(Prompt::ConfirmEmailDelete { account, id, .. }) => {
                WorkerCmd::EmailDelete { account, id }
            }
            None => return,
        };
        let _ = self.cmd_tx.send(cmd);
    }

    /// Trata teclas no modo painel (dashboard).
    fn handle_panel_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
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
            KeyCode::Char(' ') if self.focus == Panel::Email => self.toggle_email_seen(),
            KeyCode::Char('m') if self.focus == Panel::Email => self.open_move_email(),
            KeyCode::Char('d') if self.focus == Panel::Email => self.open_delete_email(),
            KeyCode::Char(' ') if self.focus == Panel::Tasks => self.toggle_task(),
            KeyCode::Char('a') if self.focus == Panel::Tasks => self.open_add_task(),
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
            KeyCode::Char('n') if self.focus == Panel::Jira => {
                self.jira_view = JiraView::Mentions;
                self.jira_mentions.cursor = 0;
                // Menções têm dados próprios; busca na primeira vez e no refresh.
                if !self.jira_mentions.loaded {
                    let _ = self.cmd_tx.send(WorkerCmd::FetchJiraMentions);
                }
            }
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
                self.now = Local::now();
                self.spinner.tick();
            }
            Msg::Key(key) => {
                if self.detail.is_some() {
                    self.handle_detail_key(key);
                } else if self.prompt.is_some() {
                    self.handle_prompt_key(key);
                } else {
                    self.handle_panel_key(key);
                }
            }
            Msg::EmailsLoaded(res) => {
                self.emails.set(res);
                self.last_refresh = Some(Local::now());
            }
            Msg::AgendaLoaded(res) => self.agenda.set(res),
            Msg::PullsLoaded(res) => self.pulls.set(res),
            Msg::JiraLoaded(res) => self.jira.set(res),
            Msg::JiraMentions(res) => self.jira_mentions.set(res),
            Msg::TasksLoaded(res) => self.tasks.set(res),
            Msg::EmailBody(res) => {
                if let Some(d) = &mut self.detail {
                    d.body = Some(res);
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

    fn jira_item(key: &str) -> JiraItem {
        JiraItem {
            key: key.into(),
            summary: "s".into(),
            status: "st".into(),
            project: "P".into(),
            url: "u".into(),
            parent: None,
            role: crate::data::jira::JiraRole::default(),
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
    fn p_and_n_switch_jira_views_and_esc_returns() {
        let mut app = test_app();
        app.update(key(KeyCode::Tab)); // Email -> Jira
        assert_eq!(app.jira_view, JiraView::Issues);

        // Cursor fora do zero antes de trocar de visão: senão `cursor == 0`
        // passaria mesmo sem reset nenhum, e não provaria nada.
        app.jira.items = vec![jira_item("A-1"), jira_item("A-2"), jira_item("A-3")];
        app.jira.cursor = 2;

        app.update(key(KeyCode::Char('p')));
        assert_eq!(app.jira_view, JiraView::ByParent);
        assert_eq!(app.jira.cursor, 0, "trocar para por-pai reseta o cursor");
        app.update(key(KeyCode::Esc));
        assert_eq!(app.jira_view, JiraView::Issues);

        app.jira_mentions.cursor = 3;
        app.update(key(KeyCode::Char('n')));
        assert_eq!(app.jira_view, JiraView::Mentions);
        assert_eq!(app.jira_mentions.cursor, 0, "trocar para menções reseta o cursor");
        app.update(key(KeyCode::Esc));
        assert_eq!(app.jira_view, JiraView::Issues, "Esc sempre volta para issues");
    }

    #[test]
    fn mentions_view_moves_its_own_cursor_not_issues_cursor() {
        // Cobre o roteamento de `jira_panel_data`/`open_selected_issue` pela
        // visão ativa: sem ele, `j`/`k` na visão de menções moveriam o cursor
        // de `jira` (que fica escondido) e o cursor exibido nunca se moveria.
        let mut app = test_app();
        app.focus = Panel::Jira;
        app.jira.items = vec![jira_item("A-1"), jira_item("A-2")];
        app.jira.cursor = 1; // valor conhecido, não deve mudar
        app.jira_mentions.items = vec![jira_item("M-1"), jira_item("M-2"), jira_item("M-3")];

        app.update(key(KeyCode::Char('n')));
        assert_eq!(app.jira_view, JiraView::Mentions);
        assert_eq!(app.jira_mentions.cursor, 0);

        app.update(key(KeyCode::Char('j')));
        assert_eq!(app.jira_mentions.cursor, 1, "j na visão de menções move jira_mentions.cursor");
        assert_eq!(app.jira.cursor, 1, "cursor de issues não deve ser afetado");
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
    fn m_opens_the_folder_picker_and_j_k_walk_it() {
        let mut app = test_app();
        app.emails.items = vec![email_item("42", false)];
        app.emails.loaded = true;
        assert_eq!(app.focus, Panel::Email);

        app.update(key(KeyCode::Char('m')));
        match &app.prompt {
            Some(Prompt::PickFolder { id, cursor, .. }) => {
                assert_eq!(id, "42");
                assert_eq!(*cursor, 0);
            }
            _ => panic!("esperava PickFolder"),
        }
        app.update(key(KeyCode::Char('j')));
        assert!(matches!(&app.prompt, Some(Prompt::PickFolder { cursor: 1, .. })));
        app.update(key(KeyCode::Char('k')));
        assert!(matches!(&app.prompt, Some(Prompt::PickFolder { cursor: 0, .. })));
        // Não passa do início nem do fim.
        app.update(key(KeyCode::Char('k')));
        assert!(matches!(&app.prompt, Some(Prompt::PickFolder { cursor: 0, .. })));
        app.update(key(KeyCode::Esc));
        assert!(app.prompt.is_none(), "Esc fecha sem mover");
    }

    #[test]
    fn d_on_email_asks_before_deleting_and_n_cancels() {
        let mut app = test_app();
        app.emails.items = vec![email_item("9", false)];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('d')));
        assert!(
            matches!(&app.prompt, Some(Prompt::ConfirmEmailDelete { subject, .. }) if subject == "assunto"),
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
        app.update(Msg::EmailBody(Ok("corpo".into())));
        let d = app.detail.as_ref().unwrap();
        assert_eq!(d.body.as_ref().unwrap().as_ref().unwrap(), "corpo");
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
