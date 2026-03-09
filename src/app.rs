use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::ListState,
};
use std::process::Command;

use crate::types::{ActiveWindow, SvnFile};

pub struct App {
    pub active_window: ActiveWindow,
    pub file_list: Vec<SvnFile>,
    pub file_list_state: ListState,
    pub current_diff: Vec<Line<'static>>,
}

impl App {
    pub fn new() -> App {
        let mut app = App {
            active_window: ActiveWindow::ChangedFiles,
            file_list: Vec::new(),
            file_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
        };
        app.refresh_status();
        app
    }

    pub fn refresh_status(&mut self) {
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

    pub fn refresh_diff(&mut self) {
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

    pub fn next_file(&mut self) {
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

    pub fn previous_file(&mut self) {
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
