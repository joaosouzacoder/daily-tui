//! Mensagens que dirigem o `update` do app.

use ratatui::crossterm::event::KeyEvent;

use crate::data::jira::JiraItem;
use crate::data::{AgendaItem, EmailItem, TaskItem};

/// Que escrita de e-mail terminou.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmailWriteKind {
    /// Marcar ou desmarcar como lido.
    Seen,
    /// Sair da pasta atual: mover ou excluir.
    Gone,
}

/// Eventos processados pelo modelo. Precisa ser `Send + 'static` para
/// trafegar do worker para o loop principal via `ratatui_tea::channel`.
///
/// `Clone` porque `ProgramHandle<Msg>` só ganha `Clone` quando `Msg` também
/// tem — o `derive` da lib exige o bound no parâmetro genérico mesmo o campo
/// interno (`Sender<M>`) não precisando dele. Sem isso, a notificação do
/// pomodoro não teria como rodar numa thread própria sem travar o worker
/// atrás dela.
#[derive(Clone)]
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
    /// Fim de uma escrita de e-mail (lido, mover, excluir): o tipo da escrita,
    /// os alvos, o erro se houve, e a lista do servidor depois dela. Vem tudo
    /// junto porque só quando o servidor responde é que a lista dele passa a
    /// valer mais do que a intenção já aplicada na tela.
    ///
    /// O `kind` existe porque o mesmo e-mail pode ter duas escritas na fila
    /// (marcado como lido e movido em seguida): encerrar a pendência errada
    /// ressuscitava a linha que ainda ia sair.
    EmailWrite {
        kind: EmailWriteKind,
        targets: Vec<(crate::data::Account, String)>,
        error: Option<String>,
        list: Result<Vec<EmailItem>, String>,
    },
    /// Resultado de abrir um e-mail no Gmail. Só interessa quando falha.
    EmailWebOpened(Result<(), String>),
    /// Corpo de um e-mail. Carrega a chave porque o corpo é buscado em segundo
    /// plano: quando chega, pode não ser mais o e-mail sob o cursor.
    EmailBody(crate::data::Account, String, Result<String, String>),
    /// Fim de um envio de notificação. Só interessa quando falha: achar que
    /// vai ser avisado e não ser é o pior defeito possível aqui.
    Notified(Result<(), String>),
    /// O worker começou um refresh completo.
    ///
    /// Vem dele porque o refresh periódico nasce do timeout do próprio
    /// `recv_timeout`: nada fora do worker sabe que ele começou.
    RefreshStarted,
    /// O worker terminou o refresh completo — todos os resultados de painel já
    /// foram enviados.
    ///
    /// Existe em par com o `RefreshStarted` em vez de o App deduzir o fim de
    /// "todos os painéis responderam": painel que falha não responde, e a
    /// dedução deixaria o indicador girando para sempre.
    RefreshDone,
}
