use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::ListState,
};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::types::{ActiveWindow, CommitField, FileTreeNode, SvnFile, SvnRevision, SvnRevisionFile};
use log::{debug, error, info, warn};

const REVISION_LOAD_BATCH_SIZE: usize = 50;

/// Returns the path to the lazysvn ignore file (`~/.config/lazysvn/ignore`).
fn ignore_file_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("lazysvn")
        .join("ignore")
}

/// Returns the path used to persist a commit message draft
/// (`~/.local/share/lazy-svn/commit_draft.txt`).
fn draft_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("lazy-svn")
        .join("commit_draft.txt")
}

/// Returns the path used to persist the set of selected files
/// (`~/.local/share/lazy-svn/selected_files.txt`).
fn selection_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("lazy-svn")
        .join("selected_files.txt")
}

/// Returns `true` if `path` matches `pattern` using simple glob-style rules:
/// - Lines starting with `#` are comments and are ignored by the caller.
/// - An empty pattern never matches.
/// - `*` in a pattern matches any sequence of characters that does not cross
///   a directory boundary (i.e. does not contain `/`).
/// - `**` matches any sequence of characters including `/`.
/// - A pattern without wildcards is compared as an exact path or as a suffix
///   starting at a directory boundary.
pub fn matches_ignore_pattern(path: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    // Compile the pattern into a regex-like check using manual glob expansion.
    glob_match(path, pattern)
}

/// Minimal glob matcher that supports `*` (no path separator) and `**` (any).
fn glob_match(path: &str, pattern: &str) -> bool {
    // Fast path: no wildcards.
    if !pattern.contains('*') {
        // Exact match or trailing-segment match (e.g. pattern "foo.txt" matches "src/foo.txt").
        if path == pattern {
            return true;
        }
        // Match at a directory boundary.
        let boundary = format!("/{}", pattern);
        return path.ends_with(&boundary) || path == pattern;
    }

    // If the pattern contains no `/`, match against the basename only (like .gitignore).
    if !pattern.contains('/') {
        let basename = path.rsplit('/').next().unwrap_or(path);
        return glob_match_recursive(basename.as_bytes(), pattern.as_bytes());
    }

    // Pattern contains `/`: match against the full path.
    glob_match_recursive(path.as_bytes(), pattern.as_bytes())
}

