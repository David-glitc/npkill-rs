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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Scan,
    Settings,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Catppuccino,
    Nord,
    TokyoNight,
}

pub struct ColorPalette {
    pub bg: Color,
    pub fg: Color,
    pub accent: Color,
    pub highlight_bg: Color,
    pub highlight_fg: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub dim: Color,
    pub surface: Color,
}

use ratatui::style::Color;

impl Theme {
    pub fn palette(&self) -> ColorPalette {
        match self {
            Theme::Catppuccino => ColorPalette {
                bg: Color::Rgb(30, 30, 46),
                fg: Color::Rgb(205, 214, 244),
                accent: Color::Rgb(137, 180, 250),
                highlight_bg: Color::Rgb(137, 180, 250),
                highlight_fg: Color::Rgb(30, 30, 46),
                success: Color::Rgb(166, 227, 161),
                warning: Color::Rgb(249, 226, 175),
                error: Color::Rgb(243, 139, 168),
                dim: Color::Rgb(108, 112, 134),
                surface: Color::Rgb(49, 50, 68),
            },
            Theme::Nord => ColorPalette {
                bg: Color::Rgb(46, 52, 64),
                fg: Color::Rgb(216, 222, 233),
                accent: Color::Rgb(136, 192, 208),
                highlight_bg: Color::Rgb(136, 192, 208),
                highlight_fg: Color::Rgb(46, 52, 64),
                success: Color::Rgb(163, 190, 140),
                warning: Color::Rgb(235, 203, 139),
                error: Color::Rgb(191, 97, 106),
                dim: Color::Rgb(76, 86, 106),
                surface: Color::Rgb(59, 66, 82),
            },
            Theme::TokyoNight => ColorPalette {
                bg: Color::Rgb(26, 27, 38),
                fg: Color::Rgb(169, 177, 214),
                accent: Color::Rgb(122, 162, 247),
                highlight_bg: Color::Rgb(122, 162, 247),
                highlight_fg: Color::Rgb(26, 27, 38),
                success: Color::Rgb(158, 206, 106),
                warning: Color::Rgb(224, 175, 104),
                error: Color::Rgb(219, 75, 95),
                dim: Color::Rgb(86, 95, 137),
                surface: Color::Rgb(36, 39, 58),
            },
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Theme::Catppuccino => "Catppuccino Mocha",
            Theme::Nord => "Nord",
            Theme::TokyoNight => "Tokyo Night",
        }
    }

    pub fn all() -> &'static [Theme] {
        &[Theme::Catppuccino, Theme::Nord, Theme::TokyoNight]
    }
}
