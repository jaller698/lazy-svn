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
