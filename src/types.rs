use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FoundFolder {
    pub path: PathBuf,
    pub target: TargetKind,
    pub size: Option<u64>,
    pub last_modified: Option<i64>,
    pub status: FolderStatus,
    pub risk: RiskLevel,
    pub risk_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    NodeModules,
    NextDotNext,
}

impl TargetKind {
    pub fn dir_name(&self) -> &'static str {
        match self {
            TargetKind::NodeModules => "node_modules",
            TargetKind::NextDotNext => ".next",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderStatus {
    Pending,
    Deleting,
    Deleted,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Safe,
    Sensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Size,
    Date,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

impl SortDirection {
    pub fn flip(&self) -> Self {
        match self {
            SortDirection::Asc => SortDirection::Desc,
            SortDirection::Desc => SortDirection::Asc,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub root_path: PathBuf,
    pub targets: Vec<TargetKind>,
    pub exclude: Vec<String>,
    pub blacklist: Vec<String>,
    pub whitelist: Vec<String>,
    pub dry_run: bool,
    pub disable_size: bool,
    pub disable_age: bool,
    pub exclude_sensitive: bool,
    pub delete_all: bool,
    pub yes: bool,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            root_path: PathBuf::from("."),
            targets: vec![TargetKind::NodeModules, TargetKind::NextDotNext],
            exclude: vec![".git".to_string()],
            blacklist: vec![],
            whitelist: vec![],
            dry_run: false,
            disable_size: false,
            disable_age: false,
            exclude_sensitive: false,
            delete_all: false,
            yes: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct AppStats {
    pub total_found: usize,
    pub total_deleted: usize,
    pub total_errors: usize,
    pub total_size_reclaimable: u64,
    pub total_size_freed: u64,
    pub scan_duration_secs: f64,
}

#[derive(Debug, Clone)]
pub struct DeleteAnimation {
    pub folder_index: usize,
    pub started_at: std::time::Instant,
    pub phase: DeletePhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeletePhase {
    Deleting,
    Fading,
    Done,
}
