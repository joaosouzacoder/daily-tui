//! daily-tui — painel de informações do dia (e-mails, agenda, PRs, relógio).

mod ansi;
mod app;
mod clock;
mod data;
mod msg;
mod ui;
mod worker;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui_bubbletea_theme::BubbleTheme;
use ratatui_tea::Program;

use app::App;
use msg::Msg;
use worker::WorkerCmd;

/// Intervalo de atualização dos dados externos.
const REFRESH: Duration = Duration::from_secs(300);
/// Timeout do poll de teclado (limita a latência do relógio e dos dados).
const POLL: Duration = Duration::from_millis(200);

fn main() -> std::io::Result<()> {
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    let (ui_handle, ui_rx) = ratatui_tea::channel::<Msg>();
    let (cmd_tx, worker_handle) = worker::spawn(ui_handle, REFRESH);

    let mut program = Program::new(App::new(BubbleTheme::default(), cmd_tx.clone()));
    program.init();
    program.draw(terminal)?;

    let mut last_tick = Instant::now();

    loop {
        let mut dirty = false;

        // Resultados que o worker mandou (e-mails, agenda, PRs, corpo de e-mail).
        while let Ok(m) = ui_rx.try_recv() {
            program.send(m);
            dirty = true;
        }

        // Teclado / redimensionamento.
        if event::poll(POLL)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    program.send(Msg::Key(k));
                    dirty = true;
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }

        // Pulso de relógio a cada segundo.
        if last_tick.elapsed() >= Duration::from_secs(1) {
            program.send(Msg::ClockTick);
            last_tick = Instant::now();
            dirty = true;
        }

        if program.model().should_quit {
            break;
        }
        if dirty {
            program.draw(terminal)?;
        }
    }

    let _ = cmd_tx.send(WorkerCmd::Quit);
    let _ = worker_handle.join();
    Ok(())
}
