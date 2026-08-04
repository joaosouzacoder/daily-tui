//! Renderização: layout, painéis com rolagem, header do relógio e overlay.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui_bubbletea_theme::BubbleTheme;

use crate::ansi;
use crate::app::{App, InputKind, Panel, Prompt};
use crate::clock;
use chrono::Datelike;
use crate::data::jira::{self, JiraItem};
use crate::data::tasks::{self, SubTask};
use crate::data::{email, AgendaItem, TaskItem};

/// Calcula o deslocamento de rolagem para manter o cursor visível.
///
/// `prev` é o deslocamento anterior (rolagem suave). Função pura.
pub fn window(total: usize, cursor: usize, prev: usize, height: usize) -> usize {
    if total == 0 || height == 0 {
        return 0;
    }
    let max_off = total.saturating_sub(height);
    let mut off = prev.min(max_off);
    if cursor < off {
        off = cursor;
    } else if cursor >= off + height {
        off = cursor + 1 - height;
    }
    off.min(max_off)
}

/// Ponto de entrada de renderização.
pub fn render(app: &App, frame: &mut Frame<'_>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // header (relógio grande + data)
            Constraint::Min(0),    // corpo
            Constraint::Length(1), // footer (ajuda)
        ])
        .split(frame.area());

    render_header(app, frame, chunks[0]);
    render_body(app, frame, chunks[1]);
    render_footer(app, frame, chunks[2]);

    if app.detail.is_some() {
        render_detail(app, frame, frame.area());
    }
    if app.prompt.is_some() {
        render_prompt(app, frame, frame.area());
    }
}

fn render_header(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let time = clock::format_time(&app.now);
    let date = clock::format_date(&app.now);

    let block = theme.block();
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(clock::BIG_HEIGHT as u16),
            Constraint::Length(1),
        ])
        .split(inner);

    // Relógio em "fonte" grande (arte ASCII), centralizado e em destaque.
    let clock_style = theme.accent.add_modifier(Modifier::BOLD);
    let big: Vec<Line> = clock::big_glyphs(&time)
        .into_iter()
        .map(|r| Line::from(Span::styled(r, clock_style)))
        .collect();
    frame.render_widget(
        Paragraph::new(big).alignment(Alignment::Center),
        rows[0],
    );

    frame.render_widget(
        Paragraph::new(Line::from(theme.muted(date))).alignment(Alignment::Center),
        rows[1],
    );
}

fn render_body(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(cols[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ])
        .split(cols[1]);

    render_emails(app, frame, left[0]);
    render_jira(app, frame, left[1]);
    render_agenda(app, frame, right[0]);
    render_pulls(app, frame, right[1]);
    render_tasks(app, frame, right[2]);
}

fn render_emails(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.emails;
    let unread = p.items.iter().filter(|e| e.unread).count();
    let title = format!(" E-MAILS  {}/{} ", unread, p.items.len());
    let focused = app.focus == Panel::Email;

    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = p
        .items
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let bullet = if e.unread {
                theme.accent("●")
            } else {
                theme.muted("·")
            };
            let line = Line::from(vec![
                bullet,
                theme.span(" "),
                theme.muted(e.account.marker()),
                theme.span(" "),
                theme.span(clip(&e.from, 16)),
                theme.muted(" — "),
                theme.span(clip(&e.subject, 60)),
            ]);
            highlight(line, theme, selected == Some(i))
        })
        .collect();

    let inner = panel_inner(frame, theme, area, title, focused);
    if render_empty_state(frame, app, inner, p) {
        return;
    }
    let inner = reserve_error_banner(frame, theme, inner, p);
    // Segue o cursor (seleção).
    let height = inner.height as usize;
    let off = window(lines.len(), p.cursor, p.offset.get(), height);
    p.offset.set(off);
    render_lines(frame, theme, inner, lines, off);
}

fn render_agenda(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.agenda;
    let title = format!(" AGENDA  {} ", p.items.len());
    let focused = app.focus == Panel::Agenda;

    let lines = build_agenda_lines(&p.items, theme);

    let inner = panel_inner(frame, theme, area, title, focused);
    if render_empty_state(frame, app, inner, p) {
        return;
    }
    let inner = reserve_error_banner(frame, theme, inner, p);
    render_scrolled(frame, theme, inner, lines, &p.scroll);
}

