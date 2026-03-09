/// Which field the cursor is in when the commit popup is open.
#[derive(Debug, PartialEq, Clone)]
pub enum CommitField {
    Message,
    Username,
    Password,
}

#[derive(Debug, PartialEq, Clone)]
pub enum ActiveWindow {
    ChangedFiles,
    Branches,
    Revisions,
    Diff,
    Commit,
    Help,
}

pub struct SvnFile {
    pub status: String,
    pub path: String,
}

pub struct SvnRevision {
    pub revision: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// A single entry in the visible file-tree list.
#[derive(Debug, Clone)]
pub enum FileTreeNode {
    /// A directory row that can be folded/unfolded.
    Dir {
        /// Full directory path including trailing slash (e.g. `"src/"`).
        path: String,
        /// Just the directory name (e.g. `"src"`).
        name: String,
        /// Nesting depth (0 = top level).
        depth: usize,
        /// Whether the directory's children are currently hidden.
        collapsed: bool,
    },
    /// A file row.
    File {
        /// SVN status character (e.g. `"M"`, `"A"`, `"D"`).
        status: String,
        /// Full file path (e.g. `"src/app.rs"`).
        path: String,
        /// Just the file name (e.g. `"app.rs"`).
        name: String,
        /// Nesting depth (0 = top level).
        depth: usize,
    },
}
pub struct Keybinding {
    pub key: &'static str,
    pub description: &'static str,
}

pub const KEYBINDINGS: &[Keybinding] = &[
    Keybinding {
        key: "?",
        description: "Show/hide this help window",
    },
    Keybinding {
        key: "q",
        description: "Quit",
    },
    Keybinding {
        key: "Tab",
        description: "Switch to next panel",
    },
    Keybinding {
        key: "1",
        description: "Switch to Files panel",
    },
    Keybinding {
        key: "2",
        description: "Switch to Branches panel",
    },
    Keybinding {
        key: "3",
        description: "Switch to Revisions panel",
    },
    Keybinding {
        key: "4",
        description: "Switch to Diff panel",
    },
    Keybinding {
        key: "j",
        description: "Move down / scroll diff down",
    },
    Keybinding {
        key: "k",
        description: "Move up / scroll diff up",
    },
    Keybinding {
        key: "}",
        description: "Jump to next hunk (Diff panel only)",
    },
    Keybinding {
        key: "{",
        description: "Jump to previous hunk (Diff panel only)",
    },
    Keybinding {
        key: "r",
        description: "Refresh all data",
    },
    Keybinding {
        key: "Enter",
        description: "Update to selected revision (Revisions panel only)",
    },
    Keybinding {
        key: "Space",
        description: "Select file / select all children of a folder (Files panel only)",
    },
    Keybinding {
        key: "a",
        description: "Add marked unversioned files (Files panel only)",
    },
    Keybinding {
        key: "d",
        description: "Delete marked files/folders via svn delete (Files panel only)",
    },
    Keybinding {
        key: "c",
        description: "Open commit popup (Files panel only)",
    },
];
