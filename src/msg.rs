//! Mensagens que dirigem o `update` do app.

use ratatui::crossterm::event::KeyEvent;

use crate::data::{AgendaItem, EmailItem};

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
    /// Corpo de um e-mail aberto no overlay de detalhe.
    EmailBody(Result<String, String>),
}