fn render_pulls(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.pulls;
    let title = " PRs (ghpending) ".to_string();
    let focused = app.focus == Panel::Pulls;

    // Reaplica as cores ANSI que o ghpending emite.
    let lines: Vec<Line> = p.items.iter().map(|l| ansi::to_line(l, theme.text)).collect();

    let inner = panel_inner(frame, theme, area, title, focused);
    if render_empty_state(frame, app, inner, p) {
        return;
    }
    let inner = reserve_error_banner(frame, theme, inner, p);
    render_scrolled(frame, theme, inner, lines, &p.scroll);
}

fn render_jira(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let (p, rows) = match app.jira_view {
        jira::JiraView::Issues => (&app.jira, jira::rows_by_project(&app.jira.items)),
        jira::JiraView::ByParent => (&app.jira, jira::rows_by_parent(&app.jira.items)),
        jira::JiraView::Mentions => (
            &app.jira_mentions,
            jira::rows_by_project(&app.jira_mentions.items),
        ),
    };
    let title = format!(
        " JIRA · {} · {} ",
        app.jira_filter.label(),
        match app.jira_view {
            jira::JiraView::Issues => "[issues] por-pai menções",
            jira::JiraView::ByParent => "issues [por-pai] menções",
            jira::JiraView::Mentions => "issues por-pai [menções]",
        }
    );
    let focused = app.focus == Panel::Jira;
    // O papel só explica algo no filtro `ambas`: nos outros, toda issue tem o
    // mesmo papel; em menções, a issue está ali por citação, não por papel.
    let show_role = app.jira_filter == jira::JiraFilter::Both && app.jira_view != jira::JiraView::Mentions;

    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = rows
        .iter()
        .map(|row| match row {
            jira::JiraRow::Header(h) => {
                Line::from(Span::styled(h.clone(), theme.accent.add_modifier(Modifier::BOLD)))
            }
            jira::JiraRow::Issue(i) => {
                let item = &p.items[*i];
                highlight(issue_line(item, theme, show_role), theme, selected == Some(*i))
            }
        })
        .collect();

    let inner = panel_inner(frame, theme, area, title, focused);
    if render_empty_state(frame, app, inner, p) {
        return;
    }
    let inner = reserve_error_banner(frame, theme, inner, p);
    // A rolagem segue o cursor, mas em linhas — o cursor indexa issues.
    let height = inner.height as usize;
    let cursor_row = jira::row_of_item(&rows, p.cursor);
    let off = window(lines.len(), cursor_row, p.offset.get(), height);
    p.offset.set(off);
    render_lines(frame, theme, inner, lines, off);
}

/// Linha de uma issue: chave, status esmaecido, papel (opcional) e resumo.
///
/// `show_role` só é verdadeiro no filtro `ambas` fora da visão de menções —
/// nos outros casos o papel não distingue nada, e mostrá-lo seria ruído.
fn issue_line(item: &JiraItem, theme: &BubbleTheme, show_role: bool) -> Line<'static> {
    let mut spans = vec![
        theme.span("  "),
        theme.accent(item.key.clone()),
        theme.muted(format!(" [{}] ", item.status)),
    ];
    // O marcador ("[AR] ") ocupa 5 colunas; o clip do resumo encurta na mesma
    // medida para a linha não vazar do painel.
    let summary_width = if show_role {
        spans.push(theme.muted(format!("{} ", item.role.marker())));
        44 - 5
    } else {
        44
    };
    spans.push(theme.span(clip(&item.summary, summary_width)));
    Line::from(spans)
}

