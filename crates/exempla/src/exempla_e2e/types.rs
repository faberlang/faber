use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct E2eResult {
    pub path: PathBuf,
    pub passed: bool,
    pub reason: String,
}
