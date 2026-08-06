//! daily-tui — painel de informações do dia (e-mails, agenda, PRs, relógio).

mod ansi;
mod app;
mod clock;
mod config;
mod data;
mod msg;
mod notify;
mod pomodoro;
mod store;
mod ui;
mod worker;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyEventKind};
use ratatui_bubbletea_theme::BubbleTheme;
use ratatui_tea::Program;

use app::App;
use msg::Msg;
use worker::WorkerCmd;

/// Timeout do poll de teclado (limita a latência do relógio e dos dados).
const POLL: Duration = Duration::from_millis(200);

/// O que a linha de comando pediu.
enum Cmd {
    /// Abrir o painel.
    Run(Option<std::path::PathBuf>),
    /// Escrever o config de exemplo no lugar certo do SO.
    Init,
    /// Despejar o config resolvido para o `setup-auth.sh`.
    PrintConfig(Option<std::path::PathBuf>),
    Help,
    /// Argumento que não existe: a mensagem já vem pronta.
    Unknown(String),
}

const USAGE: &str = "\
daily-tui — painel do dia (e-mails, Jira, agenda, PRs, tarefas).

Uso:
  daily-tui                    abre o painel
  daily-tui --config ARQUIVO   usa outro config
  daily-tui --init             escreve o config de exemplo no lugar do seu SO
  daily-tui --print-config     mostra o config resolvido (usado pelo setup-auth)
  daily-tui --help
";

/// Lê os argumentos. São quatro flags: `clap` seria mais peso do que ajuda.
fn parse_args(args: impl Iterator<Item = String>) -> Cmd {
    let mut config = None;
    let mut print = false;
    let mut init = false;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Cmd::Help,
            "--init" => init = true,
            "--print-config" => print = true,
            "--config" => match args.next() {
                Some(path) => config = Some(std::path::PathBuf::from(path)),
                None => return Cmd::Unknown("--config sem caminho".into()),
            },
            other => return Cmd::Unknown(format!("argumento desconhecido: {other}")),
        }
    }
    match (init, print) {
        (true, _) => Cmd::Init,
        (_, true) => Cmd::PrintConfig(config),
        _ => Cmd::Run(config),
    }
}

fn main() -> std::io::Result<()> {
    // Config antes da tela: o erro tem de sair no terminal de verdade, não na
    // tela alternativa que morre junto com o processo.
    let path = match parse_args(std::env::args().skip(1)) {
        Cmd::Help => {
            print!("{USAGE}");
            return Ok(());
        }
        Cmd::Unknown(msg) => {
            eprintln!("{msg}\n\n{USAGE}");
            std::process::exit(2);
        }
        Cmd::Init => match config::write_example() {
            Ok(path) => {
                println!("config escrito em {}", path.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        Cmd::PrintConfig(path) => {
            match config::load(path.as_deref()) {
                Ok(cfg) => print!("{}", cfg.print_shell()),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        Cmd::Run(path) => path,
    };

    match config::load(path.as_deref()) {
        Ok(cfg) => config::init(cfg),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    let (ui_handle, ui_rx) = ratatui_tea::channel::<Msg>();
    let refresh = Duration::from_secs(config::get().refresh.seconds);
    let (cmd_tx, worker_handle) = worker::spawn(ui_handle, refresh);

    let mut app = App::new(BubbleTheme::default(), cmd_tx.clone());
    // Banco só depois do app pronto: se ele não abrir, o painel segue vivo (sem
    // memória de notificação lida nem cache de pastas) e diz o motivo.
    app.attach_store(store::Store::open());
    let mut program = Program::new(app);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Cmd {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_opens_the_panel_with_the_default_config() {
        assert!(matches!(args(&[]), Cmd::Run(None)));
    }

    #[test]
    fn config_takes_the_next_argument_as_its_path() {
        match args(&["--config", "/tmp/x.toml"]) {
            Cmd::Run(Some(path)) => assert_eq!(path, std::path::PathBuf::from("/tmp/x.toml")),
            _ => panic!("esperava Run com caminho"),
        }
    }

    #[test]
    fn config_without_a_path_is_refused_instead_of_ignored() {
        assert!(matches!(args(&["--config"]), Cmd::Unknown(_)));
    }

    #[test]
    fn print_config_keeps_the_chosen_file() {
        assert!(matches!(
            args(&["--config", "/tmp/x.toml", "--print-config"]),
            Cmd::PrintConfig(Some(_))
        ));
    }

    #[test]
    fn an_unknown_flag_does_not_silently_open_the_panel() {
        // Abrir a TUI engolindo um `--panels` inventado esconderia o erro de
        // digitação atrás de uma tela cheia.
        assert!(matches!(args(&["--panels"]), Cmd::Unknown(_)));
    }
}