fn render_tasks(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.tasks;
    let pending = p.items.iter().filter(|t| !t.completed).count();
    let title = format!(" TAREFAS  {}/{} ", pending, p.items.len());
    let focused = app.focus == Panel::Tasks;

    // O cursor indexa linhas, e uma linha é uma tarefa ou uma subtarefa de uma
    // tarefa expandida — por isso o achatamento vem antes da renderização.
    let rows = tasks::rows(&p.items, &app.tasks_expanded);
    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(row, kind)| {
            let line = match kind {
                tasks::TaskRow::Task(t) => {
                    let item = &p.items[*t];
                    task_line(item, theme, app.tasks_expanded.contains(&item.id))
                }
                tasks::TaskRow::Sub { task, sub } => {
                    subtask_line(&p.items[*task].subtasks[*sub], theme)
                }
            };
            highlight(line, theme, selected == Some(row))
        })
        .collect();

    let inner = panel_inner(frame, theme, area, title, focused);
    if render_empty_state(frame, app, inner, p) {
        return;
    }
    let inner = reserve_error_banner(frame, theme, inner, p);
    // Segue o cursor (seleção), igual ao painel de e-mails.
    let height = inner.height as usize;
    let off = window(lines.len(), p.cursor, p.offset.get(), height);
    p.offset.set(off);
    render_lines(frame, theme, inner, lines, off);
}

/// Linha de uma tarefa: marca de expansão + checkbox + título + prazo.
///
/// A marca indica que há subtarefas escondidas: `▸` recolhida, `▾` expandida.
/// Tarefa sem subtarefas não recebe marca, para não prometer o que não existe.
fn task_line(t: &TaskItem, theme: &BubbleTheme, expanded: bool) -> Line<'static> {
    let mark = if t.subtasks.is_empty() {
        "  "
    } else if expanded {
        "▾ "
    } else {
        "▸ "
    };
    let mut spans = if t.completed {
        vec![
            theme.muted(mark),
            theme.muted("[x] "),
            theme.muted(clip(&t.title, 38)),
        ]
    } else {
        vec![
            theme.muted(mark),
            theme.span("[ ] "),
            theme.span(clip(&t.title, 38)),
        ]
    };
    if !t.due.is_empty() {
        spans.push(theme.muted("  "));
        spans.push(theme.accent(short_date(&t.due)));
    }
    Line::from(spans)
}

/// Linha de uma subtarefa: indentada sob a tarefa, com o mesmo checkbox.
fn subtask_line(s: &SubTask, theme: &BubbleTheme) -> Line<'static> {
    if s.completed {
        Line::from(vec![
            theme.muted("      [x] "),
            theme.muted(clip(&s.title, 34)),
        ])
    } else {
        Line::from(vec![
            theme.span("      [ ] "),
            theme.span(clip(&s.title, 34)),
        ])
    }
}

