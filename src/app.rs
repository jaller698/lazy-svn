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
    pub branch_list: Vec<String>,
    pub branch_list_state: ListState,
    pub current_diff: Vec<Line<'static>>,
    pub diff_scroll: u16,
}

impl App {
    pub fn new() -> App {
        let mut app = App {
            active_window: ActiveWindow::ChangedFiles,
            file_list: Vec::new(),
            file_list_state: ListState::default(),
            branch_list: Vec::new(),
            branch_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
            diff_scroll: 0,
        };
        app.refresh_status();
        app.refresh_branches();
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
                self.diff_scroll = 0;
            }
        }
    }

    pub fn refresh_branches(&mut self) {
        // Get the working copy URL and derive the branches URL by replacing /trunk with /branches
        let wc_url = Command::new("svn")
            .arg("info")
            .arg("--show-item")
            .arg("url")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

        let wc_url = match wc_url {
            Some(url) if !url.is_empty() => url,
            _ => {
                self.branch_list = vec!["Error: could not determine working copy URL".to_string()];
                self.branch_list_state = ListState::default();
                return;
            }
        };

        let branches_url = wc_url.replace("/trunk", "/branches");
        let result = Command::new("svn").arg("list").arg(&branches_url).output();

        let output = match result {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                self.branch_list = vec![format!(
                    "Error: {}",
                    stderr.lines().next().unwrap_or("svn list failed")
                )];
                self.branch_list_state = ListState::default();
                return;
            }
            Err(_) => {
                self.branch_list = vec!["Error: failed to run svn".to_string()];
                self.branch_list_state = ListState::default();
                return;
            }
        };

        self.branch_list = output
            .lines()
            // `svn list` appends a trailing slash to directory entries (branches are directories)
            .map(|line| line.trim_end_matches('/').to_string())
            .filter(|line| !line.is_empty())
            .collect();

        if !self.branch_list.is_empty() && self.branch_list_state.selected().is_none() {
            self.branch_list_state.select(Some(0));
        }
    }

    pub fn next_branch(&mut self) {
        let i = match self.branch_list_state.selected() {
            Some(i) => {
                if i >= self.branch_list.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.branch_list_state.select(Some(i));
    }

    pub fn previous_branch(&mut self) {
        let i = match self.branch_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.branch_list.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.branch_list_state.select(Some(i));
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

    pub fn scroll_diff_down(&mut self) {
        let max_scroll = self.current_diff.len().saturating_sub(1) as u16;
        if self.diff_scroll < max_scroll {
            self.diff_scroll += 1;
        }
    }

    pub fn scroll_diff_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(1);
    }

    pub fn scroll_diff_next_hunk(&mut self) {
        let start = (self.diff_scroll as usize).saturating_add(1);
        if let Some(offset) = self.current_diff[start..].iter().position(|line| {
            line.spans
                .first()
                .map_or(false, |s| s.content.starts_with("@@"))
        }) {
            self.diff_scroll = (start + offset) as u16;
        }
    }

    pub fn scroll_diff_prev_hunk(&mut self) {
        let end = self.diff_scroll as usize;
        if let Some(pos) = self.current_diff[..end].iter().rposition(|line| {
            line.spans
                .first()
                .map_or(false, |s| s.content.starts_with("@@"))
        }) {
            self.diff_scroll = pos as u16;
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
