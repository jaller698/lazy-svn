use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::{error::Error, io, process::Command};

#[derive(PartialEq, Clone)]
enum ActiveWindow {
    ChangedFiles,
    Branches,
    Revisions,
    Diff,
}

struct SvnFile {
    status: String,
    path: String,
}

struct App {
    active_window: ActiveWindow,
    file_list: Vec<SvnFile>,
    file_list_state: ListState,
    current_diff: Vec<Line<'static>>, // Changed from String
}

impl App {
    fn new() -> App {
        let mut app = App {
            active_window: ActiveWindow::ChangedFiles,
            file_list: Vec::new(),
            file_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
        };
        app.refresh_status();
        app
    }

    fn refresh_status(&mut self) {
        let output = Command::new("svn")
            .arg("status")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        self.file_list = output
            .lines()
            .filter_map(|line| {
                if line.len() > 8 {
                    Some(SvnFile {
                        status: line[..1].to_string(),
                        path: line[8..].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        if !self.file_list.is_empty() && self.file_list_state.selected().is_none() {
            self.file_list_state.select(Some(0));
            self.refresh_diff();
        }
    }

    fn refresh_diff(&mut self) {
        if let Some(i) = self.file_list_state.selected() {
            if let Some(file) = self.file_list.get(i) {
                let output = Command::new("svn")
                    .arg("diff")
                    .arg(&file.path)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_else(|_| "Error fetching diff".into());

                // Convert raw string into styled Ratatui Lines
                self.current_diff = output
                    .lines()
                    .map(|line| {
                        if line.starts_with('+') && !line.starts_with("+++") {
                            Line::from(Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::Green),
                            ))
                        } else if line.starts_with('-') && !line.starts_with("---") {
                            Line::from(Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::Red),
                            ))
                        } else if line.starts_with("@@") {
                            Line::from(Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::Cyan),
                            ))
                        } else {
                            Line::from(line.to_string())
                        }
                    })
                    .collect();
            }
        }
    }

    fn next_file(&mut self) {
        let i = match self.file_list_state.selected() {
            Some(i) => {
                if i >= self.file_list.iter().count() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.file_list_state.select(Some(i));
        self.refresh_diff();
    }

    fn previous_file(&mut self) {
        let i = match self.file_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.file_list.iter().count() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.file_list_state.select(Some(i));
        self.refresh_diff();
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

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
                    KeyCode::Char('j') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.next_file()
                        }
                    }
                    KeyCode::Char('k') => {
                        if app.active_window == ActiveWindow::ChangedFiles {
                            app.previous_file()
                        }
                    }
                    KeyCode::Char('r') => app.refresh_status(), // Manual refresh
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(f.area());

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(chunks[0]);

    // Render File List
    let items: Vec<ListItem> = app
        .file_list
        .iter()
        .map(|file| {
            let style = match file.status.as_str() {
                "M" => Style::default().fg(Color::Blue),
                "A" => Style::default().fg(Color::Green),
                "D" => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::White),
            };
            ListItem::new(format!(" {}  {}", file.status, file.path)).style(style)
        })
        .collect();

    let border_color = if app.active_window == ActiveWindow::ChangedFiles {
        Color::Yellow
    } else {
        Color::Gray
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(" Files (j/k) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 60, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, left_chunks[0], &mut app.file_list_state);

    // Other Windows (Placeholders)
    f.render_widget(
        Block::default().title(" Branches ").borders(Borders::ALL),
        left_chunks[1],
    );
    f.render_widget(
        Block::default().title(" Revisions ").borders(Borders::ALL),
        left_chunks[2],
    );

    // Diff View
    let diff_style = if app.active_window == ActiveWindow::Diff {
        Color::Yellow
    } else {
        Color::Gray
    };

    // Pass the pre-styled lines from our app state
    let diff_paragraph = Paragraph::new(app.current_diff.clone()).block(
        Block::default()
            .title(" Diff View ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(diff_style)),
    );

    f.render_widget(diff_paragraph, chunks[1]);
}
