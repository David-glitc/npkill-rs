use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use walkdir::WalkDir;

use crate::risk;
use crate::types::{FoundFolder, FolderStatus, RiskLevel, ScanConfig, TargetKind};

pub struct Scanner {
    pub config: ScanConfig,
    stop_flag: Arc<AtomicBool>,
}

impl Scanner {
    pub fn new(config: ScanConfig) -> Self {
        Self {
            config,
            stop_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop_flag.clone()
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    pub fn scan(&self) -> Vec<FoundFolder> {
        let mut results = Vec::new();

        if !self.config.root_path.exists() {
            return results;
        }

        let walker = WalkDir::new(&self.config.root_path)
            .follow_links(false)
            .same_file_system(true)
            .max_open(256);

        for entry in walker.into_iter().filter_map(|e| e.ok()) {
            if self.stop_flag.load(Ordering::SeqCst) {
                break;
            }

            let path = entry.path();
            if !entry.file_type().is_dir() {
                continue;
            }

            if !self.is_allowed(path) {
                continue;
            }

            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if let Some(target) = self.is_target(dir_name) {
                let (risk_level, risk_reason) = risk::analyze(path);

                if self.config.exclude_sensitive && risk_level == RiskLevel::Sensitive {
                    continue;
                }

                let size = if self.config.disable_size {
                    None
                } else {
                    Some(Self::get_dir_size(path))
                };

                let last_modified = if self.config.disable_age {
                    None
                } else {
                    Self::get_last_modified(path)
                };

                results.push(FoundFolder {
                    path: path.to_path_buf(),
                    target,
                    size,
                    last_modified,
                    status: FolderStatus::Pending,
                    risk: risk_level,
                    risk_reason,
                });
            }
        }

        results
    }

    fn is_target(&self, dir_name: &str) -> Option<TargetKind> {
        for target in &self.config.targets {
            if dir_name == target.dir_name() {
                return Some(*target);
            }
        }
        None
    }

    fn is_allowed(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Whitelist overrides everything
        for wl in &self.config.whitelist {
            if path_str.contains(wl) {
                return true;
            }
        }

        // Blacklist check
        for bl in &self.config.blacklist {
            if path_str.contains(bl) {
                return false;
            }
        }

        // Exclude check
        for excl in &self.config.exclude {
            if path_str.contains(excl) {
                return false;
            }
        }

        true
    }

    pub fn get_dir_size(path: &Path) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_symlink() {
                    continue;
                }
                if path.is_dir() {
                    total += Self::get_dir_size(&path);
                } else if let Ok(meta) = fs::metadata(&path) {
                    total += meta.len();
                }
            }
        }
        total
    }

    pub fn get_last_modified(path: &Path) -> Option<i64> {
        let mut newest: Option<i64> = None;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(meta) = fs::metadata(&path) {
                    if let Ok(modified) = meta.modified() {
                        if let Ok(dur) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                            let ts = dur.as_secs() as i64;
                            if newest.map_or(true, |n| ts > n) {
                                newest = Some(ts);
                            }
                        }
                    }
                }
            }
        }
        newest
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn setup_test_scan() -> (TempDir, ScanConfig) {
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("my-project");
        fs::create_dir_all(project.join("node_modules/some-pkg")).unwrap();
        fs::create_dir_all(project.join(".next")).unwrap();
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("node_modules/some-pkg/index.js"), "hello").unwrap();
        let config = ScanConfig {
            root_path: project,
            targets: vec![TargetKind::NodeModules, TargetKind::NextDotNext],
            ..Default::default()
        };
        (dir, config)
    }

    #[test]
    fn test_scan_finds_node_modules_and_next() {
        let (_dir, config) = setup_test_scan();
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert_eq!(results.len(), 2);
        let has_nm = results.iter().any(|f| f.path.ends_with("node_modules") && f.target == TargetKind::NodeModules);
        let has_next = results.iter().any(|f| f.path.ends_with(".next") && f.target == TargetKind::NextDotNext);
        assert!(has_nm);
        assert!(has_next);
    }

    #[test]
    fn test_scan_respects_exclude() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("project/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("project/.git/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().join("project"),
            targets: vec![TargetKind::NodeModules, TargetKind::NextDotNext],
            exclude: vec![".git".to_string()],
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("project/node_modules"));
    }

    #[test]
    fn test_whitelist_overrides_exclude() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("project/.git/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().join("project"),
            targets: vec![TargetKind::NodeModules],
            exclude: vec![".git".to_string()],
            whitelist: vec![".git".to_string()],
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_blacklist_skips() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("project/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().join("project"),
            targets: vec![TargetKind::NodeModules],
            blacklist: vec!["project".to_string()],
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert!(results.is_empty());
    }

    #[test]
    fn test_scan_excludes_sensitive() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".config/app/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().to_path_buf(),
            targets: vec![TargetKind::NodeModules, TargetKind::NextDotNext],
            exclude_sensitive: true,
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert!(results.is_empty());
    }

    #[test]
    fn test_get_dir_size() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), "hello").unwrap();
        fs::write(dir.path().join("sub/b.txt"), "world").unwrap();
        let size = Scanner::get_dir_size(dir.path());
        assert_eq!(size, 10);
    }

    #[test]
    fn test_empty_scan_when_root_not_found() {
        let config = ScanConfig {
            root_path: PathBuf::from("/nonexistent/path/xyz789"),
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert!(results.is_empty());
    }

    #[test]
    fn test_stop_flag() {
        let dir = TempDir::new().unwrap();
        let config = ScanConfig {
            root_path: dir.path().to_path_buf(),
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        scanner.stop();
        assert!(scanner.stop_flag.load(Ordering::SeqCst));
    }
}
