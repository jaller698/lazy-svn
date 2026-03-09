#[derive(PartialEq, Clone)]
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