/// Overlay do prompt de tarefa (entrada de texto ou confirmação de exclusão).
fn render_prompt(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let Some(prompt) = &app.prompt else { return };

    let (title, lines): (String, Vec<Line>) = match prompt {
        Prompt::Input { kind, buffer } => {
            let title = match kind {
                InputKind::AddTask => " Nova tarefa ".to_string(),
                InputKind::EditTask { .. } => " Editar tarefa ".to_string(),
            };
            let input = Line::from(vec![theme.span(buffer.clone()), theme.accent("█")]);
            let help = Line::from(theme.muted("Enter: salvar · Esc: cancelar"));
            (title, vec![input, Line::from(""), help])
        }
        Prompt::ConfirmDelete { title, .. } => (
            " Apagar tarefa ".to_string(),
            vec![
                Line::from(vec![
                    theme.muted("Apagar \""),
                    theme.span(clip(title, 40)),
                    theme.muted("\"?"),
                ]),
                Line::from(""),
                Line::from(theme.muted("y: confirmar · n/Esc: cancelar")),
            ],
        ),
        Prompt::PickFolder { cursor, .. } => {
            let mut lines = vec![Line::from(theme.muted("Mover para:")), Line::from("")];
            lines.extend(email::FOLDERS.iter().enumerate().map(|(i, folder)| {
                highlight(
                    Line::from(vec![theme.span(format!("  {folder}"))]),
                    theme,
                    i == *cursor,
                )
            }));
            lines.push(Line::from(""));
            lines.push(Line::from(theme.muted(
                "j/k: escolher · Enter: mover · Esc: cancelar",
            )));
            (" Mover e-mail ".to_string(), lines)
        }
        Prompt::ConfirmEmailDelete { subject, .. } => (
            " Excluir e-mail ".to_string(),
            vec![
                Line::from(vec![
                    theme.muted("Mover para a Lixeira: \""),
                    theme.span(clip(subject, 36)),
                    theme.muted("\"?"),
                ]),
                Line::from(""),
                Line::from(theme.muted("y: confirmar · n/Esc: cancelar")),
            ],
        ),
    };

    // O seletor de pasta tem 10 linhas (título, seis pastas, espaçamento, ajuda);
    // os outros prompts cabem em 24% da tela.
    let height = if matches!(prompt, Prompt::PickFolder { .. }) { 46 } else { 24 };
    let popup = centered_rect(60, height, area);
    frame.render_widget(Clear, popup);
    let block = theme.titled_modal_block(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    frame.render_widget(theme.paragraph(Text::from(lines)).wrap(Wrap { trim: true }), inner);
}

/// Monta as linhas da agenda agrupadas por data → hora → eventos:
///
/// ```text
/// 09/06
///    10:00
///       - Evento
///       - Evento 2
/// ```
fn build_agenda_lines(items: &[AgendaItem], theme: &BubbleTheme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cur_date: Option<String> = None;
    let mut cur_time: Option<String> = None;

    for a in items {
        if cur_date.as_deref() != Some(a.date.as_str()) {
            cur_date = Some(a.date.clone());
            cur_time = None;
            lines.push(Line::from(Span::styled(
                short_date(&a.date),
                theme.accent.add_modifier(Modifier::BOLD),
            )));
        }
        let time_label = if a.all_day() {
            "dia inteiro".to_string()
        } else {
            a.time.clone()
        };
        if cur_time.as_deref() != Some(time_label.as_str()) {
            cur_time = Some(time_label.clone());
            lines.push(Line::from(vec![theme.muted("   "), theme.accent(time_label)]));
        }
        lines.push(Line::from(vec![
            theme.muted("      - "),
            theme.span(clip(&a.title, 46)),
            theme.muted(" "),
            theme.muted(a.account.marker()),
        ]));
    }
    lines
}

/// Desenha a borda/título do painel e devolve a área interna.
fn panel_inner(frame: &mut Frame<'_>, theme: &BubbleTheme, area: Rect, title: String, focused: bool) -> Rect {
    let block = theme.block_with_focus(focused).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// Renderiza o estado vazio do painel (spinner/erro/vazio). Devolve `true` se
/// renderizou algo (ou seja, não há lista a mostrar).
fn render_empty_state<T>(frame: &mut Frame<'_>, app: &App, inner: Rect, p: &crate::app::PanelData<T>) -> bool {
    if !p.items.is_empty() {
        return false;
    }
    let theme = &app.theme;
    if !p.loaded {
        frame.render_widget(&app.spinner, inner);
    } else if let Some(err) = &p.error {
        frame.render_widget(
            theme.paragraph(theme.error(format!("erro: {err}"))).wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        frame.render_widget(theme.paragraph(theme.muted("(vazio)")), inner);
    }
    true
}

/// Quando o painel já tem itens (dados de uma busca anterior) mas a busca mais
/// recente falhou, reserva uma linha compacta no topo para o erro, sem
/// esconder a lista nem empurrá-la para fora da área visível. Sem erro,
/// devolve `inner` sem alterar — é o caminho comum, sem custo.
fn reserve_error_banner<T>(frame: &mut Frame<'_>, theme: &BubbleTheme, inner: Rect, p: &crate::app::PanelData<T>) -> Rect {
    let Some(err) = &p.error else { return inner };
    if inner.height == 0 {
        return inner;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(theme.error(format!("⚠ {err}")))),
        rows[0],
    );
    rows[1]
}

/// Renderiza `lines` a partir do deslocamento `off`.
fn render_lines(frame: &mut Frame<'_>, theme: &BubbleTheme, inner: Rect, lines: Vec<Line>, off: usize) {
    let height = inner.height as usize;
    let visible: Vec<Line> = lines.into_iter().skip(off).take(height).collect();
    frame.render_widget(theme.paragraph(Text::from(visible)), inner);
}

/// Rolagem livre (sem seleção): clampa o `scroll` ao máximo e reescreve.
fn render_scrolled(frame: &mut Frame<'_>, theme: &BubbleTheme, inner: Rect, lines: Vec<Line>, scroll: &std::cell::Cell<usize>) {
    let height = inner.height as usize;
    let max_off = lines.len().saturating_sub(height);
    let off = scroll.get().min(max_off);
    scroll.set(off);
    render_lines(frame, theme, inner, lines, off);
}

/// Teclas específicas do painel em foco, para as ações serem descobríveis.
///
/// Só lista teclas que realmente existem em `handle_panel_key` hoje.
fn panel_hints(focus: Panel) -> &'static str {
    match focus {
        Panel::Email => "espaço lido · m move · d exclui",
        Panel::Jira => "f filtro · p por-pai · n menções · esc volta",
        Panel::Tasks => "enter expande · espaço alterna · a nova · e edita · d apaga",
        _ => "",
    }
}

fn render_footer(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(22)])
        .split(area);

    let help = if app.detail.is_some() {
        theme.help_line([("j/k", "rolar"), ("Esc", "voltar")])
    } else if app.prompt.is_some() {
        theme.help_line([("Enter/y", "confirmar"), ("Esc/n", "cancelar")])
    } else if app.focus == Panel::Tasks {
        theme.help_line([
            ("Espaço", "concluir"),
            ("a", "nova"),
            ("e", "editar"),
            ("d", "apagar"),
            ("Tab", "painel"),
        ])
    } else {
        let hints = panel_hints(app.focus);
        if hints.is_empty() {
            theme.help_line([
                ("Tab", "painel"),
                ("j/k", "rolar"),
                ("Enter", "abrir"),
                ("r", "atualizar"),
                ("q", "sair"),
            ])
        } else {
            // As teclas do painel vêm primeiro: são as menos óbvias, e é por elas
            // que este trecho existe. O texto global encurta para o essencial —
            // `j/k` e `Enter` são o que qualquer um tenta primeiro, enquanto sair
            // do programa é a única tecla que precisa estar sempre visível.
            let mut spans = vec![theme.span(hints), theme.muted(" · ")];
            spans.extend(
                theme
                    .help_line([("Tab", "painel"), ("r", "atualizar"), ("q", "sair")])
                    .spans,
            );
            Line::from(spans)
        }
    };
    frame.render_widget(Paragraph::new(help), cols[0]);

    let status = match app.last_refresh {
        Some(t) => Line::from(theme.muted(format!("⟳ {}", clock::format_time(&t)))),
        None => Line::from(theme.muted("⟳ …")),
    };
    frame.render_widget(Paragraph::new(status).alignment(Alignment::Right), cols[1]);
}

