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
