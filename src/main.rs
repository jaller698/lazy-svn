mod app;
mod types;
mod ui;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
use std::{error::Error, io};

use app::App;
use types::ActiveWindow;
use ui::ui;

fn main() -> Result<(), Box<dyn Error>> {
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
        println!("Error: {}", e);
    }
    Ok(())
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app));
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Tab => {
                        app.active_window = match app.active_window {
                            ActiveWindow::ChangedFiles => ActiveWindow::Branches,
                            ActiveWindow::Branches => ActiveWindow::Revisions,
                            ActiveWindow::Revisions => ActiveWindow::Diff,
                            ActiveWindow::Diff => ActiveWindow::ChangedFiles,
                        };
                    }
                    KeyCode::Char('j') => match app.active_window {
                        ActiveWindow::ChangedFiles => app.next_file(),
                        ActiveWindow::Diff => app.scroll_diff_down(),
                        _ => {}
                    },
                    KeyCode::Char('k') => match app.active_window {
                        ActiveWindow::ChangedFiles => app.previous_file(),
                        ActiveWindow::Diff => app.scroll_diff_up(),
                        _ => {}
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
                    KeyCode::Char('r') => app.refresh_status(), // Manual refresh
                    _ => {}
                }
            }
        }
    }
}

