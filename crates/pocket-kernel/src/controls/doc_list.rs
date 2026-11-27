//! Document-list metadata belongs to its existing list HWND. Paths stay in the
//! guest VFS namespace; no host directory or file handle is captured here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DocList {
    pub directory: String,
    pub filters: Vec<Vec<String>>,
    pub filter_index: u32,
    pub paths: Vec<String>,
    pub updates_enabled: bool,
    pub descending: bool,
    pub recursive: bool,
}
