mod app;
mod types;
mod ui;

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::LevelFilter;
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
use simplelog::{CombinedLogger, Config, WriteLogger};
use std::{error::Error, fs, io, path::PathBuf, process::ExitCode};

use app::App;
use types::{ActiveWindow, CommitField};
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
        if terminal.draw(|f| ui(f, app)).is_err() {
            log::error!("Error drawing UI");
            return io::Result::Err(io::Error::new(io::ErrorKind::Other, "Failed to draw UI"));
        }
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                // When the commit popup is open, all keystrokes go to the active field.
                if app.active_window == ActiveWindow::Commit {
                    match key.code {
                        KeyCode::Esc => {
                            app.active_window = ActiveWindow::ChangedFiles;
                            app.commit_message.clear();
                            app.commit_username.clear();
                            app.commit_password.clear();
                            app.commit_active_field = CommitField::Message;
                            log::debug!("Commit cancelled");
                        }
                        // Ctrl+Enter submits from any field.
                        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            log::info!("User confirmed commit (Ctrl+Enter)");
                            app.do_commit();
                        }
                        // Plain Enter: newline in the message field, advance focus in other fields.
                        KeyCode::Enter => match app.commit_active_field {
                            CommitField::Message => app.commit_message.push('\n'),
                            CommitField::Username => {
                                app.commit_active_field = CommitField::Password;
                            }
                            CommitField::Password => {
                                log::info!("User confirmed commit (Enter on password)");
                                app.do_commit();
                            }
                        },
                        // Tab / BackTab cycle through the three fields.
                        KeyCode::Tab => {
                            app.commit_active_field = match app.commit_active_field {
                                CommitField::Message => CommitField::Username,
                                CommitField::Username => CommitField::Password,
                                CommitField::Password => CommitField::Message,
                            };
                        }
                        KeyCode::BackTab => {
                            app.commit_active_field = match app.commit_active_field {
                                CommitField::Message => CommitField::Password,
                                CommitField::Username => CommitField::Message,
                                CommitField::Password => CommitField::Username,
                            };
                        }
                        KeyCode::Backspace => match app.commit_active_field {
                            CommitField::Message => {
                                app.commit_message.pop();
                            }
                            CommitField::Username => {
                                app.commit_username.pop();
                            }
                            CommitField::Password => {
                                app.commit_password.pop();
                            }
                        },
                        KeyCode::Char(c) => match app.commit_active_field {
                            CommitField::Message => app.commit_message.push(c),
                            CommitField::Username => app.commit_username.push(c),
                            CommitField::Password => app.commit_password.push(c),
                        },
                        _ => {}
                    }
                    continue;
                }

                // When the confirm-delete popup is open, only y/n/Esc are active.
                if app.active_window == ActiveWindow::ConfirmDelete {
                    match key.code {
                        KeyCode::Char('y') => {
                            log::info!("User confirmed delete");
                            app.confirm_delete();
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            log::debug!("Delete cancelled");
                            app.delete_targets.clear();
                            app.active_window = ActiveWindow::ChangedFiles;
                        }
                        _ => {}
                    }
                    continue;
                }

                // When the help window is focused only a few keys are active.
                if app.active_window == ActiveWindow::Help {
                    match key.code {
                        KeyCode::Char('?') => app.close_help(),
                        KeyCode::Char('q') => {
                            log::info!("User quit");
                            return Ok(());
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('?') => {
                        app.open_help();
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
                            ActiveWindow::Commit => ActiveWindow::ChangedFiles,
                            ActiveWindow::Help => ActiveWindow::ChangedFiles,
                            ActiveWindow::ConfirmDelete => ActiveWindow::ChangedFiles,
                        };
                        log::debug!("Switched active window to {:?}", app.active_window);
                    }
                    KeyCode::Char('j') => match app.active_window {
                        ActiveWindow::ChangedFiles => app.next_file(),
                        ActiveWindow::Branches => app.next_branch(),
                        ActiveWindow::Revisions => app.next_revision(),
                        ActiveWindow::Diff => app.scroll_diff_down(),
                        ActiveWindow::Commit => {}
                        ActiveWindow::Help => {}
                        ActiveWindow::ConfirmDelete => {}
                    },
                    KeyCode::Char('k') => match app.active_window {
                        ActiveWindow::ChangedFiles => app.previous_file(),
                        ActiveWindow::Branches => app.previous_branch(),
                        ActiveWindow::Revisions => app.previous_revision(),
                        ActiveWindow::Diff => app.scroll_diff_up(),
                        ActiveWindow::Commit => {}
                        ActiveWindow::Help => {}
                        ActiveWindow::ConfirmDelete => {}
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
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.svn_revert_marked();
                            log::debug!("Ran svn revert on marked files");
                        } else {
                            log::info!("Refreshing all data");
                            app.refresh_status();
                            app.refresh_branches();
                            app.refresh_log();
                        }
                    }
                    // Enter: fold/unfold directory in ChangedFiles; update revision in Revisions.
                    KeyCode::Enter => match app.active_window {
                        ActiveWindow::ChangedFiles => app.toggle_folder(),
                        ActiveWindow::Revisions => {
                            log::info!("Updating working copy to selected revision");
                            app.update_to_revision();
                        }
                        _ => {}
                    },
                    // Space: toggle file selection in ChangedFiles (files only).
                    KeyCode::Char(' ') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.toggle_file_selection();
                        }
                    }
                    // 'a': run `svn add` on marked unversioned files.
                    KeyCode::Char('a') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.svn_add_marked();
                            log::debug!("Ran svn add on marked files");
                        }
                    }
                    // 'd': prompt to confirm then run `svn delete` on marked files/folders.
                    KeyCode::Char('d') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.svn_delete_marked();
                            log::debug!("Opened delete confirmation for marked files");
                        }
                    }
                    // 'c': open commit popup from the ChangedFiles panel.
                    KeyCode::Char('c') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.commit_message.clear();
                            app.active_window = ActiveWindow::Commit;
                            log::debug!("Opened commit window");
                        }
                    }
                    // 'u': undo the last delete (restore from backup).
                    KeyCode::Char('u') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.undo_last_delete();
                            log::debug!("Undid last delete");
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
