//! Estado da aplicação (Model) e o reducer `update`.

use std::cell::Cell;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui_bubbletea_components::{Spinner, SpinnerFrames};
use ratatui_bubbletea_theme::BubbleTheme;
use ratatui_tea::{Cmd, Model};

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

/// Overlay de seleção de pasta/marcador para mover o e-mail selecionado.
pub struct FolderPicker {
    /// Conta do e-mail a mover.
    pub account: Account,
    /// ID do e-mail a mover.
    pub email_id: String,
    /// Assunto, exibido no título do overlay.
    pub subject: String,
    /// `None` enquanto carrega; depois a lista de pastas ou um erro.
    pub folders: Option<Result<Vec<String>, String>>,
    /// Pasta selecionada.
    pub cursor: usize,
}

impl FolderPicker {
    /// As pastas já carregadas (vazio enquanto carrega ou em erro).
    fn folder_list(&self) -> &[String] {
        match &self.folders {
            Some(Ok(v)) => v,
            _ => &[],
        }
    }

    /// Move o cursor por `delta`, mantendo-o dentro dos limites.
    fn move_cursor(&mut self, delta: isize) {
        let len = self.folder_list().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let max = (len - 1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
    }

    fn to_last(&mut self) {
        self.cursor = self.folder_list().len().saturating_sub(1);
    }

    /// Pasta atualmente selecionada.
    fn selected(&self) -> Option<&String> {
        self.folder_list().get(self.cursor)
    }
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
}

/// Modelo principal da aplicação.
pub struct App {
    pub theme: BubbleTheme,
    pub should_quit: bool,
    pub now: DateTime<Local>,
    pub focus: Panel,
    pub emails: PanelData<EmailItem>,
    pub jira: PanelData<String>,
    pub agenda: PanelData<AgendaItem>,
    pub pulls: PanelData<String>,
    pub tasks: PanelData<TaskItem>,
    pub spinner: Spinner,
    pub last_refresh: Option<DateTime<Local>>,
    pub detail: Option<Detail>,
    pub prompt: Option<Prompt>,
    pub folder_picker: Option<FolderPicker>,
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
            agenda: PanelData::new(),
            pulls: PanelData::new(),
            tasks: PanelData::new(),
            spinner: Spinner::new()
                .frames(SpinnerFrames::DOTS)
                .label("carregando"),
            last_refresh: None,
            detail: None,
            prompt: None,
            folder_picker: None,
            cmd_tx,
        }
    }

    /// Rola/move o painel focado. E-mail move a seleção; agenda/PRs rolam livre.
    fn focused_scroll(&mut self, delta: isize) {
        match self.focus {
            Panel::Email => self.emails.move_cursor(delta),
            Panel::Jira => self.jira.scroll_by(delta),
            Panel::Agenda => self.agenda.scroll_by(delta),
            Panel::Pulls => self.pulls.scroll_by(delta),
            Panel::Tasks => self.tasks.move_cursor(delta),
        }
    }

    fn focused_to_first(&mut self) {
        match self.focus {
            Panel::Email => self.emails.to_first(),
            Panel::Jira => self.jira.scroll.set(0),
            Panel::Agenda => self.agenda.scroll.set(0),
            Panel::Pulls => self.pulls.scroll.set(0),
            Panel::Tasks => self.tasks.to_first(),
        }
    }

    fn focused_to_last(&mut self) {
        // Para listas roláveis, o valor grande é clampado ao máximo na render.
        match self.focus {
            Panel::Email => self.emails.to_last(),
            Panel::Jira => self.jira.scroll.set(usize::MAX),
            Panel::Agenda => self.agenda.scroll.set(usize::MAX),
            Panel::Pulls => self.pulls.scroll.set(usize::MAX),
            Panel::Tasks => self.tasks.to_last(),
        }
    }

    fn open_detail(&mut self) {
        if self.focus != Panel::Email {
            return;
        }
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

    /// Alterna o e-mail selecionado entre lido e não-lido.
    fn toggle_email_seen(&mut self) {
        if self.focus != Panel::Email {
            return;
        }
        if let Some(e) = self.emails.items.get(self.emails.cursor) {
            let _ = self.cmd_tx.send(WorkerCmd::SetEmailSeen {
                account: e.account,
                id: e.id.clone(),
                // não-lido -> marca como lido; lido -> volta a não-lido.
                seen: e.unread,
            });
        }
    }

    /// Abre o seletor de pasta para o e-mail selecionado e pede as pastas.
    fn open_folder_picker(&mut self) {
        if self.focus != Panel::Email {
            return;
        }
        if let Some(e) = self.emails.items.get(self.emails.cursor) {
            self.folder_picker = Some(FolderPicker {
                account: e.account,
                email_id: e.id.clone(),
                subject: e.subject.clone(),
                folders: None,
                cursor: 0,
            });
            let _ = self.cmd_tx.send(WorkerCmd::ListFolders(e.account));
        }
    }

    /// Trata teclas quando o seletor de pasta está aberto.
    fn handle_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.folder_picker = None,
            KeyCode::Enter => self.confirm_move(),
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(p) = &mut self.folder_picker {
                    p.move_cursor(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(p) = &mut self.folder_picker {
                    p.move_cursor(-1);
                }
            }
            KeyCode::Char('g') | KeyCode::Home => {
                if let Some(p) = &mut self.folder_picker {
                    p.cursor = 0;
                }
            }
            KeyCode::Char('G') | KeyCode::End => {
                if let Some(p) = &mut self.folder_picker {
                    p.to_last();
                }
            }
            _ => {}
        }
    }

    /// Move o e-mail para a pasta selecionada e fecha o seletor.
    fn confirm_move(&mut self) {
        let Some(picker) = &self.folder_picker else { return };
        let Some(target) = picker.selected().cloned() else { return };
        let _ = self.cmd_tx.send(WorkerCmd::MoveEmail {
            account: picker.account,
            id: picker.email_id.clone(),
            target,
        });
        self.folder_picker = None;
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
    fn toggle_task(&mut self) {
        if let Some(t) = self.selected_task() {
            let cmd = if t.completed {
                WorkerCmd::TaskReopen(t.id.clone())
            } else {
                WorkerCmd::TaskComplete(t.id.clone())
            };
            let _ = self.cmd_tx.send(cmd);
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
            Some(Prompt::ConfirmDelete { .. }) => match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.submit_prompt(),
                KeyCode::Char('n') | KeyCode::Esc => self.prompt = None,
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
            KeyCode::Enter => self.open_detail(),
            KeyCode::Char('r') => {
                let _ = self.cmd_tx.send(WorkerCmd::RefreshAll);
            }
            // Ações do painel de e-mails (só quando ele está focado).
            KeyCode::Char(' ') if self.focus == Panel::Email => self.toggle_email_seen(),
            KeyCode::Char('m') if self.focus == Panel::Email => self.open_folder_picker(),
            // Ações do painel de tarefas (só quando ele está focado).
            KeyCode::Char(' ') if self.focus == Panel::Tasks => self.toggle_task(),
            KeyCode::Char('a') if self.focus == Panel::Tasks => self.open_add_task(),
            KeyCode::Char('e') if self.focus == Panel::Tasks => self.open_edit_task(),
            KeyCode::Char('d') if self.focus == Panel::Tasks => self.open_delete_task(),
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
                } else if self.folder_picker.is_some() {
                    self.handle_picker_key(key);
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
            Msg::TasksLoaded(res) => self.tasks.set(res),
            Msg::EmailBody(res) => {
                if let Some(d) = &mut self.detail {
                    d.body = Some(res);
                }
            }
            Msg::FoldersLoaded(res) => {
                if let Some(p) = &mut self.folder_picker {
                    p.folders = Some(res);
                    p.cursor = 0;
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
    fn space_toggles_seen_for_selected_email() {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(BubbleTheme::default(), tx);
        let mut unread = email("1");
        unread.unread = true;
        app.emails.set(Ok(vec![unread]));
        app.focus = Panel::Email;
        app.update(key(KeyCode::Char(' ')));
        match rx.try_recv().unwrap() {
            WorkerCmd::SetEmailSeen { id, seen, .. } => {
                assert_eq!(id, "1");
                assert!(seen, "não-lido -> marca como lido");
            }
            _ => panic!("esperava SetEmailSeen"),
        }
    }

    #[test]
    fn m_opens_folder_picker_and_requests_folders() {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(BubbleTheme::default(), tx);
        app.emails.set(Ok(vec![email("7")]));
        app.focus = Panel::Email;
        app.update(key(KeyCode::Char('m')));
        assert!(app.folder_picker.is_some());
        assert!(matches!(rx.try_recv().unwrap(), WorkerCmd::ListFolders(_)));
    }

    #[test]
    fn folders_loaded_fills_picker_and_enter_moves() {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(BubbleTheme::default(), tx);
        app.emails.set(Ok(vec![email("9")]));
        app.focus = Panel::Email;
        app.update(key(KeyCode::Char('m')));
        let _ = rx.try_recv(); // consome ListFolders
        app.update(Msg::FoldersLoaded(Ok(vec![
            "INBOX".into(),
            "[Gmail]/Lixeira".into(),
        ])));
        app.update(key(KeyCode::Char('j'))); // seleciona a lixeira
        app.update(key(KeyCode::Enter));
        assert!(app.folder_picker.is_none(), "picker fecha ao mover");
        match rx.try_recv().unwrap() {
            WorkerCmd::MoveEmail { id, target, .. } => {
                assert_eq!(id, "9");
                assert_eq!(target, "[Gmail]/Lixeira");
            }
            _ => panic!("esperava MoveEmail"),
        }
    }

    #[test]
    fn esc_cancels_folder_picker_without_command() {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(BubbleTheme::default(), tx);
        app.emails.set(Ok(vec![email("3")]));
        app.focus = Panel::Email;
        app.update(key(KeyCode::Char('m')));
        let _ = rx.try_recv(); // consome ListFolders
        app.update(key(KeyCode::Esc));
        assert!(app.folder_picker.is_none());
        assert!(rx.try_recv().is_err(), "nenhum MoveEmail deve ser enviado");
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