fn render_detail(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let Some(d) = &app.detail else { return };

    let popup = centered_rect(80, 80, area);
    frame.render_widget(Clear, popup);

    let block = theme.titled_modal_block(format!(" {} ", clip(&d.subject, 60)));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![theme.muted("De: "), theme.span(d.from.clone())])),
        rows[0],
    );

    match &d.body {
        None => frame.render_widget(&app.spinner, rows[1]),
        Some(Err(e)) => frame.render_widget(
            theme.paragraph(theme.error(format!("erro: {e}"))).wrap(Wrap { trim: true }),
            rows[1],
        ),
        Some(Ok(body)) => {
            let lines: Vec<Line> = body.lines().map(|l| Line::from(theme.span(l.to_string()))).collect();
            let height = rows[1].height as usize;
            let max_off = lines.len().saturating_sub(height);
            let off = d.scroll.min(max_off);
            let visible: Vec<Line> = lines.into_iter().skip(off).take(height).collect();
            frame.render_widget(theme.paragraph(Text::from(visible)), rows[1]);
        }
    }
}

/// Estiliza uma linha como selecionada (fundo destacado) quando `on`.
fn highlight<'a>(line: Line<'a>, theme: &BubbleTheme, on: bool) -> Line<'a> {
    if on {
        line.style(theme.selected)
    } else {
        line
    }
}

