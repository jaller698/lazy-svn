mod app;
mod types;
mod ui;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::LevelFilter;
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
use simplelog::{CombinedLogger, Config, WriteLogger};
use std::{error::Error, fs, io, path::PathBuf};

use app::App;
use types::ActiveWindow;
use ui::ui;

fn init_logger() -> Result<(), Box<dyn Error>> {
    let log_dir = home_dir().join(".local").join("share").join("lazy-svn");
    fs::create_dir_all(&log_dir)?;
    let log_path = log_dir.join("lazy-svn.log");
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    CombinedLogger::init(vec![WriteLogger::new(level, Config::default(), log_file)])?;
    log::info!("lazy-svn started, log file: {}", log_path.display());
    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() -> Result<(), Box<dyn Error>> {
    if let Err(e) = init_logger() {
        eprintln!("Warning: could not initialise logger: {e}");
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut app = App::new();
    let res = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        log::error!("Fatal error: {e}");
        println!("Error: {}", e);
    }
    log::info!("lazy-svn exiting");
    Ok(())
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app));
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('?') => {
                        app.show_help = !app.show_help;
                        log::debug!("Toggled help window: {}", app.show_help);
                    }
                    KeyCode::Char('q') => {
                        log::info!("User quit");
                        return Ok(());
                    }
                    KeyCode::Char('1') => {
                        app.active_window = ActiveWindow::ChangedFiles;
                        log::debug!("Switched active window to ChangedFiles");
                    }
                    KeyCode::Char('2') => {
                        app.active_window = ActiveWindow::Branches;
                        log::debug!("Switched active window to Branches");
                    }
                    KeyCode::Char('3') => {
                        app.active_window = ActiveWindow::Revisions;
                        log::debug!("Switched active window to Revisions");
                    }
                    KeyCode::Char('4') => {
                        app.active_window = ActiveWindow::Diff;
                        log::debug!("Switched active window to Diff");
                    }
                    KeyCode::Tab => {
                        app.active_window = match app.active_window {
                            ActiveWindow::ChangedFiles => ActiveWindow::Branches,
                            ActiveWindow::Branches => ActiveWindow::Revisions,
                            ActiveWindow::Revisions => ActiveWindow::Diff,
                            ActiveWindow::Diff => ActiveWindow::ChangedFiles,
                        };
                        log::debug!("Switched active window to {:?}", app.active_window);
                    }
                    KeyCode::Char('j') => match app.active_window {
                        ActiveWindow::ChangedFiles => app.next_file(),
                        ActiveWindow::Branches => app.next_branch(),
                        ActiveWindow::Revisions => app.next_revision(),
                        ActiveWindow::Diff => app.scroll_diff_down(),
                    },
                    KeyCode::Char('k') => match app.active_window {
                        ActiveWindow::ChangedFiles => app.previous_file(),
                        ActiveWindow::Branches => app.previous_branch(),
                        ActiveWindow::Revisions => app.previous_revision(),
                        ActiveWindow::Diff => app.scroll_diff_up(),
                    },
                    KeyCode::Char('}') => {
                        if app.active_window == ActiveWindow::Diff {
                            app.scroll_diff_next_hunk();
                        }
                    }
                    KeyCode::Char('{') => {
                        if app.active_window == ActiveWindow::Diff {
                            app.scroll_diff_prev_hunk();
                        }
                    }
                    KeyCode::Char('r') => {
                        log::info!("Refreshing all data");
                        app.refresh_status();
                        app.refresh_branches();
                        app.refresh_log();
                    }
                    KeyCode::Enter => {
                        if app.active_window == ActiveWindow::Revisions {
                            log::info!("Updating working copy to selected revision");
                            app.update_to_revision();
                        }
                    }
                    KeyCode::Char(' ') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.toggle_folder();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

