use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use rayon::prelude::*;
use walkdir::WalkDir;

use crate::risk;
use crate::types::{FolderStatus, FoundFolder, RiskLevel, ScanConfig, ScanProgress, TargetKind};

pub struct Scanner {
    pub config: ScanConfig,
    stop_flag: Arc<AtomicBool>,
    pub current_path: Option<Arc<Mutex<String>>>,
    pub progress: Option<Arc<Mutex<ScanProgress>>>,
}

impl Scanner {
    pub fn new(config: ScanConfig) -> Self {
        Self {
            config,
            stop_flag: Arc::new(AtomicBool::new(false)),
            current_path: None,
            progress: None,
        }
    }

    pub fn with_current_path(mut self, path: Arc<Mutex<String>>) -> Self {
        self.current_path = Some(path);
        self
    }

    pub fn with_progress(mut self, progress: Arc<Mutex<ScanProgress>>) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop_flag.clone()
    }

    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    pub fn scan(&self) -> Vec<FoundFolder> {
        if !self.config.root_path.exists() {
            return Vec::new();
        }

        // ── Telescope: count total dirs upfront for accurate progress ──
        self.telescope_dir_count();

        // ── Phase 1: walk the tree, find targets (skip their contents) ──
        let mut results = self.walk_for_targets();

        // ── Phase 2: compute sizes (+ ages) in parallel ──
        if !self.config.disable_size || !self.config.disable_age {
            self.compute_stats_parallel(&mut results);
        }

        results
    }

    /// Quick upfront count of all directories under root_path.
    fn telescope_dir_count(&self) {
        let mut count: u64 = 0;
        let mut dirs = vec![self.config.root_path.clone()];
        while let Some(dir) = dirs.pop() {
            count += 1;
            if let Ok(entries) = crate::readdir::read_dir_fast(&dir) {
                for entry in &entries {
                    if entry.is_dir {
                        let path = dir.join(&entry.name);
                        if self.is_allowed(&path) {
                            dirs.push(path);
                        }
                    }
                }
            }
        }
        if let Some(ref p) = self.progress {
            let mut prog = p.lock().unwrap();
            prog.total_dirs_estimate = count;
        }
    }

    /// Phase 1: walk directory tree, identify target dirs, skip walking into them.
    /// Uses manual `read_dir` traversal with optional parallelism at the top level.
    pub(crate) fn walk_for_targets(&self) -> Vec<FoundFolder> {
        let max_depth = self.config.max_depth;

        // Fast path: if max_depth is 0, only check root itself (never finds targets)
        if max_depth == Some(0) {
            return Vec::new();
        }

        // Read root entries using fast getdents64 syscall
        let entries = match crate::readdir::read_dir_fast(&self.config.root_path) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        // Count root dir as visited
        self.report_dir_visited(&self.config.root_path);

        // Scan root entries: collect targets and subdirs to walk
        let mut results = Vec::new();
        let mut subdirs: Vec<std::path::PathBuf> = Vec::new();

        for entry in &entries {
            if self.stop_flag.load(Ordering::SeqCst) {
                return results;
            }
            if !entry.is_dir {
                continue;
            }

            let path = self.config.root_path.join(&entry.name);
            if !self.is_allowed(&path) {
                continue;
            }

            if let Some(target) = self.is_target(&entry.name) {
                self.record_target(&path, target, &mut results);
            } else if max_depth.is_none_or(|md| 1 <= md) {
                subdirs.push(path);
            }
        }

        // Parallel walk each top-level subdirectory
        if !subdirs.is_empty() {
            use rayon::prelude::*;
            let sub_results: Vec<Vec<FoundFolder>> = subdirs
                .par_iter()
                .map(|path| self.walk_subtree(path, 1, max_depth))
                .collect();

            for mut r in sub_results {
                results.append(&mut r);
            }
        }

        results
    }

    /// Walk a single subtree sequentially from a given root + depth.
    fn walk_subtree(&self, root: &std::path::Path, depth: usize, max_depth: Option<usize>) -> Vec<FoundFolder> {
        let mut results = Vec::new();
        let mut stack = vec![(root.to_path_buf(), depth)];

        while let Some((dir, depth)) = stack.pop() {
            if self.stop_flag.load(Ordering::SeqCst) {
                break;
            }

            self.report_dir_visited(&dir);

            let entries = match crate::readdir::read_dir_fast(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in &entries {
                if self.stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                if !entry.is_dir {
                    continue;
                }

                let path = dir.join(&entry.name);
                if !self.is_allowed(&path) {
                    continue;
                }

                let entry_depth = depth + 1;

                if max_depth.is_some_and(|md| entry_depth > md) {
                    continue;
                }

                if let Some(target) = self.is_target(&entry.name) {
                    self.record_target(&path, target, &mut results);
                } else {
                    stack.push((path, entry_depth));
                }
            }
        }

        results
    }

    fn report_dir_visited(&self, dir: &std::path::Path) {
        if let Some(ref p) = self.progress {
            let mut prog = p.lock().unwrap();
            prog.dirs_visited += 1;
            prog.current_path = dir.display().to_string();
        }
        if let Some(ref cp) = self.current_path {
            let mut p = cp.lock().unwrap();
            *p = dir.display().to_string();
        }
    }

    fn record_target(&self, path: &std::path::Path, target: TargetKind, results: &mut Vec<FoundFolder>) {
        let (risk_level, risk_reason) = risk::analyze(path);
        if self.config.exclude_sensitive && risk_level == RiskLevel::Sensitive {
            return;
        }

        let folder = FoundFolder {
            path: path.to_path_buf(),
            target,
            size: None,
            last_modified: None,
            status: FolderStatus::Pending,
            risk: risk_level,
            risk_reason,
        };

        if let Some(ref prog) = self.progress {
            let mut p = prog.lock().unwrap();
            p.folders_found += 1;
            p.pending_folders.push(folder.clone());
        }

        results.push(folder);
    }

    /// Phase 2: compute sizes (+ ages) for all discovered targets in parallel.
    pub(crate) fn compute_stats_parallel(&self, folders: &mut Vec<FoundFolder>) {
        let progress = self.progress.clone();
        let disable_size = self.config.disable_size;
        let disable_age = self.config.disable_age;

        folders.par_iter_mut().for_each(|f| {
            if disable_size && disable_age {
                return;
            }
            let (size, age) = if disable_size {
                (None, Self::get_last_modified(&f.path))
            } else if disable_age {
                (Some(Self::get_dir_size(&f.path)), None)
            } else {
                Self::get_dir_stats(&f.path)
            };
            f.size = size;
            f.last_modified = age;

            if let Some(ref p) = progress {
                let mut prog = p.lock().unwrap();
                if let Some(sz) = size {
                    prog.total_size_reclaimable += sz;
                }
            }
        });
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

    /// Single-pass size + newest-mtime under `path` (avoids walking the tree twice).
    pub fn get_dir_stats(path: &Path) -> (Option<u64>, Option<i64>) {
        let mut total = 0u64;
        let mut newest: Option<i64> = None;
        for entry in WalkDir::new(path)
            .follow_links(false)
            .same_file_system(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                    if let Ok(modified) = meta.modified() {
                        if let Ok(dur) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                            let ts = dur.as_secs() as i64;
                            if newest.is_none_or(|n| ts > n) {
                                newest = Some(ts);
                            }
                        }
                    }
                }
            }
        }
        (Some(total), newest)
    }

    /// Recursively sum file sizes under `path` using walkdir (faster than manual
    /// recursive `fs::read_dir` — walkdir batches directory opens and avoids
    /// redundant stat calls). Only reads filesystem *metadata*, never file contents.
    pub fn get_dir_size(path: &Path) -> u64 {
        WalkDir::new(path)
            .follow_links(false)
            .same_file_system(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum()
    }

    /// Newest modification time under `path`.
    pub fn get_last_modified(path: &Path) -> Option<i64> {
        WalkDir::new(path)
            .follow_links(false)
            .same_file_system(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter_map(|m| m.modified().ok())
            .filter_map(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .max()
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
        let has_nm = results
            .iter()
            .any(|f| f.path.ends_with("node_modules") && f.target == TargetKind::NodeModules);
        let has_next = results
            .iter()
            .any(|f| f.path.ends_with(".next") && f.target == TargetKind::NextDotNext);
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
    fn test_max_depth_zero_finds_nothing_beyond_root() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("project/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("project/src")).unwrap();
        // depth 0 = only root, so nothing at depth 1
        let config = ScanConfig {
            root_path: dir.path().join("project"),
            targets: vec![TargetKind::NodeModules],
            max_depth: Some(0),
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert!(results.is_empty());
    }

    #[test]
    fn test_max_depth_1_finds_direct_targets() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("project/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("project/sub/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().join("project"),
            targets: vec![TargetKind::NodeModules],
            max_depth: Some(1),
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        // Should only find project/node_modules (depth 1), not project/sub/node_modules (depth 2)
        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("project/node_modules"));
    }

    #[test]
    fn test_max_depth_2_finds_nested_targets() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("a/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("a/b/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("a/b/c/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().join("a"),
            targets: vec![TargetKind::NodeModules],
            max_depth: Some(2),
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        // Should find a/node_modules (depth 1) and a/b/node_modules (depth 2),
        // but not a/b/c/node_modules (depth 3)
        assert_eq!(results.len(), 2);
        let paths: Vec<_> = results.iter().map(|f| f.path.display().to_string()).collect();
        assert!(paths.iter().any(|p| p.ends_with("a/node_modules")));
        assert!(paths.iter().any(|p| p.ends_with("a/b/node_modules")));
    }

    #[test]
    fn test_max_depth_none_finds_all() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("a/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("a/b/node_modules")).unwrap();
        fs::create_dir_all(dir.path().join("a/b/c/node_modules")).unwrap();
        let config = ScanConfig {
            root_path: dir.path().join("a"),
            targets: vec![TargetKind::NodeModules],
            max_depth: None,
            ..Default::default()
        };
        let scanner = Scanner::new(config);
        let results = scanner.scan();
        assert_eq!(results.len(), 3);
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
