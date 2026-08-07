//! Renderização: layout, painéis com rolagem, header do relógio e overlay.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui_bubbletea_theme::BubbleTheme;

use crate::ansi;
use crate::app::{App, InputKind, Panel, Prompt, TaskField, TaskForm};
use crate::clock;
use chrono::{DateTime, Datelike, Local};
use crate::data::jira::{self, JiraItem};
use crate::data::tasks::{self, SubTask};
use crate::data::{AgendaItem, TaskItem};

/// Largura que sobra para o campo flexível de uma linha.
///
/// Os painéis tinham larguras fixas (assunto em 58, resumo em 44, título em 38),
/// o que cortava texto mesmo com a tela larga. Aqui a sobra vem da largura real
/// do painel, com um mínimo legível para não virar reticências em tela estreita.
fn room_for(avail: usize, fixed: usize) -> usize {
    avail.saturating_sub(fixed).max(12)
}

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
    if app.notifications.is_some() {
        render_notifications(app, frame, frame.area());
    }
}

/// Largura da caixa do pomodoro. Cabe `Descanso` + contador na linha de cima e
/// `P pausar · R zerar` embaixo, que é a linha mais larga.
const POMODORO_WIDTH: u16 = 22;

/// Largura mínima do header inteiro para a caixa do pomodoro aparecer.
///
/// O relógio grande sozinho precisa de uns 29 colunas (`HH:MM:SS`: seis
/// dígitos de 3 colunas, dois `:` de 1, espaçamento entre glifos e a borda do
/// bloco) — some `POMODORO_WIDTH` e o mínimo real já passa de 50. 60 dá uma
/// folga sem depender do segundo exato do relógio: abaixo disso, a caixa cede
/// o espaço de volta para o widget que o header existe para mostrar.
const MIN_WIDTH_FOR_POMODORO: u16 = 60;

fn render_header(app: &App, frame: &mut Frame<'_>, area: Rect) {
    // Caixa desligada, ou terminal estreito demais para as duas coisas,
    // devolve a largura inteira ao relógio. `app.pomodoro_enabled` é a mesma
    // fonte de verdade que trava o tick e as teclas em `app.rs` — ler o config
    // aqui de novo poderia divergir e mostrar uma caixa que o estado diz que
    // está desligada.
    let (clock_area, pomodoro_area) =
        if app.pomodoro_enabled && area.width >= MIN_WIDTH_FOR_POMODORO {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(POMODORO_WIDTH)])
                .split(area);
            (cols[0], Some(cols[1]))
        } else {
            (area, None)
        };

    render_clock(app, frame, clock_area);
    if let Some(rect) = pomodoro_area {
        render_pomodoro(app, frame, rect);
    }
}

fn render_clock(app: &App, frame: &mut Frame<'_>, area: Rect) {
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
    frame.render_widget(Paragraph::new(big).alignment(Alignment::Center), rows[0]);

    frame.render_widget(
        Paragraph::new(Line::from(theme.muted(date))).alignment(Alignment::Center),
        rows[1],
    );
}