/// Trunca uma string para `max` caracteres, com reticências.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// "2026-06-12" -> "12/06". Devolve a entrada se o formato não bater.
fn short_date(iso: &str) -> String {
    let parts: Vec<&str> = iso.split('-').collect();
    if parts.len() == 3 {
        let day = parts[2];
        let ddmm = format!("{}/{}", day, parts[1]);
        // O dia pode vir com hora colada ("09 10:00+00:00"); usa só os 10 primeiros chars.
        match chrono::NaiveDate::parse_from_str(&iso[..iso.len().min(10)], "%Y-%m-%d") {
            Ok(d) => format!("{} - {}", ddmm, clock::weekday_short_ptbr(d.weekday())),
            Err(_) => ddmm,
        }
    } else {
        iso.to_string()
    }
}

/// Retângulo centralizado com `pct_x`% × `pct_y`% da área.
fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .split(vert[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, Detail};
    use crate::data::{Account, AgendaItem, EmailItem};
    use crate::msg::Msg;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui_tea::Model;
    use std::sync::mpsc;

    fn test_app() -> App {
        let (tx, _rx) = mpsc::channel();
        App::new(BubbleTheme::default(), tx)
    }

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    fn render_to_string(app: &App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(app, f)).unwrap();
        let buf = terminal.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_titles_and_help_without_panicking() {
        let app = test_app();
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("E-MAILS"));
        assert!(out.contains("AGENDA"));
        assert!(out.contains("PRs"));
        assert!(out.contains("sair"));
    }

    #[test]
    fn renders_populated_panels() {
        let mut app = test_app();
        app.emails.items = vec![EmailItem {
            id: "1".into(),
            account: Account::Work,
            from: "Thiago".into(),
            subject: "assunto importante".into(),
            unread: true,
            date: "2026-06-09 10:00+00:00".into(),
        }];
        app.emails.loaded = true;
        app.agenda.items = vec![AgendaItem {
            account: Account::Personal,
            date: "2026-06-12".into(),
            time: String::new(),
            title: "Dia dos Namorados".into(),
        }];
        app.agenda.loaded = true;
        app.pulls.items = vec!["#12 fix algo".into()];
        app.pulls.loaded = true;

        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("Thiago"));
        assert!(out.contains("12/06"));
        assert!(out.contains("#12 fix algo"));
    }

    #[test]
    fn renders_detail_overlay() {
        let mut app = test_app();
        app.detail = Some(Detail {
            from: "Fulano".into(),
            subject: "Reunião".into(),
            body: Some(Ok("corpo do email\nlinha 2".into())),
            scroll: 0,
        });
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("Reunião"));
        assert!(out.contains("corpo do email"));
        assert!(out.contains("voltar")); // ajuda muda no modo detalhe
    }

    #[test]
    fn agenda_lines_group_by_date_then_time() {
        let theme = BubbleTheme::default();
        let mk = |date: &str, time: &str, title: &str, acc| AgendaItem {
            account: acc,
            date: date.into(),
            time: time.into(),
            title: title.into(),
        };
        let items = vec![
            mk("2026-06-09", "", "Escritório", Account::Work),
            mk("2026-06-09", "10:00", "Daily", Account::Work),
            mk("2026-06-09", "10:00", "Outro", Account::Work),
            mk("2026-06-10", "14:00", "Call", Account::Personal),
        ];
        let lines: Vec<String> = build_agenda_lines(&items, &theme)
            .iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(
            lines,
            vec![
                "09/06 - Terça",
                "   dia inteiro",
                "      - Escritório [W]",
                "   10:00",
                "      - Daily [W]",
                "      - Outro [W]",
                "10/06 - Quarta",
                "   14:00",
                "      - Call [P]",
            ]
        );
    }

    #[test]
    fn jira_panel_renders_filter_label_and_groups_by_project() {
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-101","summary":"Melhorias no dashboard","status":"Em andamento",
                 "project":"ENG","url":"u","parent":{"key":"ENG-1","summary":"Eng"}}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("JIRA · minhas"), "cabeçalho com o filtro ativo");
        assert!(out.contains("ENG"), "cabeçalho de grupo do projeto");
        assert!(out.contains("ENG-101"), "a chave da issue");
    }

    #[test]
    fn jira_header_marks_the_active_view() {
        let mut app = test_app();
        app.jira.loaded = true;
        app.jira_view = crate::data::jira::JiraView::ByParent;
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("[por-pai]"), "a visão ativa aparece entre colchetes");
    }

    #[test]
    fn footer_shows_the_keys_of_the_focused_panel() {
        let mut app = test_app();
        app.update(key(KeyCode::Tab)); // Email -> Jira
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("f filtro"), "o footer anuncia as teclas do Jira");
        assert!(out.contains("n menções"));
    }

    #[test]
    fn panel_error_is_shown_even_when_it_still_has_items() {
        // Reproduz o cenário do `open_url` falhando (ou de qualquer refresh
        // que falhe com dados antigos ainda na tela): o painel não deve
        // esconder o erro nem esconder a lista atrás dele.
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-101","summary":"Melhorias no dashboard","status":"Em andamento",
                 "project":"ENG","url":"u","parent":null}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        app.jira.error = Some("falha ao abrir o navegador: not found".into());

        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("falha ao abrir o navegador"), "o erro precisa aparecer");
        assert!(out.contains("ENG-101"), "a lista não pode sumir atrás do erro");
    }

    #[test]
    fn tasks_panel_renders_checkbox_and_titles() {
        let mut app = test_app();
        app.tasks.items = vec![
            TaskItem { id: "1".into(), title: "Comprar café".into(), completed: false, due: "2026-06-10".into(), notes: String::new(), subtasks: Vec::new() },
            TaskItem { id: "2".into(), title: "Já feito".into(), completed: true, due: String::new(), notes: String::new(), subtasks: Vec::new() },
        ];
        app.tasks.loaded = true;
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("TAREFAS"));
        assert!(out.contains("[ ] Comprar café"));
        assert!(out.contains("[x] Já feito"));
        assert!(out.contains("10/06")); // prazo formatado
    }

    #[test]
    fn prompt_overlay_renders_input_buffer() {
        let mut app = test_app();
        app.prompt = Some(Prompt::Input { kind: InputKind::AddTask, buffer: "nova tarefa".into() });
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("Nova tarefa"));
        assert!(out.contains("nova tarefa"));
    }

    #[test]
    fn prompt_overlay_renders_delete_confirmation() {
        let mut app = test_app();
        app.prompt = Some(Prompt::ConfirmDelete { id: "1".into(), title: "apagar isto".into() });
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("Apagar tarefa"));
        assert!(out.contains("apagar isto"));
    }

    #[test]
    fn pulls_panel_renders_ansi_colors() {
        let mut app = test_app();
        app.pulls.items = vec!["\x1b[36m\x1b[1mrepo/name\x1b[0m".into()];
        app.pulls.loaded = true;
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let has_cyan = (0..30)
            .any(|y| (0..60).any(|x| buf[(x, y)].fg == ratatui::style::Color::Cyan));
        assert!(has_cyan, "o nome do repo deve ser renderizado em ciano");
    }

    #[test]
    fn renders_in_tiny_terminal_without_panicking() {
        // Garante que a matemática de layout/janela não estoura em telas mínimas.
        let app = test_app();
        let _ = render_to_string(&app, 10, 6);
        let _ = render_to_string(&app, 1, 1);
    }

    #[test]
    fn window_keeps_cursor_visible_scrolling_down() {
        // 20 itens, altura 5, cursor no fim -> mostra o fim.
        assert_eq!(window(20, 19, 0, 5), 15);
        // cursor logo abaixo da janela atual avança 1.
        assert_eq!(window(20, 5, 0, 5), 1);
    }

    #[test]
    fn window_keeps_cursor_visible_scrolling_up() {
        // janela em 10, cursor sobe para 3 -> offset acompanha.
        assert_eq!(window(20, 3, 10, 5), 3);
    }

    #[test]
    fn window_no_scroll_when_everything_fits() {
        assert_eq!(window(3, 2, 0, 10), 0);
    }

    #[test]
    fn window_handles_empty_and_zero_height() {
        assert_eq!(window(0, 0, 0, 5), 0);
        assert_eq!(window(10, 5, 0, 0), 0);
    }

    #[test]
    fn short_date_formats_ddmm() {
        assert_eq!(short_date("2026-06-12"), "12/06 - Sexta");
        assert_eq!(short_date("invalid"), "invalid");
    }

    #[test]
    fn clip_truncates_with_ellipsis() {
        assert_eq!(clip("hello", 10), "hello");
        assert_eq!(clip("hello world", 5), "hell…");
    }
}
