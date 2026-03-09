#[derive(Debug, PartialEq, Clone)]
pub enum ActiveWindow {
    ChangedFiles,
    Branches,
    Revisions,
    Diff,
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
        description: "Switch panel",
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
];
