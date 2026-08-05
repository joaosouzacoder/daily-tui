//! Mensagens que dirigem o `update` do app.

use ratatui::crossterm::event::KeyEvent;

use crate::data::jira::JiraItem;
use crate::data::{AgendaItem, EmailItem, TaskItem};

/// Eventos processados pelo modelo. Precisa ser `Send + 'static` para
/// trafegar do worker para o loop principal via `ratatui_tea::channel`.
pub enum Msg {
    /// Pulso de relógio (1s) — força redesenho do header.
    ClockTick,
    /// Tecla pressionada.
    Key(KeyEvent),
    /// Resultado da busca de e-mails (já agregados e ordenados).
    EmailsLoaded(Result<Vec<EmailItem>, String>),
    /// Resultado da busca de agenda (já agregada e ordenada).
    AgendaLoaded(Result<Vec<AgendaItem>, String>),
    /// Resultado da busca de PRs/issues (linhas limpas do ghpending).
    PullsLoaded(Result<Vec<String>, String>),
    /// Resultado da busca de issues do Jira.
    JiraLoaded(Result<Vec<JiraItem>, String>),
    /// Resultado da busca de menções do Jira.
    JiraMentions(Result<Vec<JiraItem>, String>),
    /// Resultado da busca/escrita de tarefas (lista do mstodo, já atualizada).
    TasksLoaded(Result<Vec<TaskItem>, String>),
    /// Pastas de uma conta, para o seletor de "mover" (inclui as etiquetas).
    FoldersLoaded(crate::data::Account, Result<Vec<String>, String>),
    /// Corpo de um e-mail. Carrega a chave porque o corpo é buscado em segundo
    /// plano: quando chega, pode não ser mais o e-mail sob o cursor.
    EmailBody(crate::data::Account, String, Result<String, String>),
}
