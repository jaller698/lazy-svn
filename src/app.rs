use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::ListState,
};
use std::collections::{BTreeSet, HashSet};
use std::process::Command;

use crate::types::{ActiveWindow, FileTreeNode, SvnFile, SvnRevision};
use log::{debug, error, info, warn};

pub struct App {
    pub active_window: ActiveWindow,
    /// Window that was active before the help popup was opened; used to
    /// restore focus when help is closed.
    pub prev_window: Option<ActiveWindow>,
    pub file_list: Vec<SvnFile>,
    pub file_list_state: ListState,
    /// Flat list of tree entries currently visible (respects collapsed dirs).
    pub visible_items: Vec<FileTreeNode>,
    /// Set of directory paths (with trailing `/`) that are currently collapsed.
    pub collapsed_dirs: HashSet<String>,
    pub branch_list: Vec<String>,
    pub branch_list_state: ListState,
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
            prev_window: None,
            file_list: Vec::new(),
            file_list_state: ListState::default(),
            visible_items: Vec::new(),
            collapsed_dirs: HashSet::new(),
            branch_list: Vec::new(),
            branch_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
            diff_scroll: 0,
            revision_list: Vec::new(),
            revision_list_state: ListState::default(),
            working_copy_revision: None,
            repository_url: None,
        };
        app.refresh_status();
        app.refresh_branches();
        app.refresh_log();
        app
    }

    /// Open the help window, saving the current active window so it can be
    /// restored when help is closed.
    pub fn open_help(&mut self) {
        self.prev_window = Some(self.active_window.clone());
        self.active_window = ActiveWindow::Help;
        log::debug!("Opened help window");
    }

    /// Close the help window and restore the previously active window.
    pub fn close_help(&mut self) {
        self.active_window = self
            .prev_window
            .take()
            .unwrap_or(ActiveWindow::ChangedFiles);
        log::debug!("Closed help window, restored to {:?}", self.active_window);
    }

    /// Construct a minimal `App` for use in tests without running any SVN commands.
    #[cfg(test)]
    pub fn test_new() -> App {
        App {
            active_window: ActiveWindow::ChangedFiles,
            prev_window: None,
            file_list: Vec::new(),
            file_list_state: ListState::default(),
            visible_items: Vec::new(),
            collapsed_dirs: HashSet::new(),
            branch_list: Vec::new(),
            branch_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
            diff_scroll: 0,
            revision_list: Vec::new(),
            revision_list_state: ListState::default(),
            working_copy_revision: None,
            repository_url: None,
        }
    }

    pub fn refresh_status(&mut self) {
        debug!("Refreshing SVN status");
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

        info!("SVN status: {} changed file(s)", self.file_list.len());

        self.rebuild_visible_items();

        if !self.visible_items.is_empty() && self.file_list_state.selected().is_none() {
            // Select the first file entry (skip directory rows at the top).
            let first_file = self
                .visible_items
                .iter()
                .position(|n| matches!(n, FileTreeNode::File { .. }));
            if let Some(idx) = first_file {
                self.file_list_state.select(Some(idx));
                self.refresh_diff();
            } else {
                self.file_list_state.select(Some(0));
            }
        }

        self.refresh_working_copy_revision();
    }

    /// Rebuild `visible_items` from `file_list` while honouring `collapsed_dirs`.
    /// The current selection index is clamped so it stays in-bounds.
    fn rebuild_visible_items(&mut self) {
        let mut result = Vec::new();
        Self::build_tree_for_prefix("", 0, &self.file_list, &self.collapsed_dirs, &mut result);
        self.visible_items = result;

        // Keep the selection in-bounds after a rebuild.
        let len = self.visible_items.len();
        if len == 0 {
            self.file_list_state.select(None);
        } else if let Some(sel) = self.file_list_state.selected() {
            if sel >= len {
                self.file_list_state.select(Some(len - 1));
            }
        }
    }

    /// Recursively populate `result` with the visible entries rooted at `prefix`.
    ///
    /// * Directories are listed first (alphabetically), then files at the
    ///   same level.
    /// * Children of a collapsed directory are omitted.
    fn build_tree_for_prefix(
        prefix: &str,
        depth: usize,
        file_list: &[SvnFile],
        collapsed_dirs: &HashSet<String>,
        result: &mut Vec<FileTreeNode>,
    ) {
        let mut subdirs: BTreeSet<String> = BTreeSet::new();
        let mut files_at_level: Vec<&SvnFile> = Vec::new();

        for file in file_list {
            if !file.path.starts_with(prefix) {
                continue;
            }
            let rest = &file.path[prefix.len()..];
            if let Some(slash_pos) = rest.find('/') {
                // There is a sub-directory component – record the immediate child dir.
                let subdir = format!("{}{}/", prefix, &rest[..slash_pos]);
                subdirs.insert(subdir);
            } else {
                files_at_level.push(file);
            }
        }

        // Emit directory rows and recurse into them (unless collapsed).
        for dir_path in &subdirs {
            let trimmed = dir_path.trim_end_matches('/');
            let name = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
            let collapsed = collapsed_dirs.contains(dir_path.as_str());
            result.push(FileTreeNode::Dir {
                path: dir_path.clone(),
                name,
                depth,
                collapsed,
            });
            if !collapsed {
                Self::build_tree_for_prefix(dir_path, depth + 1, file_list, collapsed_dirs, result);
            }
        }

        // Emit file rows at this level (sorted by path for stability).
        files_at_level.sort_by(|a, b| a.path.cmp(&b.path));
        for file in files_at_level {
            let name = file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&file.path)
                .to_string();
            result.push(FileTreeNode::File {
                status: file.status.clone(),
                path: file.path.clone(),
                name,
                depth,
            });
        }
    }

    /// Toggle the collapsed state of the directory that is currently selected,
    /// then rebuild the visible items.
    pub fn toggle_folder(&mut self) {
        if let Some(i) = self.file_list_state.selected() {
            if let Some(FileTreeNode::Dir { path, .. }) = self.visible_items.get(i) {
                let path = path.clone();
                if self.collapsed_dirs.contains(&path) {
                    self.collapsed_dirs.remove(&path);
                } else {
                    self.collapsed_dirs.insert(path);
                }
                self.rebuild_visible_items();
            }
        }
    }

    pub fn refresh_working_copy_revision(&mut self) {
        debug!("Refreshing working copy revision info");
        let output = Command::new("svn")
            .arg("info")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        self.working_copy_revision = output.lines().find_map(|line| {
            line.strip_prefix("Revision: ")
                .map(|r| r.trim().to_string())
        });

        self.repository_url = output
            .lines()
            .find_map(|line| line.strip_prefix("URL: ").map(|u| u.trim().to_string()));

        match (&self.working_copy_revision, &self.repository_url) {
            (Some(rev), Some(url)) => info!("Working copy: revision {rev}, url {url}"),
            (Some(rev), None) => warn!("Working copy: revision {rev}, URL not found"),
            (None, _) => warn!("Could not determine working copy revision"),
        }
    }

    pub fn refresh_log(&mut self) {
        // Use -r HEAD:1 so that revisions on the remote that are newer than
        // the working copy are also included in the list.
        debug!("Fetching SVN log");
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
                    let message = lines.collect::<Vec<_>>().join(" ").trim().to_string();
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
        info!("SVN log: {} revision(s) loaded", self.revision_list.len());
    }

    fn revision_number(revision: &str) -> &str {
        revision.trim_start_matches('r')
    }

    pub fn refresh_revision_diff(&mut self) {
        if let Some(i) = self.revision_list_state.selected() {
            if let Some(rev) = self.revision_list.get(i) {
                let rev_num = Self::revision_number(&rev.revision);
                debug!("Fetching diff for revision {rev_num}");
                if !rev_num.chars().all(|c| c.is_ascii_digit()) {
                    warn!("Invalid revision number: {rev_num}");
                    self.current_diff = vec![Line::from("Invalid revision number".to_string())];
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
                    .unwrap_or_else(|e| {
                        error!("Failed to fetch revision diff for {rev_num}: {e}");
                        "Error fetching revision diff".into()
                    });

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
                    warn!("update_to_revision: invalid revision number: {rev_num}");
                    return;
                }
                info!("Updating working copy to revision {rev_num}");
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
            if let Some(FileTreeNode::File { path, .. }) = self.visible_items.get(i) {
                let path = path.clone();
                debug!("Fetching diff for file: {}", path);
                let output = Command::new("svn")
                    .arg("diff")
                    .arg(&path)
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                    .unwrap_or_else(|e| {
                        error!("Failed to fetch diff for {}: {e}", path);
                        "Error fetching diff".into()
                    });

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

    pub fn refresh_branches(&mut self) {
        // Get the working copy URL and derive the branches URL by replacing /trunk with /branches
        debug!("Refreshing branch list");
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
                warn!("Could not determine working copy URL for branch listing");
                self.branch_list = vec!["Error: could not determine working copy URL".to_string()];
                self.branch_list_state = ListState::default();
                return;
            }
        };

        let branches_url = wc_url.replace("/trunk", "/branches");
        debug!("Listing branches at {branches_url}");
        let result = Command::new("svn").arg("list").arg(&branches_url).output();

        let output = match result {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).to_string();
                let msg = stderr
                    .lines()
                    .next()
                    .unwrap_or("svn list failed")
                    .to_string();
                error!("svn list failed for {branches_url}: {msg}");
                self.branch_list = vec![format!("Error: {msg}")];
                self.branch_list_state = ListState::default();
                return;
            }
            Err(e) => {
                error!("Failed to run svn list: {e}");
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

        info!("Branch list: {} branch(es) loaded", self.branch_list.len());

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
        let len = self.visible_items.len();
        if len == 0 {
            return;
        }
        let i = match self.file_list_state.selected() {
            Some(i) => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.file_list_state.select(Some(i));
        if matches!(self.visible_items.get(i), Some(FileTreeNode::File { .. })) {
            self.refresh_diff();
        }
    }

    pub fn previous_file(&mut self) {
        let len = self.visible_items.len();
        if len == 0 {
            return;
        }
        let i = match self.file_list_state.selected() {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.file_list_state.select(Some(i));
        if matches!(self.visible_items.get(i), Some(FileTreeNode::File { .. })) {
            self.refresh_diff();
        }
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
