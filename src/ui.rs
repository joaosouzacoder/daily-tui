//! Renderização: layout, painéis com rolagem, header do relógio e overlay.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use ratatui_bubbletea_theme::BubbleTheme;

use crate::app::{App, Panel};
use crate::clock;

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
            Constraint::Length(3), // header (relógio)
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
}

fn render_header(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let time = clock::format_time(&app.now);
    let date = clock::format_date(&app.now);

    let line = Line::from(vec![
        Span::styled(time, theme.accent.add_modifier(Modifier::BOLD)),
        theme.muted("   "),
        theme.muted(date),
    ]);

    let block = theme.block();
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(line).alignment(Alignment::Center),
        inner,
    );
}

fn render_body(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(cols[1]);

    render_emails(app, frame, cols[0]);
    render_agenda(app, frame, right[0]);
    render_pulls(app, frame, right[1]);
}

fn render_emails(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.emails;
    let unread = p.items.iter().filter(|e| e.unread).count();
    let title = format!(" E-MAILS  {}/{} ", unread, p.items.len());

    let selected = (app.focus == Panel::Email).then_some(p.cursor);
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

    render_scroll_panel(frame, app, area, title, app.focus == Panel::Email, lines, p);
}

fn render_agenda(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.agenda;
    let title = format!(" AGENDA  {} ", p.items.len());

    let selected = (app.focus == Panel::Agenda).then_some(p.cursor);
    let lines: Vec<Line> = p
        .items
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let when = if a.all_day() {
                format!("{} dia", short_date(&a.date))
            } else {
                format!("{} {}", short_date(&a.date), a.time)
            };
            let line = Line::from(vec![
                theme.accent(when),
                theme.span(" "),
                theme.span(clip(&a.title, 40)),
                theme.muted(" "),
                theme.muted(a.account.marker()),
            ]);
            highlight(line, theme, selected == Some(i))
        })
        .collect();

    render_scroll_panel(frame, app, area, title, app.focus == Panel::Agenda, lines, p);
}

fn render_pulls(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.pulls;
    let title = " PRs (ghpending) ".to_string();

    let selected = (app.focus == Panel::Pulls).then_some(p.cursor);
    let lines: Vec<Line> = p
        .items
        .iter()
        .enumerate()
        .map(|(i, l)| highlight(Line::from(theme.span(l.as_str())), theme, selected == Some(i)))
        .collect();

    render_scroll_panel(frame, app, area, title, app.focus == Panel::Pulls, lines, p);
}

/// Renderiza um painel com borda, título e rolagem; ou um estado vazio
/// (spinner enquanto carrega, erro, ou "vazio").
fn render_scroll_panel<T>(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: String,
    focused: bool,
    lines: Vec<Line>,
    p: &crate::app::PanelData<T>,
) {
    let theme = &app.theme;
    let block = theme.block_with_focus(focused).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if lines.is_empty() {
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
        return;
    }

    let height = inner.height as usize;
    let off = window(lines.len(), p.cursor, p.offset.get(), height);
    p.offset.set(off);
    let visible: Vec<Line> = lines.into_iter().skip(off).take(height).collect();
    frame.render_widget(theme.paragraph(Text::from(visible)), inner);
}

fn render_footer(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(22)])
        .split(area);

    let help = if app.detail.is_some() {
        theme.help_line([("j/k", "rolar"), ("Esc", "voltar")])
    } else {
        theme.help_line([
            ("Tab", "painel"),
            ("j/k", "rolar"),
            ("Enter", "abrir"),
            ("r", "atualizar"),
            ("q", "sair"),
        ])
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
        format!("{}/{}", parts[2], parts[1])
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
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::sync::mpsc;

    fn test_app() -> App {
        let (tx, _rx) = mpsc::channel();
        App::new(BubbleTheme::default(), tx)
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
        assert_eq!(short_date("2026-06-12"), "12/06");
        assert_eq!(short_date("invalid"), "invalid");
    }

    #[test]
    fn clip_truncates_with_ellipsis() {
        assert_eq!(clip("hello", 10), "hello");
        assert_eq!(clip("hello world", 5), "hell…");
    }
}