fn glob_match_recursive(path: &[u8], pattern: &[u8]) -> bool {
    let mut pi = 0usize;
    let mut gi = 0usize;

    while gi < pattern.len() {
        if pattern[gi] == b'*' {
            // Check for `**`.
            if gi + 1 < pattern.len() && pattern[gi + 1] == b'*' {
                let rest_pattern = &pattern[(gi + 2)..];
                // `**` matches zero or more path components (including `/`).
                // Try matching the rest of the pattern at every position in path.
                for start in pi..=path.len() {
                    if glob_match_recursive(&path[start..], rest_pattern) {
                        return true;
                    }
                }
                return false;
            } else {
                let rest_pattern = &pattern[(gi + 1)..];
                // Single `*`: match any sequence that does not contain `/`.
                for start in pi..=path.len() {
                    if start > pi && path[start - 1] == b'/' {
                        break;
                    }
                    if glob_match_recursive(&path[start..], rest_pattern) {
                        return true;
                    }
                }
                return false;
            }
        }

        if pi >= path.len() {
            return false;
        }

        if pattern[gi] != path[pi] {
            return false;
        }

        pi += 1;
        gi += 1;
    }

    pi == path.len()
}

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
    /// Set of file paths that the user has marked for the next commit.
    pub selected_files: HashSet<String>,
    pub branch_list: Vec<String>,
    pub branch_list_state: ListState,
    pub current_diff: Vec<Line<'static>>,
    pub diff_scroll: u16,
    /// The working-copy file path whose diff is currently shown, or `None`
    /// when the diff shows a revision or nothing.
    pub current_diff_file: Option<String>,
    pub revision_list: Vec<SvnRevision>,
    pub revision_log_limit: usize,
    pub revision_list_state: ListState,
    /// Files changed in the currently selected revision (populated by
    /// `refresh_revision_files()`).
    pub revision_files: Vec<SvnRevisionFile>,
    pub revision_file_list_state: ListState,
    pub working_copy_revision: Option<String>,
    pub repository_url: Option<String>,
    /// Commit message being composed in the commit popup.
    pub commit_message: String,
    /// Byte offset of the cursor within `commit_message`.
    /// Always kept at a valid UTF-8 char boundary.
    pub commit_message_cursor: usize,
    /// Optional SVN username for the commit.
    pub commit_username: String,
    /// Optional SVN password for the commit.
    /// The password is stored as a plain `String` and is cleared immediately after
    /// the commit completes or is cancelled. It is never written to disk by this
    /// application, but may briefly be visible in process argument lists when passed
    /// to `svn` via `--password`. Users who require stronger security should rely on
    /// SVN's own credential caching (~/.subversion/auth) instead.
    pub commit_password: String,
    /// Which field is currently active in the commit popup.
    pub commit_active_field: CommitField,
    /// Targets pending y/n confirmation before `svn delete` runs.
    pub delete_targets: Vec<String>,
    /// Targets pending y/n confirmation before `svn revert` runs.
    pub revert_targets: Vec<String>,
    /// Backup created before the last `svn delete`.
    /// Tuple of (backup_dir, original_paths); cleared after undo.
    pub last_backup: Option<(PathBuf, Vec<String>)>,
    /// Patterns loaded from `~/.config/lazysvn/ignore`; files matching any of
    /// these are hidden from the Files panel.
    pub ignore_patterns: Vec<String>,
    /// The file path pending y/n confirmation before being added to the ignore file.
    pub ignore_target: Option<String>,
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
            selected_files: HashSet::new(),
            branch_list: Vec::new(),
            branch_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
            diff_scroll: 0,
            current_diff_file: None,
            revision_list: Vec::new(),
            revision_log_limit: REVISION_LOAD_BATCH_SIZE,
            revision_list_state: ListState::default(),
            revision_files: Vec::new(),
            revision_file_list_state: ListState::default(),
            working_copy_revision: None,
            repository_url: None,
            commit_message: String::new(),
            commit_message_cursor: 0,
            commit_username: String::new(),
            commit_password: String::new(),
            commit_active_field: CommitField::Message,
            delete_targets: Vec::new(),
            revert_targets: Vec::new(),
            last_backup: None,
            ignore_patterns: Vec::new(),
            ignore_target: None,
        };
        app.load_ignore_patterns();
        app.refresh_status();
        app.load_selection();
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
            selected_files: HashSet::new(),
            branch_list: Vec::new(),
            branch_list_state: ListState::default(),
            current_diff: vec![String::from("Select a file to see diff").into()],
            diff_scroll: 0,
            current_diff_file: None,
            revision_list: Vec::new(),
            revision_log_limit: REVISION_LOAD_BATCH_SIZE,
            revision_list_state: ListState::default(),
            revision_files: Vec::new(),
            revision_file_list_state: ListState::default(),
            working_copy_revision: None,
            repository_url: None,
            commit_message: String::new(),
            commit_message_cursor: 0,
            commit_username: String::new(),
            commit_password: String::new(),
            commit_active_field: CommitField::Message,
            delete_targets: Vec::new(),
            revert_targets: Vec::new(),
            last_backup: None,
            ignore_patterns: Vec::new(),
            ignore_target: None,
        }
    }

    pub fn refresh_status(&mut self) {
        debug!("Refreshing SVN status");
        let output = Command::new("svn")
            .arg("status")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        let patterns = self.ignore_patterns.clone();
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
            .filter(|f| {
                !patterns
                    .iter()
                    .any(|p| matches_ignore_pattern(&f.path, p))
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

    /// Toggle whether the currently selected item is marked.
    /// - For a **file**: toggle that single file.
    /// - For a **directory**: toggle all files under that directory prefix.
    ///   If all children are already selected, they are deselected; otherwise all are selected.
    pub fn toggle_file_selection(&mut self) {
        if let Some(i) = self.file_list_state.selected() {
            match self.visible_items.get(i).cloned() {
                Some(FileTreeNode::File { path, .. }) => {
                    if self.selected_files.contains(&path) {
                        self.selected_files.remove(&path);
                    } else {
                        self.selected_files.insert(path);
                    }
                }
                Some(FileTreeNode::Dir { path: dir_path, .. }) => {
                    // `dir_path` always ends with `/` (e.g. `"src/"`), so
                    // `starts_with` only matches real children and not paths
                    // that merely share a common prefix (e.g. `"srcbar/..."`).
                    let children: Vec<String> = self
                        .file_list
                        .iter()
                        .filter(|f| f.path.starts_with(dir_path.as_str()))
                        .map(|f| f.path.clone())
                        .collect();

                    if children.is_empty() {
                        return;
                    }

                    let all_selected = children.iter().all(|p| self.selected_files.contains(p));

                    if all_selected {
                        for p in &children {
                            self.selected_files.remove(p);
                        }
                    } else {
                        for p in children {
                            self.selected_files.insert(p);
                        }
                    }
                }
                None => {}
            }
        }
        self.save_selection();
    }

    /// Run `svn delete` on the marked files/folders.
    /// If no files are marked, operates on the currently selected item.
    /// Transitions to the ConfirmDelete window so the user can confirm.
    pub fn svn_delete_marked(&mut self) {
        let targets: Vec<String> = if self.selected_files.is_empty() {
            // Fall back to whatever item is currently highlighted.
            if let Some(i) = self.file_list_state.selected() {
                match self.visible_items.get(i) {
                    Some(FileTreeNode::File { path, .. }) => vec![path.clone()],
                    Some(FileTreeNode::Dir { path, .. }) => {
                        // `path` always ends with `/` so `starts_with` is an
                        // exact directory-boundary match (won't match sibling
                        // dirs sharing a common prefix).
                        self.file_list
                            .iter()
                            .filter(|f| f.path.starts_with(path.as_str()))
                            .map(|f| f.path.clone())
                            .collect()
                    }
                    None => vec![],
                }
            } else {
                vec![]
            }
        } else {
            self.selected_files.iter().cloned().collect()
        };

        if targets.is_empty() {
            debug!("svn_delete_marked: nothing to delete");
            return;
        }

        self.delete_targets = targets;
        self.active_window = ActiveWindow::ConfirmDelete;
    }

    /// Called when the user confirms the delete prompt (presses 'y').
    /// Backs up the targets to `/tmp/lazy-svn/<timestamp>/` then runs
    /// `svn delete --force`, and stores the backup location for undo.
    pub fn confirm_delete(&mut self) {
        let targets: Vec<String> = std::mem::take(&mut self.delete_targets);
        if targets.is_empty() {
            self.active_window = ActiveWindow::ChangedFiles;
            return;
        }

        // Create a timestamped backup directory (owner-only permissions on Unix).
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup_dir = PathBuf::from(format!("/tmp/lazy-svn/{}", timestamp));
        if let Err(e) = fs::create_dir_all(&backup_dir) {
            error!("Failed to create backup root {:?}: {}", backup_dir, e);
        }
        #[cfg(unix)]
        {
            let perms = fs::Permissions::from_mode(0o700);
            if let Err(e) = fs::set_permissions(&backup_dir, perms) {
                error!("Failed to set permissions on backup dir: {}", e);
            }
        }

        // Copy each target file into the backup directory, preserving relative paths.
        // SVN status paths are always relative (e.g. "src/main.rs"); trim_start_matches('/')
        // is a no-op for them but keeps the join safe for absolute paths.
        for target in &targets {
            let backup_path = backup_dir.join(target.trim_start_matches('/'));
            if let Some(parent) = backup_path.parent() {
                if let Err(e) = fs::create_dir_all(parent) {
                    error!("Failed to create backup dir {:?}: {}", parent, e);
                }
            }
            if Path::new(target).is_file() {
                if let Err(e) = fs::copy(target, &backup_path) {
                    error!("Failed to backup {}: {}", target, e);
                }
            }
        }

        info!("Running svn delete for {} item(s)", targets.len());
        debug!("svn delete targets: {:?}", targets);
        let mut cmd = Command::new("svn");
        cmd.arg("delete").arg("--force").args(&targets);
        debug!("Running command: {:?}", cmd);
        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("svn delete failed: {stderr}");
                }
            }
            Err(e) => error!("Failed to run svn delete: {e}"),
        }

        self.last_backup = Some((backup_dir, targets));
        self.selected_files.clear();
        self.save_selection();
        self.active_window = ActiveWindow::ChangedFiles;
        self.refresh_status();
    }

    /// Undo the last delete by restoring files from the backup directory.
    /// Only one level of undo is supported; calling this a second time without
    /// an intervening delete does nothing.
    pub fn undo_last_delete(&mut self) {
        let Some((backup_dir, paths)) = self.last_backup.take() else {
            debug!("undo_last_delete: nothing to undo");
            return;
        };

        info!("Undoing last delete ({} item(s))", paths.len());
        for original_path in &paths {
            let backup_path = backup_dir.join(original_path.trim_start_matches('/'));
            if backup_path.exists() {
                // Recreate parent directory if needed.
                if let Some(parent) = Path::new(original_path).parent() {
                    if !parent.as_os_str().is_empty() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            error!("Failed to recreate dir {:?}: {}", parent, e);
                        }
                    }
                }
                if let Err(e) = fs::copy(&backup_path, original_path) {
                    error!("Failed to restore {}: {}", original_path, e);
                }
            } else {
                warn!(
                    "undo_last_delete: backup missing for '{}' (expected at {:?}), skipping",
                    original_path, backup_path
                );
            }
        }

        // Revert SVN state for any previously-versioned files.
        let mut cmd = Command::new("svn");
        cmd.arg("revert")
            .arg("--depth")
            .arg("infinity")
            .args(&paths);
        if let Err(e) = cmd.output() {
            error!("Failed to run svn revert during undo: {e}");
        }

        self.refresh_status();
    }

    /// Load ignore patterns from `~/.config/lazysvn/ignore`, creating the file
    /// (and its parent directories) if it does not yet exist.
    pub fn load_ignore_patterns(&mut self) {
        let path = ignore_file_path();
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("Could not create ignore file directory {:?}: {}", parent, e);
            }
        }
        // Create the file if it does not exist yet.
        if !path.exists() {
            if let Err(e) = fs::write(&path, "") {
                warn!("Could not create ignore file {:?}: {}", path, e);
            }
        }
        match fs::read_to_string(&path) {
            Ok(contents) => {
                self.ignore_patterns = contents
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .collect();
                info!(
                    "Loaded {} ignore pattern(s) from {:?}",
                    self.ignore_patterns.len(),
                    path
                );
            }
            Err(e) => {
                warn!("Could not read ignore file {:?}: {}", path, e);
                self.ignore_patterns = Vec::new();
            }
        }
    }

    /// If the currently hovered item is a file, store it as `ignore_target` and
    /// open the ConfirmIgnore popup.
    pub fn ignore_current_file(&mut self) {
        if let Some(i) = self.file_list_state.selected() {
            if let Some(FileTreeNode::File { path, .. }) = self.visible_items.get(i) {
                self.ignore_target = Some(path.clone());
                self.active_window = ActiveWindow::ConfirmIgnore;
                debug!("Opened ignore confirmation for '{}'", path);
            }
        }
    }

    /// Called when the user confirms the ignore prompt (presses 'y').
    /// Appends the target path to the ignore file and refreshes the file list.
    pub fn confirm_ignore(&mut self) {
        let Some(target) = self.ignore_target.take() else {
            self.active_window = ActiveWindow::ChangedFiles;
            return;
        };

        let path = ignore_file_path();
        // Read existing contents so we can append without duplicating.
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let already_present = existing
            .lines()
            .any(|l| l.trim() == target.as_str());

        if !already_present {
            let new_line = if existing.ends_with('\n') || existing.is_empty() {
                format!("{}\n", target)
            } else {
                format!("\n{}\n", target)
            };
            match fs::OpenOptions::new().create(true).append(true).open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    if let Err(e) = file.write_all(new_line.as_bytes()) {
                        error!("Failed to write to ignore file {:?}: {}", path, e);
                    } else {
                        info!("Added '{}' to ignore file", target);
                    }
                }
                Err(e) => error!("Failed to open ignore file {:?}: {}", path, e),
            }
        } else {
            debug!("'{}' is already in the ignore file", target);
        }

        self.selected_files.remove(&target);
        self.save_selection();
        self.load_ignore_patterns();
        self.active_window = ActiveWindow::ChangedFiles;
        self.refresh_status();
    }

    /// Prepare `svn revert` for the marked files/folders.
    /// If no files are marked, operates on the currently selected item.
    /// Transitions to the ConfirmRevert window so the user can confirm.
    pub fn svn_revert_marked(&mut self) {
        let targets: Vec<String> = if self.selected_files.is_empty() {
            if let Some(i) = self.file_list_state.selected() {
                match self.visible_items.get(i) {
                    Some(FileTreeNode::File { path, .. }) => vec![path.clone()],
                    Some(FileTreeNode::Dir { path, .. }) => {
                        // `path` always ends with `/` so `starts_with` is an
                        // exact directory-boundary match.
                        self.file_list
                            .iter()
                            .filter(|f| f.path.starts_with(path.as_str()))
                            .map(|f| f.path.clone())
                            .collect()
                    }
                    None => vec![],
                }
            } else {
                vec![]
            }
        } else {
            self.selected_files.iter().cloned().collect()
        };

        if targets.is_empty() {
            debug!("svn_revert_marked: nothing to revert");
            return;
        }

        self.revert_targets = targets;
        self.active_window = ActiveWindow::ConfirmRevert;
    }

    /// Called when the user confirms the revert prompt (presses 'y').
    /// Runs `svn revert --depth infinity` on the pending targets.
    pub fn confirm_revert(&mut self) {
        let targets: Vec<String> = std::mem::take(&mut self.revert_targets);
        if targets.is_empty() {
            self.active_window = ActiveWindow::ChangedFiles;
            return;
        }

        info!("Running svn revert for {} item(s)", targets.len());
        let mut cmd = Command::new("svn");
        cmd.arg("revert")
            .arg("--depth")
            .arg("infinity")
            .args(&targets);
        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("svn revert failed for {:?}: {stderr}", targets);
                }
            }
            Err(e) => error!("Failed to run svn revert: {e}"),
        }

        self.selected_files.clear();
        self.save_selection();
        self.active_window = ActiveWindow::ChangedFiles;
        self.refresh_status();
    }

    /// Run `svn add` on the marked files that have unversioned (`?`) status.
    /// If no files are marked, adds all unversioned files in the file list.
    /// Refreshes the status afterwards.
    pub fn svn_add_marked(&mut self) {
        let status_map: std::collections::HashMap<&str, &str> = self
            .file_list
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str()))
            .collect();

        let candidates: Vec<&str> = if self.selected_files.is_empty() {
            self.file_list
                .iter()
                .filter(|f| f.status == "?")
                .map(|f| f.path.as_str())
                .collect()
        } else {
            self.selected_files
                .iter()
                .filter(|p| status_map.get(p.as_str()).map_or(false, |&s| s == "?"))
                .map(|p| p.as_str())
                .collect()
        };

        if candidates.is_empty() {
            debug!("svn_add_marked: no unversioned files to add");
            return;
        }

        info!("Running svn add for {} file(s)", candidates.len());
        let mut cmd = Command::new("svn");
        cmd.arg("add").args(&candidates);
        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("svn add failed: {stderr}");
                }
            }
            Err(e) => error!("Failed to run svn add: {e}"),
        }

        self.refresh_status();
    }

    /// Returns the (line_index, column) of the commit message cursor.
    /// Both values are zero-based and measured in characters (not bytes).
    pub fn commit_cursor_line_col(&self) -> (usize, usize) {
        let before = &self.commit_message[..self.commit_message_cursor];
        let line_idx = before.matches('\n').count();
        let last_newline = before.rfind('\n').map_or(0, |i| i + 1);
        let col = before[last_newline..].chars().count();
        (line_idx, col)
    }

    /// Insert `c` at the current cursor position and advance the cursor.
    pub fn commit_message_insert_char(&mut self, c: char) {
        self.commit_message.insert(self.commit_message_cursor, c);
        self.commit_message_cursor += c.len_utf8();
    }

    /// Delete the character immediately before the cursor (backspace).
    pub fn commit_message_delete_before_cursor(&mut self) {
        if self.commit_message_cursor == 0 {
            return;
        }
        let before = &self.commit_message[..self.commit_message_cursor];
        let prev_char_len = before.chars().last().map_or(0, |c| c.len_utf8());
        let new_cursor = self.commit_message_cursor - prev_char_len;
        self.commit_message.remove(new_cursor);
        self.commit_message_cursor = new_cursor;
    }

    /// Move the commit message cursor one character to the left.
    pub fn commit_message_move_left(&mut self) {
        if self.commit_message_cursor == 0 {
            return;
        }
        let before = &self.commit_message[..self.commit_message_cursor];
        let prev_char_len = before.chars().last().map_or(0, |c| c.len_utf8());
        self.commit_message_cursor -= prev_char_len;
    }

    /// Move the commit message cursor one character to the right.
    pub fn commit_message_move_right(&mut self) {
        if self.commit_message_cursor >= self.commit_message.len() {
            return;
        }
        let next_char_len = self.commit_message[self.commit_message_cursor..]
            .chars()
            .next()
            .map_or(0, |c| c.len_utf8());
        self.commit_message_cursor += next_char_len;
    }

    /// Move the commit message cursor up one line, preserving column.
    pub fn commit_message_move_up(&mut self) {
        let (line_idx, col) = self.commit_cursor_line_col();
        if line_idx == 0 {
            // Already on the first line; move to start of line.
            self.commit_message_move_to_line_start();
            return;
        }
        let lines: Vec<&str> = self.commit_message.split('\n').collect();
        let target_line = lines[line_idx - 1];
        let target_col = col.min(target_line.chars().count());
        let start_of_target: usize = lines[..line_idx - 1]
            .iter()
            .map(|l| l.len() + 1)
            .sum();
        let col_bytes: usize = target_line
            .chars()
            .take(target_col)
            .map(|c| c.len_utf8())
            .sum();
        self.commit_message_cursor = start_of_target + col_bytes;
    }

    /// Move the commit message cursor down one line, preserving column.
    pub fn commit_message_move_down(&mut self) {
        let (line_idx, col) = self.commit_cursor_line_col();
        let lines: Vec<&str> = self.commit_message.split('\n').collect();
        if line_idx + 1 >= lines.len() {
            // Already on the last line; move to end of line.
            self.commit_message_move_to_line_end();
            return;
        }
        let target_line = lines[line_idx + 1];
        let target_col = col.min(target_line.chars().count());
        let start_of_target: usize = lines[..line_idx + 1]
            .iter()
            .map(|l| l.len() + 1)
            .sum();
        let col_bytes: usize = target_line
            .chars()
            .take(target_col)
            .map(|c| c.len_utf8())
            .sum();
        self.commit_message_cursor = start_of_target + col_bytes;
    }

    /// Move the commit message cursor to the start of the current line.
    pub fn commit_message_move_to_line_start(&mut self) {
        let before = &self.commit_message[..self.commit_message_cursor];
        let last_newline = before.rfind('\n').map_or(0, |i| i + 1);
        self.commit_message_cursor = last_newline;
    }

    /// Move the commit message cursor to the end of the current line.
    pub fn commit_message_move_to_line_end(&mut self) {
        let rest = &self.commit_message[self.commit_message_cursor..];
        let to_next_newline = rest.find('\n').unwrap_or(rest.len());
        self.commit_message_cursor += to_next_newline;
    }

    /// Persist the current commit message to disk as a draft so it survives
    /// Esc cancellations and commit failures.
    pub fn save_commit_draft(&self) {
        self.save_commit_draft_to(&draft_path());
    }

    fn save_commit_draft_to(&self, path: &Path) {
        if self.commit_message.is_empty() {
            self.clear_commit_draft_at(path);
            return;
        }
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("Could not create draft directory {:?}: {}", parent, e);
                return;
            }
        }
        if let Err(e) = fs::write(path, &self.commit_message) {
            warn!("Failed to save commit draft {:?}: {}", path, e);
        } else {
            info!("Saved commit draft to {:?}", path);
        }
    }

    /// Load a previously saved commit draft from disk into `commit_message`,
    /// placing the cursor at the end.  Does nothing if no draft exists.
    pub fn load_commit_draft(&mut self) {
        self.load_commit_draft_from(&draft_path());
    }

    fn load_commit_draft_from(&mut self, path: &Path) {
        if !path.exists() {
            return;
        }
        match fs::read_to_string(path) {
            Ok(content) if !content.is_empty() => {
                self.commit_message = content;
                self.commit_message_cursor = self.commit_message.len();
                info!("Loaded commit draft from {:?}", path);
            }
            Ok(_) => {}
            Err(e) => warn!("Failed to load commit draft {:?}: {}", path, e),
        }
    }

    /// Remove the draft file from disk (called after a successful commit).
    pub fn clear_commit_draft(&self) {
        self.clear_commit_draft_at(&draft_path());
    }

    fn clear_commit_draft_at(&self, path: &Path) {
        if path.exists() {
            if let Err(e) = fs::remove_file(path) {
                warn!("Failed to remove commit draft {:?}: {}", path, e);
            } else {
                info!("Removed commit draft {:?}", path);
            }
        }
    }

    /// Persist the current selection to disk.
    pub fn save_selection(&self) {
        self.save_selection_to(&selection_path());
    }

    fn save_selection_to(&self, path: &Path) {
        if self.selected_files.is_empty() {
            if path.exists() && fs::remove_file(path).is_err() {
                warn!("Failed to remove selection file {:?}", path);
            }
            return;
        }
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                warn!("Could not create selection directory {:?}: {}", parent, e);
                return;
            }
        }
        let mut selected: Vec<&str> = self.selected_files.iter().map(String::as_str).collect();
        selected.sort_unstable();
        let payload = format!("{}\n", selected.join("\n"));
        if let Err(e) = fs::write(path, payload) {
            warn!("Failed to save selection {:?}: {}", path, e);
        }
    }

    /// Load persisted selection from disk, ignoring files no longer present.
    pub fn load_selection(&mut self) {
        self.load_selection_from(&selection_path());
    }

    fn load_selection_from(&mut self, path: &Path) {
        self.selected_files.clear();
        let Ok(contents) = fs::read_to_string(path) else {
            return;
        };
        let known_files: HashSet<&str> = self.file_list.iter().map(|f| f.path.as_str()).collect();
        for line in contents.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if known_files.contains(line) {
                self.selected_files.insert(line.to_string());
            }
        }
    }

    /// Run `svn commit` with the current `commit_message`.
    /// Commits the explicitly selected files, or all changed files if none are selected.
    /// Clears the selection and commit message on success; keeps them on failure so the
    /// user can correct and retry.
    /// Returns `false` when the commit was not attempted (e.g. empty message) or failed.
    pub fn do_commit(&mut self) -> bool {
        let message = self.commit_message.trim().to_string();
        if message.is_empty() {
            warn!("do_commit: empty commit message, aborting");
            return false;
        }

        // Build status map to identify unversioned (`?`) files.
        let status_map: std::collections::HashMap<&str, &str> = self
            .file_list
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str()))
            .collect();

        let files: Vec<String> = if self.selected_files.is_empty() {
            self.file_list.iter().map(|f| f.path.clone()).collect()
        } else {
            self.selected_files.iter().cloned().collect()
        };

        if files.is_empty() {
            warn!("do_commit: no files to commit");
            return false;
        }

        // Run `svn add` on any unversioned (`?`) files before committing so that
        // SVN accepts them and doesn't return E200009.
        let unversioned: Vec<&str> = files
            .iter()
            .filter(|p| status_map.get(p.as_str()).map_or(false, |&s| s == "?"))
            .map(|p| p.as_str())
            .collect();

        if !unversioned.is_empty() {
            info!(
                "Running svn add for {} unversioned file(s)",
                unversioned.len()
            );
            let mut add_cmd = Command::new("svn");
            add_cmd.arg("add").args(&unversioned);
            match add_cmd.output() {
                Ok(output) => {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        error!("svn add failed for {} file(s): {stderr}", unversioned.len());
                        return false;
                    }
                }
                Err(e) => {
                    error!("Failed to run svn add: {e}");
                    return false;
                }
            }
        }

        // Similarly, run `svn delete` on any missing (`!`) files so that they are removed from the commit.
        let missing: Vec<&str> = files
            .iter()
            .filter(|p| status_map.get(p.as_str()).map_or(false, |&s| s == "!"))
            .map(|p| p.as_str())
            .collect();

        if !missing.is_empty() {
            info!("Running svn delete for {} missing file(s)", missing.len());
            let mut delete_cmd = Command::new("svn");
            delete_cmd.arg("delete").args(&missing);
            match delete_cmd.output() {
                Ok(output) => {
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        error!("svn delete failed for {} file(s): {stderr}", missing.len());
                        return false;
                    }
                }
                Err(e) => {
                    error!("Failed to run svn delete: {e}");
                    return false;
                }
            }
        }

        let mut cmd = Command::new("svn");
        cmd.arg("commit").arg("-m").arg(&message);
        if !self.commit_username.is_empty() {
            cmd.arg("--username").arg(&self.commit_username);
        }
        if !self.commit_password.is_empty() {
            cmd.arg("--password").arg(&self.commit_password);
        }
        cmd.args(&files);

        info!("Running svn commit for {} file(s)", files.len());

        // print the full command with arguments (except password) for debugging purposes
        let debug_cmd = cmd
            .get_args()
            .map(|arg| {
                if arg == "--password" {
                    "--password <redacted>".into()
                } else {
                    arg.to_owned()
                }
            })
            .collect::<Vec<_>>();
        debug!("svn commit command: {:?}", debug_cmd);
        let mut commit_succeeded = false;
        match cmd.output() {
            Ok(output) => {
                if output.status.success() {
                    info!("svn commit succeeded");
                    commit_succeeded = true;
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("svn commit failed: {stderr}");
                    // Save draft so the message survives even if the app is closed.
                    self.save_commit_draft();
                }
            }
            Err(e) => {
                error!("Failed to run svn commit: {e}");
                self.save_commit_draft();
            }
        }

        if commit_succeeded {
            self.selected_files.clear();
            self.save_selection();
            self.commit_message.clear();
            self.commit_message_cursor = 0;
            self.commit_username.clear();
            self.commit_password.clear();
            self.commit_active_field = CommitField::Message;
            self.active_window = ActiveWindow::ChangedFiles;
            self.clear_commit_draft();
            self.refresh_status();
            true
        } else {
            // Leave the popup open so the user can fix the message and retry.
            false
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
        let limit = self.revision_log_limit.to_string();
        let output = Command::new("svn")
            .arg("log")
            .arg("-r")
            .arg("HEAD:1")
            .arg("--limit")
            .arg(&limit)
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

    fn increase_revision_log_limit(&mut self) {
        self.revision_log_limit = self
            .revision_log_limit
            .saturating_add(REVISION_LOAD_BATCH_SIZE);
    }

    pub fn load_more_revisions(&mut self) {
        self.increase_revision_log_limit();
        info!(
            "Loading more revisions (new limit: {})",
            self.revision_log_limit
        );
        self.refresh_log();
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
                    self.current_diff_file = None;
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
                let revision_info = format!(
                    "Revision {}: {} | {} | {}",
                    rev.revision, rev.author, rev.date, rev.message
                );
                //format the diff output with colored lines and insert the revision info at the top
                let revision_info = Line::from(Span::styled(
                    revision_info,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));

                let mut diff_lines = Self::style_diff_output(&output);
                diff_lines.insert(0, Line::from(revision_info));
                self.current_diff = diff_lines;

                self.diff_scroll = 0;
                self.current_diff_file = None;
            }
        }
        self.refresh_revision_files();
    }

    pub fn refresh_revision_files(&mut self) {
        self.revision_files.clear();
        let Some(i) = self.revision_list_state.selected() else {
            self.revision_file_list_state.select(None);
            return;
        };
        let Some(rev) = self.revision_list.get(i) else {
            self.revision_file_list_state.select(None);
            return;
        };

        let rev_num = Self::revision_number(&rev.revision);
        if !rev_num.chars().all(|c| c.is_ascii_digit()) {
            self.revision_file_list_state.select(None);
            return;
        }

        let mut cmd = Command::new("svn");
        cmd.arg("diff").arg("--summarize").arg("-c").arg(rev_num);
        if let Some(url) = &self.repository_url {
            cmd.arg(url);
        }
        let output = cmd
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        self.revision_files = output
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let status = parts.next()?.to_string();
                let path = parts.collect::<Vec<_>>().join(" ");
                if path.is_empty() {
                    None
                } else {
                    Some(SvnRevisionFile { status, path })
                }
            })
            .collect();

        if self.revision_files.is_empty() {
            self.revision_file_list_state.select(None);
            return;
        }

        let idx = self
            .revision_file_list_state
            .selected()
            .unwrap_or(0)
            .min(self.revision_files.len().saturating_sub(1));
        self.revision_file_list_state.select(Some(idx));
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

    pub fn next_revision_file(&mut self) {
        let len = self.revision_files.len();
        if len == 0 {
            return;
        }
        let i = match self.revision_file_list_state.selected() {
            Some(i) if i + 1 < len => i + 1,
            _ => 0,
        };
        self.revision_file_list_state.select(Some(i));
        self.refresh_revision_file_diff();
    }

    pub fn previous_revision_file(&mut self) {
        let len = self.revision_files.len();
        if len == 0 {
            return;
        }
        let i = match self.revision_file_list_state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.revision_file_list_state.select(Some(i));
        self.refresh_revision_file_diff();
    }

    pub fn refresh_revision_file_diff(&mut self) {
        let Some(rev_idx) = self.revision_list_state.selected() else {
            return;
        };
        let Some(file_idx) = self.revision_file_list_state.selected() else {
            return;
        };
        let Some(rev) = self.revision_list.get(rev_idx) else {
            return;
        };
        let Some(file) = self.revision_files.get(file_idx) else {
            return;
        };

        let rev_num = Self::revision_number(&rev.revision);
        if !rev_num.chars().all(|c| c.is_ascii_digit()) {
            return;
        }

        let output = Command::new("svn")
            .arg("diff")
            .arg("-c")
            .arg(rev_num)
            .arg(&file.path)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_else(|e| {
                error!("Failed to fetch revision file diff for {}: {e}", file.path);
                "Error fetching revision file diff".into()
            });

        let header = Line::from(Span::styled(
            format!("Revision {} file: {}", rev.revision, file.path),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        let mut diff_lines = Self::style_diff_output(&output);
        diff_lines.insert(0, header);
        self.current_diff = diff_lines;
        self.current_diff_file = None;
        self.diff_scroll = 0;
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
                self.current_diff_file = Some(path);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ── matches_ignore_pattern ──────────────────────────────────────────────

    #[test]
    fn test_ignore_exact_match() {
        assert!(matches_ignore_pattern("src/main.rs", "src/main.rs"));
        assert!(!matches_ignore_pattern("src/app.rs", "src/main.rs"));
    }

    #[test]
    fn test_ignore_basename_match() {
        // A pattern without a `/` matches as a trailing basename.
        assert!(matches_ignore_pattern("src/main.rs", "main.rs"));
        assert!(matches_ignore_pattern("main.rs", "main.rs"));
        assert!(!matches_ignore_pattern("src/app.rs", "main.rs"));
    }

    #[test]
    fn test_ignore_wildcard_extension() {
        // Pattern without `/` matches against the basename at any depth.
        assert!(matches_ignore_pattern("src/main.rs", "*.rs"));
        assert!(matches_ignore_pattern("main.rs", "*.rs"));
        assert!(!matches_ignore_pattern("src/main.rs", "*.txt"));
    }

    #[test]
    fn test_ignore_wildcard_no_cross_dir() {
        // Pattern with `/`: `*` must not cross directory boundaries.
        assert!(matches_ignore_pattern("src/main.rs", "src/*.rs"));
        assert!(!matches_ignore_pattern("src/sub/main.rs", "src/*.rs"));
    }

    #[test]
    fn test_ignore_double_star() {
        // `**` should match across directory boundaries.
        assert!(matches_ignore_pattern("src/sub/main.rs", "src/**/*.rs"));
        assert!(matches_ignore_pattern("a/b/c/d.txt", "**/*.txt"));
    }

    #[test]
    fn test_ignore_empty_pattern_never_matches() {
        assert!(!matches_ignore_pattern("anything", ""));
    }

    #[test]
    fn test_increase_revision_log_limit_by_batch_size() {
        let mut app = App::test_new();
        assert_eq!(app.revision_log_limit, REVISION_LOAD_BATCH_SIZE);
        app.increase_revision_log_limit();
        assert_eq!(app.revision_log_limit, REVISION_LOAD_BATCH_SIZE * 2);
    }

    // ── load_ignore_patterns / confirm_ignore ──────────────────────────────

    #[test]
    fn test_load_ignore_patterns_filters_comments_and_blanks() {
        let dir = tempfile::tempdir().unwrap();
        let ignore_path = dir.path().join("ignore");
        let mut f = std::fs::File::create(&ignore_path).unwrap();
        writeln!(f, "# this is a comment").unwrap();
        writeln!(f, "").unwrap();
        writeln!(f, "*.log").unwrap();
        writeln!(f, "build/").unwrap();
        drop(f);

        let contents = std::fs::read_to_string(&ignore_path).unwrap();
        let patterns: Vec<String> = contents
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        assert_eq!(patterns, vec!["*.log", "build/"]);
    }

    #[test]
    fn test_confirm_ignore_writes_to_file_and_filters_file_list() {
        let dir = tempfile::tempdir().unwrap();
        let ignore_path = dir.path().join("ignore");
        std::fs::write(&ignore_path, "").unwrap();

        // Simulate what confirm_ignore does: append pattern + reload + filter.
        let target = "src/debug.log".to_string();

        // Append.
        let existing = std::fs::read_to_string(&ignore_path).unwrap_or_default();
        let new_line = if existing.ends_with('\n') || existing.is_empty() {
            format!("{}\n", target)
        } else {
            format!("\n{}\n", target)
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&ignore_path)
            .unwrap();
        file.write_all(new_line.as_bytes()).unwrap();
        drop(file);

        // Reload.
        let contents = std::fs::read_to_string(&ignore_path).unwrap();
        let patterns: Vec<String> = contents
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();

        // Check pattern was stored.
        assert!(patterns.contains(&target));

        // Simulate filtering.
        let files = vec![
            SvnFile { status: "M".into(), path: "src/main.rs".into() },
            SvnFile { status: "?".into(), path: "src/debug.log".into() },
        ];
        let filtered: Vec<_> = files
            .iter()
            .filter(|f| !patterns.iter().any(|p| matches_ignore_pattern(&f.path, p)))
            .collect();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, "src/main.rs");
    }

    // ── commit message cursor movement ─────────────────────────────────────

    fn make_commit_app(msg: &str, cursor: usize) -> App {
        let mut app = App::test_new();
        app.commit_message = msg.to_string();
        app.commit_message_cursor = cursor;
        app
    }

    #[test]
    fn test_cursor_insert_char_at_start() {
        let mut app = make_commit_app("bc", 0);
        app.commit_message_insert_char('a');
        assert_eq!(app.commit_message, "abc");
        assert_eq!(app.commit_message_cursor, 1);
    }

    #[test]
    fn test_cursor_insert_char_at_end() {
        let mut app = make_commit_app("ab", 2);
        app.commit_message_insert_char('c');
        assert_eq!(app.commit_message, "abc");
        assert_eq!(app.commit_message_cursor, 3);
    }

    #[test]
    fn test_cursor_insert_char_in_middle() {
        let mut app = make_commit_app("ac", 1);
        app.commit_message_insert_char('b');
        assert_eq!(app.commit_message, "abc");
        assert_eq!(app.commit_message_cursor, 2);
    }

    #[test]
    fn test_cursor_delete_before_cursor_basic() {
        let mut app = make_commit_app("abc", 3);
        app.commit_message_delete_before_cursor();
        assert_eq!(app.commit_message, "ab");
        assert_eq!(app.commit_message_cursor, 2);
    }

    #[test]
    fn test_cursor_delete_before_cursor_at_start_noop() {
        let mut app = make_commit_app("abc", 0);
        app.commit_message_delete_before_cursor();
        assert_eq!(app.commit_message, "abc");
        assert_eq!(app.commit_message_cursor, 0);
    }

    #[test]
    fn test_cursor_delete_before_cursor_middle() {
        let mut app = make_commit_app("abc", 2);
        app.commit_message_delete_before_cursor();
        assert_eq!(app.commit_message, "ac");
        assert_eq!(app.commit_message_cursor, 1);
    }

    #[test]
    fn test_cursor_move_left() {
        let mut app = make_commit_app("hello", 5);
        app.commit_message_move_left();
        assert_eq!(app.commit_message_cursor, 4);
        app.commit_message_move_left();
        assert_eq!(app.commit_message_cursor, 3);
    }

    #[test]
    fn test_cursor_move_left_at_start_noop() {
        let mut app = make_commit_app("hello", 0);
        app.commit_message_move_left();
        assert_eq!(app.commit_message_cursor, 0);
    }

    #[test]
    fn test_cursor_move_right() {
        let mut app = make_commit_app("hello", 0);
        app.commit_message_move_right();
        assert_eq!(app.commit_message_cursor, 1);
    }

    #[test]
    fn test_cursor_move_right_at_end_noop() {
        let mut app = make_commit_app("hi", 2);
        app.commit_message_move_right();
        assert_eq!(app.commit_message_cursor, 2);
    }

    #[test]
    fn test_cursor_line_col_single_line() {
        let app = make_commit_app("hello", 3);
        assert_eq!(app.commit_cursor_line_col(), (0, 3));
    }

    #[test]
    fn test_cursor_line_col_multiline() {
        // "line1\nline2" – cursor at byte 7, which is 'i' (the second char of "line2")
        let msg = "line1\nline2";
        let app = make_commit_app(msg, 7); // byte 7 = 'i' in "line2", one char after 'l'
        assert_eq!(app.commit_cursor_line_col(), (1, 1));
    }

    #[test]
    fn test_cursor_move_up() {
        // "abc\nde" – cursor at end of second line (byte 6)
        let msg = "abc\nde";
        let mut app = make_commit_app(msg, 6);
        assert_eq!(app.commit_cursor_line_col(), (1, 2));
        app.commit_message_move_up();
        // Should land at col 2 of "abc" → byte 2
        assert_eq!(app.commit_message_cursor, 2);
        assert_eq!(app.commit_cursor_line_col(), (0, 2));
    }

    #[test]
    fn test_cursor_move_up_clips_to_shorter_line() {
        // "ab\nlong line" – cursor at end of second line
        let msg = "ab\nlong line";
        let mut app = make_commit_app(msg, msg.len());
        app.commit_message_move_up();
        // "ab" is only 2 chars, so cursor clips to end of first line
        assert_eq!(app.commit_cursor_line_col(), (0, 2));
    }

    #[test]
    fn test_cursor_move_down() {
        // "abc\nde" – cursor at byte 1 (col 1 of first line)
        let msg = "abc\nde";
        let mut app = make_commit_app(msg, 1);
        app.commit_message_move_down();
        // col 1 of "de" → byte 5
        assert_eq!(app.commit_message_cursor, 5);
        assert_eq!(app.commit_cursor_line_col(), (1, 1));
    }

    #[test]
    fn test_cursor_move_to_line_start() {
        let msg = "abc\ndef";
        let mut app = make_commit_app(msg, 7); // end of "def"
        app.commit_message_move_to_line_start();
        assert_eq!(app.commit_message_cursor, 4); // start of "def"
    }

    #[test]
    fn test_cursor_move_to_line_end() {
        let msg = "abc\ndef";
        let mut app = make_commit_app(msg, 4); // start of "def"
        app.commit_message_move_to_line_end();
        assert_eq!(app.commit_message_cursor, 7); // end of "def"
    }

    // ── draft persistence ─────────────────────────────────────────────────

    #[test]
    fn test_save_and_load_commit_draft() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = tmp.path().join("commit_draft.txt");

        let mut app = App::test_new();
        app.commit_message = "my draft message".to_string();
        app.commit_message_cursor = app.commit_message.len();
        app.save_commit_draft_to(&draft);

        // A second app instance should load the draft.
        let mut app2 = App::test_new();
        assert!(app2.commit_message.is_empty());
        app2.load_commit_draft_from(&draft);
        assert_eq!(app2.commit_message, "my draft message");
        assert_eq!(app2.commit_message_cursor, "my draft message".len());

        // clear_commit_draft should remove the file.
        app2.clear_commit_draft_at(&draft);
        let mut app3 = App::test_new();
        app3.load_commit_draft_from(&draft);
        assert!(app3.commit_message.is_empty());
    }

    #[test]
    fn test_save_empty_draft_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let draft = tmp.path().join("commit_draft.txt");

        let mut app = App::test_new();
        app.commit_message = "initial draft".to_string();
        app.save_commit_draft_to(&draft);
        assert!(draft.exists());

        // Now clear and save again – should remove the draft file.
        app.commit_message.clear();
        app.save_commit_draft_to(&draft);

        let mut app2 = App::test_new();
        app2.load_commit_draft_from(&draft);
        assert!(app2.commit_message.is_empty());
    }

    // ── open_help / close_help ───────────────────────────────────────────────

    #[test]
    fn test_open_help_saves_prev_window_and_switches_to_help() {
        let mut app = App::test_new();
        app.active_window = ActiveWindow::Branches;
        app.open_help();
        assert_eq!(app.active_window, ActiveWindow::Help);
        assert_eq!(app.prev_window, Some(ActiveWindow::Branches));
    }

    #[test]
    fn test_close_help_restores_prev_window() {
        let mut app = App::test_new();
        app.active_window = ActiveWindow::Revisions;
        app.open_help();
        app.close_help();
        assert_eq!(app.active_window, ActiveWindow::Revisions);
        assert_eq!(app.prev_window, None);
    }

    #[test]
    fn test_close_help_without_prev_window_defaults_to_changed_files() {
        let mut app = App::test_new();
        app.active_window = ActiveWindow::Help;
        app.prev_window = None;
        app.close_help();
        assert_eq!(app.active_window, ActiveWindow::ChangedFiles);
    }

    // ── build_tree_for_prefix / rebuild_visible_items ────────────────────────

    fn svn_file(status: &str, path: &str) -> SvnFile {
        SvnFile {
            status: status.to_string(),
            path: path.to_string(),
        }
    }

    #[test]
    fn test_build_tree_flat_files_no_dirs() {
        let files = vec![svn_file("A", "b.rs"), svn_file("M", "a.rs")];
        let collapsed = HashSet::new();
        let mut result = Vec::new();
        App::build_tree_for_prefix("", 0, &files, &collapsed, &mut result);
        // No subdirectories; both entries are files, sorted by path.
        assert_eq!(result.len(), 2);
        assert!(matches!(result[0], FileTreeNode::File { .. }));
        assert!(matches!(result[1], FileTreeNode::File { .. }));
        if let FileTreeNode::File { path, .. } = &result[0] {
            assert_eq!(path, "a.rs");
        }
        if let FileTreeNode::File { path, .. } = &result[1] {
            assert_eq!(path, "b.rs");
        }
    }

    #[test]
    fn test_build_tree_dirs_emitted_before_files_at_same_level() {
        let files = vec![svn_file("M", "src/main.rs"), svn_file("A", "root.rs")];
        let collapsed = HashSet::new();
        let mut result = Vec::new();
        App::build_tree_for_prefix("", 0, &files, &collapsed, &mut result);
        // src/ dir first, then src/main.rs (child), then root.rs at top level.
        assert_eq!(result.len(), 3);
        assert!(matches!(result[0], FileTreeNode::Dir { .. }));
        assert!(matches!(result[1], FileTreeNode::File { .. }));
        assert!(matches!(result[2], FileTreeNode::File { .. }));
        if let FileTreeNode::File { path, .. } = &result[2] {
            assert_eq!(path, "root.rs");
        }
    }

    #[test]
    fn test_build_tree_collapsed_dir_hides_children() {
        let files = vec![svn_file("M", "src/main.rs"), svn_file("A", "src/lib.rs")];
        let mut collapsed = HashSet::new();
        collapsed.insert("src/".to_string());
        let mut result = Vec::new();
        App::build_tree_for_prefix("", 0, &files, &collapsed, &mut result);
        // Only the dir row is visible; its children are suppressed.
        assert_eq!(result.len(), 1);
        if let FileTreeNode::Dir { collapsed, path, .. } = &result[0] {
            assert!(collapsed, "dir should be marked collapsed");
            assert_eq!(path, "src/");
        } else {
            panic!("expected Dir node");
        }
    }

    #[test]
    fn test_build_tree_nested_dirs_have_correct_depth() {
        let files = vec![svn_file("M", "a/b/c.rs")];
        let collapsed = HashSet::new();
        let mut result = Vec::new();
        App::build_tree_for_prefix("", 0, &files, &collapsed, &mut result);
        // a/ (depth 0) → a/b/ (depth 1) → a/b/c.rs (depth 2)
        assert_eq!(result.len(), 3);
        if let FileTreeNode::Dir { depth, path, .. } = &result[0] {
            assert_eq!(*depth, 0);
            assert_eq!(path, "a/");
        } else {
            panic!("expected Dir");
        }
        if let FileTreeNode::Dir { depth, path, .. } = &result[1] {
            assert_eq!(*depth, 1);
            assert_eq!(path, "a/b/");
        } else {
            panic!("expected Dir");
        }
        if let FileTreeNode::File { depth, path, .. } = &result[2] {
            assert_eq!(*depth, 2);
            assert_eq!(path, "a/b/c.rs");
        } else {
            panic!("expected File");
        }
    }

    // ── toggle_folder ────────────────────────────────────────────────────────

    #[test]
    fn test_toggle_folder_collapses_then_expands() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "src/main.rs"), svn_file("A", "src/lib.rs")];
        app.rebuild_visible_items();
        // Initially expanded: dir row + 2 file rows.
        assert_eq!(app.visible_items.len(), 3);
        // Select the directory row (index 0).
        app.file_list_state.select(Some(0));

        app.toggle_folder();
        // After collapse only the directory row remains visible.
        assert_eq!(app.visible_items.len(), 1);

        app.toggle_folder();
        // After re-expand all three rows are visible again.
        assert_eq!(app.visible_items.len(), 3);
    }

    #[test]
    fn test_toggle_folder_on_file_row_does_nothing() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "main.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));
        // index 0 is a File, not a Dir.
        app.toggle_folder();
        assert_eq!(app.visible_items.len(), 1);
    }

    // ── toggle_file_selection ────────────────────────────────────────────────

    #[test]
    fn test_toggle_file_selection_single_file_toggles() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "main.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));

        app.toggle_file_selection();
        assert!(app.selected_files.contains("main.rs"));

        app.toggle_file_selection();
        assert!(!app.selected_files.contains("main.rs"));
    }

    #[test]
    fn test_toggle_file_selection_directory_selects_all_children() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "src/a.rs"), svn_file("A", "src/b.rs")];
        app.rebuild_visible_items();
        // Index 0 is the `src/` directory row.
        app.file_list_state.select(Some(0));

        app.toggle_file_selection();
        assert!(app.selected_files.contains("src/a.rs"));
        assert!(app.selected_files.contains("src/b.rs"));

        // Second toggle deselects all children.
        app.toggle_file_selection();
        assert!(!app.selected_files.contains("src/a.rs"));
        assert!(!app.selected_files.contains("src/b.rs"));
    }

    // ── svn_delete_marked ────────────────────────────────────────────────────

    #[test]
    fn test_svn_delete_marked_no_selection_uses_hovered_file() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "foo.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));

        app.svn_delete_marked();
        assert_eq!(app.active_window, ActiveWindow::ConfirmDelete);
        assert_eq!(app.delete_targets, vec!["foo.rs".to_string()]);
    }

    #[test]
    fn test_svn_delete_marked_with_selected_files() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "a.rs"), svn_file("M", "b.rs")];
        app.rebuild_visible_items();
        app.selected_files.insert("a.rs".to_string());
        app.selected_files.insert("b.rs".to_string());

        app.svn_delete_marked();
        assert_eq!(app.active_window, ActiveWindow::ConfirmDelete);
        let mut targets = app.delete_targets.clone();
        targets.sort();
        assert_eq!(targets, vec!["a.rs".to_string(), "b.rs".to_string()]);
    }

    #[test]
    fn test_svn_delete_marked_no_items_does_nothing() {
        let mut app = App::test_new();
        // No file_list, no selected_files, no selection.
        app.svn_delete_marked();
        assert_eq!(app.active_window, ActiveWindow::ChangedFiles);
        assert!(app.delete_targets.is_empty());
    }

    // ── svn_revert_marked ────────────────────────────────────────────────────

    #[test]
    fn test_svn_revert_marked_no_selection_uses_hovered_file() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "foo.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));

        app.svn_revert_marked();
        assert_eq!(app.active_window, ActiveWindow::ConfirmRevert);
        assert_eq!(app.revert_targets, vec!["foo.rs".to_string()]);
    }

    #[test]
    fn test_svn_revert_marked_with_selected_files() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "a.rs"), svn_file("M", "b.rs")];
        app.rebuild_visible_items();
        app.selected_files.insert("a.rs".to_string());
        app.selected_files.insert("b.rs".to_string());

        app.svn_revert_marked();
        assert_eq!(app.active_window, ActiveWindow::ConfirmRevert);
        let mut targets = app.revert_targets.clone();
        targets.sort();
        assert_eq!(targets, vec!["a.rs".to_string(), "b.rs".to_string()]);
    }

    #[test]
    fn test_svn_revert_marked_no_items_does_nothing() {
        let mut app = App::test_new();
        app.svn_revert_marked();
        assert_eq!(app.active_window, ActiveWindow::ChangedFiles);
        assert!(app.revert_targets.is_empty());
    }

    // ── ignore_current_file ──────────────────────────────────────────────────

    #[test]
    fn test_ignore_current_file_sets_target_and_opens_confirm() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("?", "debug.log")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));

        app.ignore_current_file();
        assert_eq!(app.active_window, ActiveWindow::ConfirmIgnore);
        assert_eq!(app.ignore_target, Some("debug.log".to_string()));
    }

    #[test]
    fn test_ignore_current_file_on_dir_does_nothing() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("?", "src/debug.log")];
        app.rebuild_visible_items();
        // Index 0 is the `src/` directory row.
        app.file_list_state.select(Some(0));

        app.ignore_current_file();
        assert_eq!(app.active_window, ActiveWindow::ChangedFiles);
        assert!(app.ignore_target.is_none());
    }

    #[test]
    fn test_ignore_current_file_no_selection_does_nothing() {
        let mut app = App::test_new();
        app.ignore_current_file();
        assert_eq!(app.active_window, ActiveWindow::ChangedFiles);
        assert!(app.ignore_target.is_none());
    }

    // ── next_branch / previous_branch ────────────────────────────────────────

    #[test]
    fn test_next_branch_advances_selection() {
        let mut app = App::test_new();
        app.branch_list = vec!["main".into(), "dev".into(), "feature".into()];
        app.branch_list_state.select(Some(0));

        app.next_branch();
        assert_eq!(app.branch_list_state.selected(), Some(1));
        app.next_branch();
        assert_eq!(app.branch_list_state.selected(), Some(2));
    }

    #[test]
    fn test_next_branch_wraps_around() {
        let mut app = App::test_new();
        app.branch_list = vec!["a".into(), "b".into()];
        app.branch_list_state.select(Some(1));

        app.next_branch();
        assert_eq!(app.branch_list_state.selected(), Some(0));
    }

    #[test]
    fn test_previous_branch_retreats_selection() {
        let mut app = App::test_new();
        app.branch_list = vec!["main".into(), "dev".into()];
        app.branch_list_state.select(Some(1));

        app.previous_branch();
        assert_eq!(app.branch_list_state.selected(), Some(0));
    }

    #[test]
    fn test_previous_branch_wraps_around() {
        let mut app = App::test_new();
        app.branch_list = vec!["a".into(), "b".into(), "c".into()];
        app.branch_list_state.select(Some(0));

        app.previous_branch();
        assert_eq!(app.branch_list_state.selected(), Some(2));
    }

    // ── next_revision / previous_revision ────────────────────────────────────

    fn make_revision(rev: &str) -> SvnRevision {
        SvnRevision {
            revision: rev.to_string(),
            author: "author".into(),
            date: "2024-01-01".into(),
            message: "msg".into(),
        }
    }

    #[test]
    fn test_next_revision_advances_selection() {
        let mut app = App::test_new();
        app.revision_list = vec![
            make_revision("r1"),
            make_revision("r2"),
            make_revision("r3"),
        ];
        app.revision_list_state.select(Some(0));

        app.next_revision();
        assert_eq!(app.revision_list_state.selected(), Some(1));
    }

    #[test]
    fn test_next_revision_wraps_around() {
        let mut app = App::test_new();
        app.revision_list = vec![make_revision("r1"), make_revision("r2")];
        app.revision_list_state.select(Some(1));

        app.next_revision();
        assert_eq!(app.revision_list_state.selected(), Some(0));
    }

    #[test]
    fn test_previous_revision_retreats_selection() {
        let mut app = App::test_new();
        app.revision_list = vec![make_revision("r1"), make_revision("r2")];
        app.revision_list_state.select(Some(1));

        app.previous_revision();
        assert_eq!(app.revision_list_state.selected(), Some(0));
    }

    #[test]
    fn test_previous_revision_wraps_around() {
        let mut app = App::test_new();
        app.revision_list = vec![
            make_revision("r1"),
            make_revision("r2"),
            make_revision("r3"),
        ];
        app.revision_list_state.select(Some(0));

        app.previous_revision();
        assert_eq!(app.revision_list_state.selected(), Some(2));
    }

    #[test]
    fn test_revision_navigation_empty_list_does_not_panic() {
        let mut app = App::test_new();
        app.next_revision();
        app.previous_revision();
        assert_eq!(app.revision_list_state.selected(), None);
    }

    // ── next_file / previous_file ─────────────────────────────────────────────

    #[test]
    fn test_next_file_advances_selection() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "a.rs"), svn_file("A", "b.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));

        app.next_file();
        assert_eq!(app.file_list_state.selected(), Some(1));
    }

    #[test]
    fn test_next_file_wraps_around() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "a.rs"), svn_file("M", "b.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(1));

        app.next_file();
        assert_eq!(app.file_list_state.selected(), Some(0));
    }

    #[test]
    fn test_previous_file_wraps_around() {
        let mut app = App::test_new();
        app.file_list = vec![svn_file("M", "a.rs"), svn_file("M", "b.rs")];
        app.rebuild_visible_items();
        app.file_list_state.select(Some(0));

        app.previous_file();
        assert_eq!(app.file_list_state.selected(), Some(1));
    }

    // ── scroll_diff_down / scroll_diff_up ─────────────────────────────────────

    #[test]
    fn test_scroll_diff_down_increments() {
        let mut app = App::test_new();
        app.current_diff = vec![
            Line::from("a"),
            Line::from("b"),
            Line::from("c"),
        ];
        app.diff_scroll = 0;

        app.scroll_diff_down();
        assert_eq!(app.diff_scroll, 1);
        app.scroll_diff_down();
        assert_eq!(app.diff_scroll, 2);
    }

    #[test]
    fn test_scroll_diff_down_caps_at_max() {
        let mut app = App::test_new();
        // Two lines → max scroll = len - 1 = 1.
        app.current_diff = vec![Line::from("a"), Line::from("b")];
        app.diff_scroll = 1;

        app.scroll_diff_down();
        assert_eq!(app.diff_scroll, 1);
    }

    #[test]
    fn test_scroll_diff_up_decrements() {
        let mut app = App::test_new();
        app.diff_scroll = 3;

        app.scroll_diff_up();
        assert_eq!(app.diff_scroll, 2);
    }

    #[test]
    fn test_scroll_diff_up_clamps_at_zero() {
        let mut app = App::test_new();
        app.diff_scroll = 0;

        app.scroll_diff_up();
        assert_eq!(app.diff_scroll, 0);
    }

    // ── scroll_diff_next_hunk / scroll_diff_prev_hunk ─────────────────────────

    fn diff_with_hunks() -> Vec<Line<'static>> {
        vec![
            Line::from("context line"),
            Line::from(Span::styled(
                "@@ -1,3 +1,3 @@",
                Style::default().fg(Color::Cyan),
            )),
            Line::from("+added"),
            Line::from(Span::styled(
                "@@ -10,3 +10,3 @@",
                Style::default().fg(Color::Cyan),
            )),
            Line::from("-removed"),
        ]
    }

    #[test]
    fn test_scroll_diff_next_hunk_jumps_forward() {
        let mut app = App::test_new();
        app.current_diff = diff_with_hunks();
        app.diff_scroll = 0;

        app.scroll_diff_next_hunk();
        assert_eq!(app.diff_scroll, 1); // first `@@` is at index 1

        app.scroll_diff_next_hunk();
        assert_eq!(app.diff_scroll, 3); // second `@@` is at index 3
    }

    #[test]
    fn test_scroll_diff_prev_hunk_jumps_backward() {
        let mut app = App::test_new();
        app.current_diff = diff_with_hunks();
        app.diff_scroll = 4;

        app.scroll_diff_prev_hunk();
        assert_eq!(app.diff_scroll, 3); // second `@@` at index 3

        app.scroll_diff_prev_hunk();
        assert_eq!(app.diff_scroll, 1); // first `@@` at index 1
    }

    // ── style_diff_output ─────────────────────────────────────────────────────

    #[test]
    fn test_style_diff_output_added_lines_green_triple_plus_unchanged() {
        let output = "+added line\n+++not a change\ncontext";
        let lines = App::style_diff_output(output);
        // Line starting with `+` (not `+++`) → green foreground.
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Green)));
        // Line starting with `+++` → no green foreground.
        assert!(lines[1]
            .spans
            .iter()
            .all(|s| s.style.fg != Some(Color::Green)));
        // Plain context line → no foreground colour.
        assert!(lines[2].spans.iter().all(|s| s.style.fg.is_none()));
    }

    #[test]
    fn test_style_diff_output_removed_lines_red_triple_minus_unchanged() {
        let output = "-removed\n---not a removal\ncontext";
        let lines = App::style_diff_output(output);
        // `-` lines are red.
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Red)));
        // `---` lines have no red foreground.
        assert!(lines[1]
            .spans
            .iter()
            .all(|s| s.style.fg != Some(Color::Red)));
    }

    #[test]
    fn test_style_diff_output_hunk_headers_cyan() {
        let output = "@@ -1,3 +1,3 @@\ncontext";
        let lines = App::style_diff_output(output);
        // `@@` lines are cyan.
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Cyan)));
        // Context lines have no foreground colour.
        assert!(lines[1].spans.iter().all(|s| s.style.fg.is_none()));
    }

    // ── revision_number ───────────────────────────────────────────────────────

    #[test]
    fn test_revision_number_strips_leading_r() {
        assert_eq!(App::revision_number("r42"), "42");
        assert_eq!(App::revision_number("r1234"), "1234");
    }

    #[test]
    fn test_revision_number_no_leading_r_unchanged() {
        assert_eq!(App::revision_number("42"), "42");
        assert_eq!(App::revision_number("HEAD"), "HEAD");
    }

    // ── do_commit early exits ─────────────────────────────────────────────────

    #[test]
    fn test_do_commit_empty_message_returns_false() {
        let mut app = App::test_new();
        app.commit_message = "   ".to_string(); // only whitespace
        app.file_list = vec![svn_file("M", "foo.rs")];
        assert!(!app.do_commit());
    }

    #[test]
    fn test_do_commit_no_files_returns_false() {
        let mut app = App::test_new();
        app.commit_message = "fix: something".to_string();
        // file_list and selected_files are both empty.
        assert!(!app.do_commit());
    }

    // ── glob_match edge cases ─────────────────────────────────────────────────

    #[test]
    fn test_ignore_double_star_alone_matches_any_basename() {
        // `**` without a `/` is treated as basename-only matching.
        assert!(matches_ignore_pattern("a/b/c.rs", "**"));
        assert!(matches_ignore_pattern("file.rs", "**"));
    }

    #[test]
    fn test_ignore_double_star_prefix_matches_at_any_depth() {
        // `**/foo.txt` matches `something/foo.txt` and deeper paths.
        assert!(matches_ignore_pattern("a/foo.txt", "**/foo.txt"));
        assert!(matches_ignore_pattern("a/b/foo.txt", "**/foo.txt"));
        assert!(!matches_ignore_pattern("a/bar.txt", "**/foo.txt"));
    }

    #[test]
    fn test_ignore_basename_only_does_not_match_mid_component() {
        // Pattern `bar.txt` must match at a directory boundary, not mid-component.
        assert!(matches_ignore_pattern("foo/bar.txt", "bar.txt"));
        assert!(!matches_ignore_pattern("foobar.txt", "bar.txt"));
    }

    // ── commit message Unicode and edge cases ─────────────────────────────────

    #[test]
    fn test_cursor_insert_multibyte_char() {
        // 'ñ' is 2 bytes in UTF-8.
        let mut app = make_commit_app("bc", 0);
        app.commit_message_insert_char('ñ');
        assert_eq!(app.commit_message, "ñbc");
        assert_eq!(app.commit_message_cursor, 'ñ'.len_utf8());
    }

    #[test]
    fn test_cursor_delete_multibyte_char() {
        let cursor = 'ñ'.len_utf8(); // 2 bytes
        let mut app = make_commit_app("ñbc", cursor);
        app.commit_message_delete_before_cursor();
        assert_eq!(app.commit_message, "bc");
        assert_eq!(app.commit_message_cursor, 0);
    }

    #[test]
    fn test_cursor_move_left_multibyte_char() {
        let cursor = 'ñ'.len_utf8();
        let mut app = make_commit_app("ñ", cursor);
        app.commit_message_move_left();
        assert_eq!(app.commit_message_cursor, 0);
    }

    #[test]
    fn test_cursor_move_right_multibyte_char() {
        let mut app = make_commit_app("ñbc", 0);
        app.commit_message_move_right();
        assert_eq!(app.commit_message_cursor, 'ñ'.len_utf8());
    }

    #[test]
    fn test_cursor_line_col_empty_message() {
        let app = make_commit_app("", 0);
        assert_eq!(app.commit_cursor_line_col(), (0, 0));
    }

    #[test]
    fn test_cursor_move_down_on_last_line_goes_to_line_end() {
        let msg = "abc\ndef";
        // Cursor at 'd' (byte 5, inside the last line "def").
        let mut app = make_commit_app(msg, 5);
        app.commit_message_move_down();
        // Already on last line → cursor moves to end of "def".
        assert_eq!(app.commit_message_cursor, msg.len());
    }

    #[test]
    fn test_cursor_move_up_on_first_line_goes_to_line_start() {
        let mut app = make_commit_app("abc", 2);
        app.commit_message_move_up();
        // Already on first line → cursor moves to start of line.
        assert_eq!(app.commit_message_cursor, 0);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
