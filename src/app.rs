use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::ListState,
};
use std::process::Command;

use crate::types::{ActiveWindow, SvnFile, SvnRevision};

pub struct App {
    pub active_window: ActiveWindow,
    pub file_list: Vec<SvnFile>,
    pub file_list_state: ListState,
    pub current_diff: Vec<Line<'static>>,
    pub diff_scroll: u16,
    pub revision_list: Vec<SvnRevision>,
    pub revision_list_state: ListState,
    pub working_copy_revision: Option<String>,
    pub repository_url: Option<String>,
}

impl App {
    pub fn new() -> App {
        let mut app = App {
            active_window: ActiveWindow::ChangedFiles,
            file_list: Vec::new(),
            file_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
            diff_scroll: 0,
            revision_list: Vec::new(),
            revision_list_state: ListState::default(),
            working_copy_revision: None,
            repository_url: None,
        };
        app.refresh_status();
        app.refresh_log();
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

        self.refresh_working_copy_revision();
    }

    pub fn refresh_working_copy_revision(&mut self) {
        let output = Command::new("svn")
            .arg("info")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        self.working_copy_revision = output.lines().find_map(|line| {
            line.strip_prefix("Revision: ").map(|r| r.trim().to_string())
        });

        self.repository_url = output.lines().find_map(|line| {
            line.strip_prefix("URL: ").map(|u| u.trim().to_string())
        });
    }

    pub fn refresh_log(&mut self) {
        // Use -r HEAD:1 so that revisions on the remote that are newer than
        // the working copy are also included in the list.
        let output = Command::new("svn")
            .arg("log")
            .arg("-r")
            .arg("HEAD:1")
            .arg("--limit")
            .arg("50")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        self.revision_list = output
            .split("------------------------------------------------------------------------")
            .filter_map(|block| {
                let block = block.trim();
                if block.is_empty() {
                    return None;
                }
                let mut lines = block.lines();
                let header = lines.next()?;
                let parts: Vec<&str> = header.splitn(4, " | ").collect();
                if parts.len() >= 3 {
                    // Skip the blank line between header and message
                    lines.next();
                    let message = lines
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string();
                    Some(SvnRevision {
                        revision: parts[0].to_string(),
                        author: parts[1].to_string(),
                        date: parts[2]
                            .splitn(2, ' ')
                            .take(2)
                            .collect::<Vec<_>>()
                            .join(" "),
                        message,
                    })
                } else {
                    None
                }
            })
            .collect();

        if !self.revision_list.is_empty() && self.revision_list_state.selected().is_none() {
            self.revision_list_state.select(Some(0));
            self.refresh_revision_diff();
        }
    }

    fn revision_number(revision: &str) -> &str {
        revision.trim_start_matches('r')
    }

    pub fn refresh_revision_diff(&mut self) {
        if let Some(i) = self.revision_list_state.selected() {
            if let Some(rev) = self.revision_list.get(i) {
                let rev_num = Self::revision_number(&rev.revision);
                if !rev_num.chars().all(|c| c.is_ascii_digit()) {
                    self.current_diff =
                        vec![Line::from("Invalid revision number".to_string())];
                    self.diff_scroll = 0;
                    return;
                }
                let mut cmd = Command::new("svn");
                cmd.arg("diff").arg("-c").arg(rev_num);
                if let Some(url) = &self.repository_url {
                    cmd.arg(url);
                }
                let output = cmd
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_else(|_| "Error fetching revision diff".into());

                self.current_diff = Self::style_diff_output(&output);
                self.diff_scroll = 0;
            }
        }
    }

    pub fn next_revision(&mut self) {
        let len = self.revision_list.len();
        if len == 0 {
            return;
        }
        let i = match self.revision_list_state.selected() {
            Some(i) => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.revision_list_state.select(Some(i));
        self.refresh_revision_diff();
    }

    pub fn previous_revision(&mut self) {
        let len = self.revision_list.len();
        if len == 0 {
            return;
        }
        let i = match self.revision_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.revision_list_state.select(Some(i));
        self.refresh_revision_diff();
    }

    pub fn update_to_revision(&mut self) {
        if let Some(i) = self.revision_list_state.selected() {
            if let Some(rev) = self.revision_list.get(i) {
                let rev_num = Self::revision_number(&rev.revision);
                if !rev_num.chars().all(|c| c.is_ascii_digit()) {
                    return;
                }
                Command::new("svn")
                    .arg("update")
                    .arg("-r")
                    .arg(rev_num)
                    .output()
                    .ok();
                self.refresh_status();
            }
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

                self.current_diff = Self::style_diff_output(&output);
                self.diff_scroll = 0;
            }
        }
    }

    fn style_diff_output(output: &str) -> Vec<Line<'static>> {
        output
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
            .collect()
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