/// Tempo restante como `MM:SS`. Passando de uma hora segue contando em
/// minutos: um pomodoro de 75 minutos é `75:00`, e um campo de hora só gastaria
/// largura numa caixa que não tem.
fn format_left(left: Duration) -> String {
    let secs = left.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Barra do decorrido, com exatamente `width` colunas.
fn progress_bar(elapsed: Duration, total: Duration, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if total.is_zero() {
        return "░".repeat(width);
    }
    let ratio = (elapsed.as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0);
    let filled = ((ratio * width as f64).round() as usize).min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Contador de focos fechados, pronto para a linha de cabeçalho da caixa.
/// Zero focos não mostra nada: `0 ✓` num pomodoro que nem começou é ruído.
fn done_counter(done: u32) -> String {
    if done > 0 {
        format!("{} ✓", done)
    } else {
        String::new()
    }
}

fn render_pomodoro(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.pomodoro;
    let now = std::time::Instant::now();
    let left = p.remaining(now);

    // Nunca focado: o pomodoro não é painel, e `Tab` não passa por ele.
    let inner = panel_inner(frame, theme, area, " POMODORO ".to_string(), false);
    let width = inner.width as usize;

    // Fase à esquerda, focos fechados à direita.
    // Sem sufixo de "parado": a linha de dicas já diz `P iniciar` quando
    // parado e `P pausar` quando rodando — repetir o estado aqui só encurtava
    // a folga da caixa contra o contador de dois dígitos (revisão do task 5).
    let phase = p.phase().label().to_string();
    let count = done_counter(p.done());
    let gap = width.saturating_sub(phase.chars().count() + count.chars().count());
    let head = Line::from(vec![
        // Mesmo destaque do relógio e dos cabeçalhos do Jira: o tema expõe
        // `accent` como `Style`, não como construtor de `Span`.
        Span::styled(phase, theme.accent.add_modifier(Modifier::BOLD)),
        theme.muted(" ".repeat(gap)),
        theme.muted(count),
    ]);

    // A linha de dicas é o único sinal de rodando/parado (o sufixo `(parado)`
    // na fase foi removido de propósito) — ela não pode sumir, nem quando o
    // aviso falha. O aviso ocupa a linha em branco entre a fase e o tempo, que
    // não carrega informação nenhuma quando não há nada a dizer.
    let warning = match &app.notify_error {
        Some(_) => Line::from(theme.error("⚠ aviso não saiu")),
        None => Line::from(""),
    };
    let hint = Line::from(theme.muted(format!(
        "P {} · R zerar",
        if p.running() { "pausar" } else { "iniciar" }
    )));

    let lines = vec![
        head,
        warning,
        Line::from(theme.span(format_left(left))),
        Line::from(theme.muted(progress_bar(
            p.total().saturating_sub(left),
            p.total(),
            width,
        ))),
        hint,
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Em que coluna cada painel mora e com que peso.
///
/// Os pesos são as proporções de sempre (60/40 à esquerda, 40/30/30 à direita).
/// Com painel desligado eles são normalizados sobre os que sobraram — é isso, e
/// só isso, que faz o layout se redistribuir sozinho.
const LAYOUT: [(Panel, bool, u16); 5] = [
    (Panel::Email, true, 60),
    (Panel::Jira, true, 40),
    (Panel::Agenda, false, 40),
    (Panel::Pulls, false, 30),
    (Panel::Tasks, false, 30),
];

/// Painéis de uma coluna, cada um com o seu peso na altura.
pub type Column = Vec<(Panel, u16)>;

/// Divide os painéis ligados nas duas colunas, cada um com seu peso.
/// Função pura: é o que dá para testar sem desenhar.
pub fn columns(on: &[Panel]) -> (Column, Column) {
    let mut left: Column = Vec::new();
    let mut right: Column = Vec::new();
    for (panel, is_left, weight) in LAYOUT {
        if !on.contains(&panel) {
            continue;
        }
        if is_left {
            left.push((panel, weight));
        } else {
            right.push((panel, weight));
        }
    }
    (left, right)
}

fn render_body(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let (left, right) = columns(&app.panels);
    // Coluna vazia não fica com espaço reservado: a outra recebe a tela inteira.
    let (left_area, right_area) = match (left.is_empty(), right.is_empty()) {
        (false, false) => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            (cols[0], cols[1])
        }
        (true, false) => (Rect::ZERO, area),
        (false, true) => (area, Rect::ZERO),
        // Config sem painel nenhum é recusado no arranque; aqui só não desenha.
        (true, true) => return,
    };

    for (panel, rect) in stack(&left, left_area)
        .into_iter()
        .chain(stack(&right, right_area))
    {
        match panel {
            Panel::Email => render_emails(app, frame, rect),
            Panel::Jira => render_jira(app, frame, rect),
            Panel::Agenda => render_agenda(app, frame, rect),
            Panel::Pulls => render_pulls(app, frame, rect),
            Panel::Tasks => render_tasks(app, frame, rect),
        }
    }
}

/// Empilha os painéis de uma coluna, dividindo a altura pelos pesos.
fn stack(panels: &[(Panel, u16)], area: Rect) -> Vec<(Panel, Rect)> {
    if panels.is_empty() {
        return Vec::new();
    }
    let constraints: Vec<Constraint> = panels
        .iter()
        .map(|(_, weight)| Constraint::Fill(*weight))
        .collect();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    panels
        .iter()
        .zip(rows.iter())
        .map(|((panel, _), rect)| (*panel, *rect))
        .collect()
}

fn render_emails(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.emails;
    let unread = p.items.iter().filter(|e| e.unread).count();
    let title = if app.emails_marked.is_empty() {
        format!(" E-MAILS  {}/{} ", unread, p.items.len())
    } else {
        format!(" E-MAILS  {}/{} · {} marcados ", unread, p.items.len(), app.emails_marked.len())
    };
    let focused = app.focus == Panel::Email;

    let inner_probe = area.width.saturating_sub(2) as usize; // bordas do painel
    let from_w = (inner_probe / 5).clamp(10, 24);
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
            // Marcado para ação em lote: o ✓ vem antes de tudo, para a faixa
            // marcada ser lida de relance na coluna da esquerda.
            let mark = if app.emails_marked.contains(&(e.account, e.id.clone())) {
                theme.accent("✓ ")
            } else {
                theme.span("  ")
            };
            let line = Line::from(vec![
                mark,
                bullet,
                theme.span(" "),
                theme.muted(e.account.marker()),
                theme.span(" "),
                theme.span(clip(&e.from, from_w)),
                theme.muted(" — "),
                theme.span(clip(
                    &e.subject,
                    room_for(inner_probe, 2 + 1 + 1 + 3 + 1 + from_w + 3),
                )),
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

    let lines = build_agenda_lines(&p.items, theme, area.width.saturating_sub(2) as usize);

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
    };
    let title = format!(
        " JIRA · {} · {} ",
        app.jira_filter.label(),
        match app.jira_view {
            jira::JiraView::Issues => "[issues] por-pai",
            jira::JiraView::ByParent => "issues [por-pai]",
        }
    );
    let focused = app.focus == Panel::Jira;
    let avail = area.width.saturating_sub(2) as usize;
    // O papel só explica algo no filtro `ambas`: nos outros, toda issue tem o
    // mesmo papel; em menções, a issue está ali por citação, não por papel.
    let show_role = app.jira_filter == jira::JiraFilter::Both;

    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = rows
        .iter()
        .map(|row| match row {
            jira::JiraRow::Header(h) => {
                Line::from(Span::styled(h.clone(), theme.accent.add_modifier(Modifier::BOLD)))
            }
            jira::JiraRow::Issue(i) => {
                let item = &p.items[*i];
                let nested = jira::is_nested(&p.items, *i);
                highlight(
                    issue_line(item, theme, show_role, nested, avail),
                    theme,
                    selected == Some(*i),
                )
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
fn issue_line(
    item: &JiraItem,
    theme: &BubbleTheme,
    show_role: bool,
    nested: bool,
    avail: usize,
) -> Line<'static> {
    // Subtarefa entra deslocada, embaixo do pai — a indentação é o que diz que
    // ela pertence à linha de cima em vez de disputar o mesmo nível.
    let indent = if nested { "     " } else { "  " };
    // O tipo vem antes da chave: é o que diz se a linha é uma história do dia a
    // dia ou uma iniciativa acima dela, e lido em coluna se compara de relance.
    let kind = item.type_marker();
    let mut spans = vec![theme.muted(format!("{indent}{kind} "))];
    // No filtro `ambas`, marca só o que **não** é seu para fazer: sem marcador
    // significa "é sua". `REL` vem antes da chave, em verde — a mesma marca do
    // `jirapending`, e não `[rel]`: some por completo quando você também é
    // responsável, em vez de disputar espaço com o tipo e o status entre colchetes.
    let role = if show_role && item.role == jira::JiraRole::Reporter {
        spans.push(theme.success("REL "));
        4
    } else {
        0
    };
    spans.push(theme.accent(item.key.clone()));
    spans.push(theme.muted(format!(" [{}] ", item.status)));
    let summary_width = room_for(
        avail,
        indent.chars().count()
            + kind.chars().count()
            + 1
            + item.key.chars().count()
            + item.status.chars().count()
            + 4
            + role,
    );
    spans.push(theme.span(clip(&item.summary, summary_width)));
    Line::from(spans)
}

fn render_tasks(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.tasks;
    let pending = p.items.iter().filter(|t| !t.completed).count();
    let title = format!(" TAREFAS  {}/{} ", pending, p.items.len());
    let focused = app.focus == Panel::Tasks;
    let avail = area.width.saturating_sub(2) as usize;

    // O cursor indexa linhas, e uma linha é uma tarefa ou uma subtarefa de uma
    // tarefa expandida — por isso o achatamento vem antes da renderização.
    let rows = app.task_rows();
    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(row, kind)| {
            let line = match kind {
                tasks::TaskRow::Header(group) => {
                    // Mesmo tratamento do cabeçalho de projeto no Jira.
                    return Line::from(Span::styled(
                        group.label(),
                        theme.accent.add_modifier(Modifier::BOLD),
                    ));
                }
                tasks::TaskRow::Task(t) => {
                    let item = &p.items[*t];
                    task_line(item, theme, app.tasks_expanded.contains(&item.id), avail)
                }
                tasks::TaskRow::Sub { task, sub } => {
                    subtask_line(&p.items[*task].subtasks[*sub], theme, avail)
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
fn task_line(t: &TaskItem, theme: &BubbleTheme, expanded: bool, avail: usize) -> Line<'static> {
    let mark = if t.subtasks.is_empty() {
        "  "
    } else if expanded {
        "▾ "
    } else {
        "▸ "
    };
    // "  [x] " ocupa 6 colunas; o prazo, quando existe, mais 7 ("  dd/mm"); a
    // prioridade o tanto de `!` mais o espaço, e a repetição outros 2.
    let priority = t.priority.marker();
    let priority_w = if priority.is_empty() {
        0
    } else {
        priority.chars().count() + 1
    };
    // A hora, quando existe, ocupa " HH:MM".
    let time_w = if t.time.is_empty() { 0 } else { 6 };
    let title_w = room_for(
        avail,
        6 + priority_w
            + if t.due.is_empty() { 0 } else { 7 }
            + time_w
            + t.recur.marker().chars().count(),
    );
    let mut spans = if t.completed {
        vec![
            theme.muted(mark),
            theme.muted("[x] "),
            theme.muted(clip(&t.title, title_w)),
        ]
    } else {
        vec![
            theme.muted(mark),
            theme.span("[ ] "),
            theme.span(clip(&t.title, title_w)),
        ]
    };
    // Prioridade e repetição ficam colados no prazo, à direita: é o bloco que
    // responde "isso é urgente?" de uma olhada só.
    if !priority.is_empty() {
        spans.push(theme.span(" "));
        spans.push(theme.accent(priority));
    }
    if !t.due.is_empty() {
        spans.push(theme.muted("  "));
        spans.push(theme.accent(short_date(&t.due)));
    }
    if !t.time.is_empty() {
        spans.push(theme.accent(format!(" {}", t.time)));
    }
    if !t.recur.marker().is_empty() {
        spans.push(theme.muted(t.recur.marker()));
    }
    Line::from(spans)
}

/// Overlay da central de notificações.
///
/// Sobrepõe a tela como o detalhe de e-mail. Cada linha traz o marcador da fonte
/// (`[JIRA]` hoje), então quando entrarem convites de agenda e menções do GitHub
/// a lista continua legível sem mudar o layout.
fn render_notifications(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let Some(view) = &app.notifications else { return };
    let items = app.notification_items();

    let title = format!(" NOTIFICAÇÕES  {} ", items.len());
    // O overlay ocupa 76% da largura; menos as bordas do bloco.
    let inner_w = (area.width as usize * 76 / 100).saturating_sub(4);
    let lines: Vec<Line> = if items.is_empty() {
        let msg = if app.jira_mentions.loaded {
            "Nada pedindo sua atenção."
        } else {
            "Buscando…"
        };
        vec![Line::from(theme.muted(msg))]
    } else {
        items
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let line = Line::from(vec![
                    theme.muted(n.source.marker()),
                    theme.span(" "),
                    theme.span(clip(&n.title, room_for(inner_w, 8 + 2 + 26))),
                    theme.muted("  "),
                    theme.muted(clip(&n.context, 26)),
                ]);
                highlight(line, theme, i == view.cursor)
            })
            .collect()
    };

    let popup = centered_rect(76, 60, area);
    frame.render_widget(Clear, popup);
    let block = theme.titled_modal_block(title);
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    let height = rows[0].height as usize;
    let off = window(lines.len(), view.cursor, 0, height);
    render_lines(frame, theme, rows[0], lines, off);
    // Sem banco a leitura não sobrevive ao fechar o programa; melhor dizer isso
    // aqui do que deixar as mesmas notificações voltando sem explicação.
    let footer = match &app.store_error {
        Some(e) => Line::from(theme.error(format!("banco indisponível: {e}"))),
        None => Line::from(theme.muted(
            "j/k: navegar · Espaço: marcar lida · Enter: abrir no navegador · Esc/n: fechar",
        )),
    };
    frame.render_widget(Paragraph::new(footer), rows[1]);
}

/// Linha de uma subtarefa: indentada sob a tarefa, com o mesmo checkbox.
fn subtask_line(s: &SubTask, theme: &BubbleTheme, avail: usize) -> Line<'static> {
    if s.completed {
        Line::from(vec![
            theme.muted("      [x] "),
            theme.muted(clip(&s.title, room_for(avail, 10))),
        ])
    } else {
        Line::from(vec![
            theme.span("      [ ] "),
            theme.span(clip(&s.title, room_for(avail, 10))),
        ])
    }
}

/// Overlay do prompt de tarefa (entrada de texto ou confirmação de exclusão).
/// Campos do formulário de tarefa, na ordem em que aparecem.
const TASK_FIELDS: [TaskField; 4] = [
    TaskField::Title,
    TaskField::Due,
    TaskField::Recur,
    TaskField::Priority,
];

/// Uma linha do formulário: rótulo, valor e a marca de qual campo está ativo.
///
/// Campo de texto mostra o cursor (`█`); campo de escolha mostra `‹ valor ›`,
/// que é o que diz "aqui se circula em vez de digitar".
fn form_field_line(form: &TaskForm, field: TaskField, theme: &BubbleTheme) -> Line<'static> {
    let active = form.field == field;
    let mut spans = vec![
        if active {
            theme.accent("▸ ")
        } else {
            theme.span("  ")
        },
        theme.muted(format!("{:<12}", field.label())),
    ];
    match field {
        TaskField::Title | TaskField::Due => {
            let value = if field == TaskField::Title {
                form.title.clone()
            } else {
                form.due.clone()
            };
            spans.push(theme.span(value));
            if active {
                spans.push(theme.accent("█"));
            }
            if field == TaskField::Due && form.due.trim().is_empty() {
                spans.push(theme.muted("  (vazio = sem data; hoje, amanhã, +3d)"));
            }
        }
        TaskField::Recur => spans.push(choice(form.recur.label(), active, theme)),
        TaskField::Priority => spans.push(choice(form.priority.label(), active, theme)),
    }
    Line::from(spans)
}

/// Valor de um campo de escolha, com as setas quando ele está ativo.
fn choice(label: &str, active: bool, theme: &BubbleTheme) -> Span<'static> {
    if active {
        theme.accent(format!("‹ {label} ›"))
    } else {
        theme.span(format!("  {label}  "))
    }
}

fn render_prompt(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let Some(prompt) = &app.prompt else { return };

    let (title, lines): (String, Vec<Line>) = match prompt {
        Prompt::Input { kind, buffer } => {
            let title = match kind {
                InputKind::AddTask => " Nova tarefa ".to_string(),
                InputKind::AddSubtask { .. } => " Nova subtarefa ".to_string(),
                InputKind::EditSubtask { .. } => " Editar subtarefa ".to_string(),
            };
            let input = Line::from(vec![theme.span(buffer.clone()), theme.accent("█")]);
            let help = Line::from(theme.muted("Enter: salvar · Esc: cancelar"));
            (title, vec![input, Line::from(""), help])
        }
        Prompt::EditTask(form) => {
            let mut lines: Vec<Line> = TASK_FIELDS
                .iter()
                .map(|f| form_field_line(form, *f, theme))
                .collect();
            lines.push(Line::from(""));
            lines.push(match &form.error {
                Some(e) => Line::from(theme.error(e.clone())),
                None => Line::from(theme.muted(
                    "Tab: campo · Espaço/←→: escolher · Enter: salvar · Esc: cancelar",
                )),
            });
            (" Editar tarefa ".to_string(), lines)
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
        Prompt::ConfirmSubtaskDelete { title, .. } => (
            " Apagar subtarefa ".to_string(),
            vec![
                Line::from(vec![
                    theme.muted("Apagar a etapa \""),
                    theme.span(clip(title, 40)),
                    theme.muted("\"?"),
                ]),
                Line::from(""),
                Line::from(theme.muted("y: confirmar · n/Esc: cancelar")),
            ],
        ),
        Prompt::PickFolder {
            folders, cursor, ..
        } => {
            // Uma conta do Gmail tem dezenas de etiquetas (40 na do autor), então
            // a lista rola em torno do cursor em vez de tentar caber inteira.
            const VISIBLE: usize = 12;
            let title = if folders.is_empty() {
                " Mover e-mail ".to_string()
            } else {
                format!(" Mover e-mail  {}/{} ", cursor + 1, folders.len())
            };
            let mut lines = vec![Line::from(theme.muted("Mover para:")), Line::from("")];
            if folders.is_empty() {
                lines.push(Line::from(theme.muted("  buscando as pastas da conta…")));
            }
            let off = window(folders.len(), *cursor, cursor.saturating_sub(VISIBLE / 2), VISIBLE);
            lines.extend(
                folders
                    .iter()
                    .enumerate()
                    .skip(off)
                    .take(VISIBLE)
                    .map(|(i, (account, folder))| {
                        highlight(
                            Line::from(vec![
                                theme.muted(format!("  {} ", account.marker())),
                                theme.span(clip(folder, 42)),
                            ]),
                            theme,
                            i == *cursor,
                        )
                    }),
            );
            lines.push(Line::from(""));
            lines.push(Line::from(theme.muted(
                "j/k: escolher · Enter: mover · Esc: cancelar",
            )));
            (title, lines)
        }
        Prompt::ConfirmEmailDelete { what, .. } => (
            " Excluir e-mail ".to_string(),
            vec![
                Line::from(vec![
                    theme.muted("Mover para a Lixeira: \""),
                    theme.span(clip(what, 36)),
                    theme.muted("\"?"),
                ]),
                Line::from(""),
                Line::from(theme.muted("y: confirmar · n/Esc: cancelar")),
            ],
        ),
    };

    // O seletor de pasta tem 10 linhas (título, seis pastas, espaçamento, ajuda);
    // os outros prompts cabem em 24% da tela.
    let height = match prompt {
        Prompt::PickFolder { .. } => 46,
        // Quatro campos, espaçamento e a linha de ajuda/erro.
        Prompt::EditTask(_) => 34,
        _ => 24,
    };
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
fn build_agenda_lines(items: &[AgendaItem], theme: &BubbleTheme, avail: usize) -> Vec<Line<'static>> {
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
            theme.span(clip(&a.title, room_for(avail, 12))),
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
/// Compõe o rodapé cabendo na largura, sem nunca perder `q sair`.
///
/// As dicas do painel vêm primeiro porque são as menos óbvias, mas num terminal
/// estreito elas são cortadas da direita para a esquerda até o global caber — a
/// tecla de sair é a única que precisa estar visível sempre. Contar caracteres à
/// mão já custou dois testes vermelhos; aqui a largura decide.
fn fit_footer(
    theme: &BubbleTheme,
    hints: &str,
    globals: &[(&str, &str)],
    width: usize,
) -> Line<'static> {
    // O `help_line` empresta dos `&str` recebidos; reconstruir com conteúdo
    // próprio deixa a linha independente da vida dos argumentos.
    let global: Line<'static> = Line::from(
        theme
            .help_line(globals.iter().copied())
            .spans
            .into_iter()
            .map(|s| Span::styled(s.content.into_owned(), s.style))
            .collect::<Vec<_>>(),
    );
    let global_len: usize = global.spans.iter().map(|s| s.content.chars().count()).sum();
    let mut segments: Vec<&str> = if hints.is_empty() {
        Vec::new()
    } else {
        hints.split(" · ").collect()
    };

    // Corta dicas do fim até o conjunto caber junto do global.
    while !segments.is_empty() {
        let kept = segments.join(" · ").chars().count();
        if kept + 3 + global_len <= width {
            break;
        }
        segments.pop();
    }

    if segments.is_empty() {
        return global;
    }
    let mut spans = vec![theme.span(segments.join(" · ")), theme.muted(" · ")];
    spans.extend(global.spans);
    Line::from(spans)
}

fn panel_hints(focus: Panel) -> &'static str {
    match focus {
        Panel::Email => "shift+↑↓ marca · x alterna · espaço lido · m move · d exclui · ctrl+enter gmail",
        Panel::Jira => "f filtro · p por-pai · esc volta",
        Panel::Tasks => "enter expande · espaço alterna · a nova · A subtarefa · e edita · d apaga",
        _ => "",
    }
}

/// Largura da coluna de status do footer (o `⟳ HH:MM:SS` à direita).
const FOOTER_STATUS_WIDTH: u16 = 22;

/// Texto da coluna de status do footer.
///
/// Puro para a largura poder ser fixada em teste: texto que passa da coluna é
/// cortado em silêncio pelo widget, e sobraria a palavra truncada na tela.
fn status_text(refreshing: bool, frame: &str, last_refresh: Option<DateTime<Local>>) -> String {
    if refreshing {
        return format!("{frame} recarregando");
    }
    match last_refresh {
        Some(t) => format!("⟳ {}", clock::format_time(&t)),
        None => "⟳ …".to_string(),
    }
}

fn render_footer(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(FOOTER_STATUS_WIDTH)])
        .split(area);

    let help = if app.detail.is_some() {
        theme.help_line([("j/k", "rolar"), ("Esc", "voltar")])
    } else if app.prompt.is_some() {
        theme.help_line([("Enter/y", "confirmar"), ("Esc/n", "cancelar")])
    } else {
        let hints = panel_hints(app.focus);
        let globals: &[(&str, &str)] = if hints.is_empty() {
            &[
                ("Tab", "painel"),
                ("j/k", "rolar"),
                ("Enter", "abrir"),
                ("n", "notificações"),
                ("r", "atualizar"),
                ("q", "sair"),
            ]
        } else {
            // Com dicas do painel na frente, o global encurta: `j/k` e `Enter`
            // são o que qualquer um tenta primeiro.
            &[("Tab", "painel"), ("n", "notificações"), ("q", "sair")]
        };
        fit_footer(theme, hints, globals, cols[0].width as usize)
    };
    frame.render_widget(Paragraph::new(help), cols[0]);

    // O mesmo spinner do painel vazio, para a tela não ter duas animações
    // discordando sobre o que significa "trabalhando".
    let status = status_text(
        app.refreshing,
        app.spinner.current_frame(),
        app.last_refresh,
    );
    frame.render_widget(
        Paragraph::new(Line::from(theme.muted(status))).alignment(Alignment::Right),
        cols[1],
    );
}

fn render_detail(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let Some(d) = &app.detail else { return };

    let popup = centered_rect(80, 80, area);
    frame.render_widget(Clear, popup);

    let block = theme.titled_modal_block(format!(
        " {} ",
        clip(&d.subject, room_for(area.width as usize * 76 / 100, 8))
    ));
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
    use crate::pomodoro::{Phase, Pomodoro};
    use ratatui_bubbletea_components::SpinnerFrames;
    use std::time::{Duration, Instant};

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
        let lines: Vec<String> = build_agenda_lines(&items, &theme, 80)
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
        assert!(out.contains("JIRA · ambas"), "cabeçalho com o filtro ativo");
        assert!(out.contains("ENG"), "cabeçalho de grupo do projeto");
        assert!(out.contains("ENG-101"), "a chave da issue");
    }

    #[test]
    fn each_issue_line_opens_with_the_type_marker() {
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-101","summary":"Melhorias no dashboard","status":"Em andamento",
                 "project":"ENG","url":"u","parent":null,"type":"História"},
                {"key":"ENG-1","summary":"Plataforma","status":"Em andamento",
                 "project":"ENG","url":"u","parent":null,"type":"Iniciativa"}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("[S] ENG-101"), "história marcada com [S]");
        assert!(out.contains("[I] ENG-1"), "iniciativa marcada com [I]");
    }

    #[test]
    fn in_the_both_filter_only_what_is_not_yours_is_marked() {
        // A queixa era "nada me difere o que é meu do que eu só relatei".
        // Marcar os dois lados dava três grupos de colchetes esmaecidos na
        // mesma linha; agora sem marca significa "é sua".
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-1","summary":"minha","status":"Em andamento","project":"ENG",
                 "url":"u","parent":null,"type":"História","role":"assignee"},
                {"key":"ENG-2","summary":"só relatei","status":"Em andamento","project":"ENG",
                 "url":"u","parent":null,"type":"História","role":"reporter"},
                {"key":"ENG-3","summary":"as duas","status":"Em andamento","project":"ENG",
                 "url":"u","parent":null,"type":"História","role":"both"}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        app.jira_filter = crate::data::jira::JiraFilter::Both;

        let out = render_to_string(&app, 120, 30);
        let line = |needle: &str| {
            out.lines()
                .find(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("falta a linha de {needle}"))
                .to_string()
        };
        assert!(line("ENG-2").contains("REL "), "só relator é marcado");
        assert!(!line("ENG-1").contains("REL "), "a sua não leva marca");
        assert!(!line("ENG-3").contains("REL "), "nem a que é sua e você relatou");
    }

    #[test]
    fn the_role_marker_cannot_be_confused_with_the_request_type() {
        // Visto em dado real: uma requisição relatada por mim saía
        // "[R] PDS-1122 [Pendente] [R] …" — a mesma letra para tipo e papel.
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"PDS-1","summary":"pedido","status":"Pendente","project":"PDS",
                 "url":"u","parent":null,"type":"[System] Service request","role":"reporter"}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        app.jira_filter = crate::data::jira::JiraFilter::Both;
        let line = render_to_string(&app, 120, 30)
            .lines()
            .find(|l| l.contains("PDS-1"))
            .expect("linha")
            .to_string();
        assert!(line.contains("[R] "), "o tipo requisição segue sendo [R]");
        assert!(line.contains("REL PDS-1"), "e o papel tem marcador próprio, antes da chave");
    }

    #[test]
    fn the_role_marker_stays_out_of_the_other_filters() {
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-2","summary":"s","status":"x","project":"ENG","url":"u",
                 "parent":null,"type":"História","role":"reporter"}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        app.jira_filter = crate::data::jira::JiraFilter::Assignee;
        // Filtro `minhas`: toda issue tem o mesmo papel, e a marca seria ruído.
        assert!(!render_to_string(&app, 120, 30).contains("REL "));
    }

    #[test]
    fn a_subtask_is_drawn_indented_under_its_parent() {
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-9","summary":"etapa","status":"Em andamento","project":"ENG",
                 "url":"u","type":"Subtarefa","subtask":true,
                 "parent":{"key":"ENG-7","summary":"história"}},
                {"key":"ENG-7","summary":"história","status":"Em andamento","project":"ENG",
                 "url":"u","type":"História","subtask":false,"parent":null}]"#,
        )
        .unwrap();
        app.jira.loaded = true;

        let out = render_to_string(&app, 120, 30);
        let indent_of = |needle: &str| {
            let line = out.lines().find(|l| l.contains(needle)).expect("linha");
            let inner = line.trim_start_matches(['│', ' ']);
            line.find(inner).unwrap_or(0)
        };
        assert!(out.contains("[s] ENG-9"), "subtarefa tem marcador próprio");
        assert!(
            indent_of("ENG-9") > indent_of("ENG-7"),
            "e entra deslocada em relação ao pai"
        );
        let pos = |k: &str| out.find(k).unwrap();
        assert!(pos("ENG-7") < pos("ENG-9"), "logo abaixo dele, não antes");
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
    fn notifications_overlay_lists_each_source_with_its_marker() {
        let mut app = test_app();
        app.jira_mentions.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-101","summary":"Revisar o plano de capacidade","status":"Em andamento",
                 "project":"ENG","url":"https://example.atlassian.net/browse/ENG-101","parent":null}]"#,
        )
        .unwrap();
        app.jira_mentions.loaded = true;
        app.notifications = Some(crate::app::NotificationsView { cursor: 0 });

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("NOTIFICAÇÕES  1"), "título com a contagem");
        assert!(out.contains("[JIRA]"), "marcador da fonte");
        assert!(out.contains("Revisar o plano de capacidade"));
        assert!(out.contains("ENG-101 · Em andamento"), "contexto da linha");
    }

    #[test]
    fn notifications_overlay_says_when_there_is_nothing() {
        let mut app = test_app();
        app.jira_mentions.loaded = true;
        app.notifications = Some(crate::app::NotificationsView { cursor: 0 });
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("Nada pedindo sua atenção"));
    }

    #[test]
    fn folder_picker_lists_the_real_folders_and_says_when_still_loading() {
        let mut app = test_app();
        app.prompt = Some(Prompt::PickFolder {
            items: vec![(crate::data::Account::Personal, "1".into())],
            folders: Vec::new(),
            cursor: 0,
        });
        assert!(render_to_string(&app, 120, 30).contains("buscando as pastas"));

        app.prompt = Some(Prompt::PickFolder {
            items: vec![(crate::data::Account::Personal, "1".into())],
            folders: vec![
                (crate::data::Account::Personal, "inbox".into()),
                (crate::data::Account::Personal, "Clientes".into()),
            ],
            cursor: 1,
        });
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("Mover para:"));
        assert!(out.contains("Clientes"), "etiqueta do usuário, não só os aliases");
    }

    #[test]
    fn folder_picker_scrolls_around_the_cursor_with_many_labels() {
        // A conta real tem 40 etiquetas: o seletor rola em vez de tentar desenhar
        // todas de uma vez.
        let folders: Vec<(crate::data::Account, String)> = (0..40)
            .map(|i| (crate::data::Account::Personal, format!("etiqueta-{i:02}")))
            .collect();
        let mut app = test_app();
        app.prompt = Some(Prompt::PickFolder {
            items: vec![(crate::data::Account::Personal, "1".into())],
            folders,
            cursor: 30,
        });
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("31/40"), "o título situa onde você está");
        assert!(out.contains("etiqueta-30"), "a etiqueta sob o cursor aparece");
        assert!(!out.contains("etiqueta-00"), "as distantes não são desenhadas");
    }

    #[test]
    fn panels_use_the_width_they_have_instead_of_a_fixed_clip() {
        // O bug: larguras fixas (assunto em 58, resumo em 44, título em 38)
        // cortavam texto mesmo com a tela larga. O mesmo conteúdo renderizado em
        // duas larguras tem de mostrar mais na maior.
        let long: String = "Assunto comprido que só cabe inteiro quando a janela é larga de verdade".into();
        let mut app = test_app();
        app.emails.items = vec![crate::data::EmailItem {
            id: "1".into(),
            account: crate::data::Account::Personal,
            from: "Alguem".into(),
            subject: long.clone(),
            unread: true,
            date: "2026-08-04 10:00+00:00".into(),
        }];
        app.emails.loaded = true;

        // O painel ocupa metade da largura da tela, então a conta exata depende
        // do layout: o que importa é que caiba MAIS quando há mais espaço.
        let longest_visible = |w: u16| -> usize {
            let out = render_to_string(&app, w, 30);
            (1..=long.chars().count())
                .rev()
                .find(|n| {
                    let prefix: String = long.chars().take(*n).collect();
                    out.contains(&prefix)
                })
                .unwrap_or(0)
        };
        let narrow = longest_visible(100);
        let wide = longest_visible(200);
        assert!(
            wide > narrow,
            "largura maior tem de mostrar mais do assunto (estreito={narrow}, largo={wide})"
        );
    }

    #[test]
    fn footer_drops_panel_hints_before_losing_the_quit_key() {
        // Em terminal estreito as dicas do painel são cortadas da direita para a
        // esquerda; sair é a única tecla que precisa aparecer sempre.
        let app = test_app(); // foco em E-mail, que tem dicas longas
        for width in [70usize, 100, 160] {
            let out = render_to_string(&app, width as u16, 30);
            assert!(out.contains("sair"), "largura {width} perdeu o `q sair`");
        }
        // Na larga, as dicas do painel cabem junto.
        assert!(render_to_string(&app, 160, 30).contains("marca"));
    }

    #[test]
    fn the_tasks_footer_shows_its_own_keys_including_the_subtask_one() {
        // O painel de Tarefas tinha um rodapé próprio, fora do caminho que
        // encurta por largura — e por isso ignorava as dicas declaradas nele.
        let mut app = test_app();
        app.focus = Panel::Tasks;
        let out = render_to_string(&app, 160, 30);
        assert!(out.contains("A subtarefa"), "a tecla nova aparece");
        assert!(out.contains("expande"), "e as que já existiam também");
        assert!(out.contains("sair"));
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
        assert!(out.contains("notificações"), "e o `n` global");
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

    /// Tarefa mínima para os testes de render.
    fn task_fixture(id: &str, title: &str) -> TaskItem {
        TaskItem {
            id: id.into(),
            title: title.into(),
            completed: false,
            due: String::new(),
            notes: String::new(),
            subtasks: Vec::new(),
            time: String::new(),
            priority: Default::default(),
            recur: Default::default(),
        }
    }

    /// App com um conjunto de painéis ligados, sem depender do config global.
    fn app_with_panels(on: &[Panel]) -> App {
        let mut app = test_app();
        app.panels = on.to_vec();
        app.focus = on[0];
        app
    }

    #[test]
    fn the_columns_keep_todays_proportions_when_everything_is_on() {
        let (left, right) = columns(&Panel::ORDER);
        assert_eq!(left, vec![(Panel::Email, 60), (Panel::Jira, 40)]);
        assert_eq!(
            right,
            vec![(Panel::Agenda, 40), (Panel::Pulls, 30), (Panel::Tasks, 30)]
        );
    }

    #[test]
    fn a_column_left_empty_is_not_reserved() {
        let (left, right) = columns(&[Panel::Agenda, Panel::Tasks]);
        assert!(left.is_empty(), "nada na esquerda");
        assert_eq!(right, vec![(Panel::Agenda, 40), (Panel::Tasks, 30)]);
    }

    #[test]
    fn a_panel_that_is_off_is_not_drawn() {
        let app = app_with_panels(&[Panel::Email, Panel::Agenda]);
        let out = render_to_string(&app, 120, 40);
        assert!(out.contains("E-MAILS"));
        assert!(out.contains("AGENDA"));
        assert!(!out.contains("JIRA"), "painel desligado não aparece");
        assert!(!out.contains("TAREFAS"));
        assert!(!out.contains("ghpending"));
    }

    #[test]
    fn the_only_panel_left_takes_the_whole_screen() {
        // Com uma coluna vazia, a outra não fica com metade da tela sobrando.
        let mut app = app_with_panels(&[Panel::Email]);
        app.emails.items = vec![crate::data::EmailItem {
            id: "1".into(),
            account: crate::data::Account::Personal,
            from: "Alguem".into(),
            subject: "Assunto comprido que só cabe inteiro numa coluna larga".into(),
            unread: true,
            date: "2026-08-05 10:00+00:00".into(),
        }];
        app.emails.loaded = true;
        let out = render_to_string(&app, 120, 40);
        assert!(
            out.contains("numa coluna larga"),
            "o assunto inteiro cabe: a largura toda é do e-mail"
        );
    }

    #[test]
    fn tasks_panel_renders_checkbox_and_titles() {
        let mut app = test_app();
        app.tasks.items = vec![
            TaskItem { id: "1".into(), title: "Comprar café".into(), completed: false, due: "2026-06-10".into(), notes: String::new(), subtasks: Vec::new(), time: String::new(), priority: Default::default(), recur: Default::default() },
            TaskItem { id: "2".into(), title: "Já feito".into(), completed: true, due: String::new(), notes: String::new(), subtasks: Vec::new(), time: String::new(), priority: Default::default(), recur: Default::default() },
        ];
        app.tasks.loaded = true;
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("TAREFAS"));
        assert!(out.contains("[ ] Comprar café"));
        assert!(out.contains("[x] Já feito"));
        assert!(out.contains("10/06")); // prazo formatado
    }

    #[test]
    fn the_tasks_panel_groups_by_deadline_and_marks_priority() {
        let today = chrono::Local::now().date_naive();
        let mut app = test_app();
        let mut atrasada = task_fixture("1", "Orçar o serviço");
        atrasada.due = (today - chrono::Duration::days(3))
            .format("%Y-%m-%d")
            .to_string();
        let mut hoje = task_fixture("2", "Revisar o plano");
        hoje.due = today.format("%Y-%m-%d").to_string();
        hoje.priority = tasks::Priority::High;
        hoje.recur = tasks::Recur::Weekly;
        let sem_data = task_fixture("3", "Ler o artigo salvo");
        app.tasks.items = vec![atrasada, hoje, sem_data];
        app.tasks.loaded = true;

        let out = render_to_string(&app, 120, 40);
        let at = |needle: &str| out.find(needle).unwrap_or_else(|| panic!("falta {needle}"));
        assert!(at("ATRASADAS") < at("HOJE"), "atrasadas vêm primeiro");
        assert!(at("HOJE") < at("SEM DATA"), "sem data vai para o fim");
        assert!(!out.contains("ESTA SEMANA"), "faixa vazia não vira cabeçalho");
        assert!(out.contains("!!!"), "prioridade alta marca a linha com três");
        assert!(out.contains("↻"), "e a repetição também");
    }

    #[test]
    fn the_edit_form_shows_every_field_with_the_active_one_marked() {
        let mut app = test_app();
        let mut t = task_fixture("1", "Revisar o plano");
        t.due = "2026-08-07".into();
        t.recur = tasks::Recur::Weekly;
        t.priority = tasks::Priority::High;
        // Pelo caminho real: é ele que deixa o cursor numa linha de tarefa.
        app.update(crate::msg::Msg::TasksLoaded(Ok(vec![t])));
        app.focus = Panel::Tasks;
        app.update(crate::msg::Msg::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char('e'),
            ratatui::crossterm::event::KeyModifiers::empty(),
        )));

        let out = render_to_string(&app, 120, 40);
        assert!(out.contains("Editar tarefa"));
        assert!(out.contains("Revisar o plano"));
        assert!(out.contains("2026-08-07"));
        assert!(out.contains("semanal"), "repetição pelo nome");
        assert!(out.contains("alta"), "prioridade pelo nome");
        assert!(out.contains("Tab"), "e como andar entre os campos");
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

    #[test]
    fn the_remaining_time_is_zero_padded_minutes_and_seconds() {
        assert_eq!(format_left(Duration::from_secs(90)), "01:30");
        assert_eq!(format_left(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format_left(Duration::ZERO), "00:00");
        // Acima de uma hora segue em minutos: a caixa não tem espaço para
        // um campo de hora que ninguém usa num pomodoro.
        assert_eq!(format_left(Duration::from_secs(75 * 60)), "75:00");
    }

    #[test]
    fn the_bar_fills_in_proportion_and_always_has_the_asked_width() {
        let total = Duration::from_secs(100);
        assert_eq!(progress_bar(Duration::ZERO, total, 10), "░░░░░░░░░░");
        assert_eq!(progress_bar(Duration::from_secs(50), total, 10), "█████░░░░░");
        assert_eq!(progress_bar(total, total, 10), "██████████");
        for elapsed in [0, 33, 67, 100, 250] {
            let bar = progress_bar(Duration::from_secs(elapsed), total, 10);
            assert_eq!(bar.chars().count(), 10, "decorrido {elapsed}");
        }
    }

    #[test]
    fn a_zero_length_total_gives_an_empty_bar_instead_of_dividing_by_zero() {
        assert_eq!(progress_bar(Duration::ZERO, Duration::ZERO, 4), "░░░░");
        assert_eq!(progress_bar(Duration::ZERO, Duration::from_secs(60), 0), "");
    }

    #[test]
    fn the_header_shows_the_pomodoro_next_to_the_clock() {
        let app = test_app();
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("POMODORO"), "{out}");
        // Parado no arranque, com o foco cheio e a dica de iniciar.
        assert!(out.contains("25:00"), "{out}");
        assert!(out.contains("iniciar"), "{out}");
        // O relógio continua lá: a caixa foi somada ao header, não trocou nada.
        // Parado e cheio, a barra está toda vazia (`░`), então os glifos
        // grandes do relógio são a única fonte de `█` no header.
        assert!(out.contains('█'), "{out}");
    }

    #[test]
    fn a_narrow_terminal_keeps_the_clock_and_drops_the_pomodoro_box() {
        // Abaixo de MIN_WIDTH_FOR_POMODORO, `Constraint::Min(0)` para o relógio
        // e `Constraint::Length(22)` para a caixa colapsariam o relógio a zero
        // colunas — o acessório expulsando o widget que o header existe para
        // mostrar.
        let app = test_app();
        let out = render_to_string(&app, 40, 30);
        assert!(!out.contains("POMODORO"), "{out}");
        // O relógio grande usa `█`/`░`; sem a caixa disputando espaço, ele
        // ainda desenha os glifos por extenso.
        assert!(out.contains('█'), "{out}");
    }

    #[test]
    fn a_running_pomodoro_offers_to_pause_instead_of_to_start() {
        let mut app = test_app();
        app.update(key(KeyCode::Char('P')));
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("pausar"), "{out}");
    }

    #[test]
    fn the_finished_focus_count_only_shows_up_after_the_first_one() {
        let mut app = test_app();
        let out = render_to_string(&app, 100, 30);
        assert!(!out.contains('✓'), "zero focos não mostra contador: {out}");

        app.pomodoro = Pomodoro::new(Duration::ZERO, Duration::from_secs(300));
        app.pomodoro.toggle(Instant::now());
        app.update(Msg::ClockTick);
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("1 ✓"), "{out}");
        assert!(out.contains("Descanso"), "{out}");
    }

    #[test]
    fn a_notification_that_did_not_leave_shows_up_without_hiding_the_hint() {
        // Engolir isso faria você confiar num aviso que não vem. E a dica é o
        // único sinal de rodando/parado desde que o sufixo `(parado)` saiu da
        // fase — perder as duas ao mesmo tempo era o achado da revisão final.
        let mut app = test_app();
        app.update(Msg::Notified(Err("sistema: sem servidor".into())));
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("aviso não saiu"), "{out}");
        assert!(out.contains("R zerar"), "a dica continua visível: {out}");
    }

    #[test]
    fn a_stopped_break_with_a_double_digit_count_still_shows_the_full_counter() {
        // Achado da revisão: com o sufixo `(parado)`, `"Descanso (parado)"`
        // (17) + `"10 ✓"` (4) passava dos 20 de largura interna e o contador
        // saía cortado, sem que nenhum teste percebesse — um dia comum com 10
        // focos fechados, não uma borda rara.
        let now = Instant::now();
        let mut p = Pomodoro::new(Duration::ZERO, Duration::ZERO);
        for _ in 0..9 {
            p.toggle(now); // arma o foco
            p.tick(now); // fecha o foco, entra no descanso (rodando)
            p.tick(now); // fecha o descanso, volta ao foco (parado)
        }
        p.toggle(now); // arma o décimo foco
        p.tick(now); // fecha o décimo foco: done vira 10, entra no descanso
        p.toggle(now); // pausa o descanso
        assert_eq!(p.done(), 10);
        assert_eq!(p.phase(), Phase::Break);
        assert!(!p.running());

        let mut app = test_app();
        app.pomodoro = p;
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("10 ✓"), "{out}");
        assert!(out.contains("Descanso"), "{out}");
    }

    #[test]
    fn the_head_line_never_outgrows_the_boxs_inner_width_even_with_many_focuses() {
        // Propriedade direta: rótulo de fase + contador não podem passar da
        // largura interna da caixa (POMODORO_WIDTH menos as duas bordas), ou o
        // widget corta em silêncio — o mesmo defeito do achado acima, mas
        // verificado como invariante em vez de um único valor fixo.
        //
        // A invariante é exata (soma == largura, sem sobra) em `u32::MAX`: com
        // `"Descanso"` (8) e um contador de 10 dígitos (12), 8 + 12 = 20. Sem
        // um valor colado nessa borda, uma regressão que acrescentasse um único
        // caractere fixo ao formato de `done_counter` (um espaço de mais, um
        // separador mais largo) passaria despercebida em `done = 999` — a soma
        // ficaria 13/20, longe do limite — e só estouraria a largura real da
        // caixa nos valores grandes. `999_999_999` (9 dígitos) fica a 1 caractere
        // da borda, e `u32::MAX` fica exatamente nela.
        let inner_width = POMODORO_WIDTH as usize - 2;
        for label in [Phase::Focus.label(), Phase::Break.label()] {
            for done in [0, 1, 9, 10, 99, 999, 999_999_999, u32::MAX] {
                let count = done_counter(done);
                assert!(
                    label.chars().count() + count.chars().count() <= inner_width,
                    "fase {label:?} com {done} focos: {count:?} não cabe em {inner_width}"
                );
            }
        }
    }

    #[test]
    fn the_footer_says_it_is_reloading_while_the_refresh_runs() {
        let mut app = test_app();
        let out = render_to_string(&app, 100, 30);
        assert!(!out.contains("recarregando"), "parado, não anuncia nada: {out}");

        app.update(Msg::RefreshStarted);
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("recarregando"), "{out}");
    }

    #[test]
    fn the_footer_goes_back_to_the_last_refresh_time_when_it_finishes() {
        let mut app = test_app();
        app.update(Msg::RefreshStarted);
        // `EmailsLoaded` é o que preenche `last_refresh`.
        app.update(Msg::EmailsLoaded(Ok(vec![])));
        app.update(Msg::RefreshDone);

        let out = render_to_string(&app, 100, 30);
        assert!(!out.contains("recarregando"), "{out}");
        assert!(out.contains('⟳'), "a hora volta: {out}");
    }

    #[test]
    fn every_spinner_frame_keeps_the_reloading_status_inside_its_column() {
        // Texto que passa da coluna é cortado em silêncio pelo widget, e o que
        // sobraria na tela seria a palavra truncada.
        for frame in SpinnerFrames::DOTS.frames() {
            let text = status_text(true, frame, None);
            // Sem esta asserção o teste mediria a largura do texto errado e
            // continuaria verde se o ramo de "recarregando" desaparecesse.
            assert!(text.contains("recarregando"), "quadro {frame:?}: {text:?}");
            assert!(
                text.chars().count() <= FOOTER_STATUS_WIDTH as usize,
                "quadro {frame:?}: {text:?} passa de {FOOTER_STATUS_WIDTH}"
            );
        }
    }
}
