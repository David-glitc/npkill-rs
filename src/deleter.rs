use std::fs;
use std::io;
use std::path::Path;

use crate::types::FoundFolder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteResult {
    Success,
    DryRun,
    Failed,
}

pub fn delete_folder(folder: &FoundFolder, dry_run: bool) -> DeleteResult {
    if dry_run {
        return DeleteResult::DryRun;
    }

    match fs::remove_dir_all(&folder.path) {
        Ok(()) => DeleteResult::Success,
        Err(e) => {
            eprintln!("Failed to delete {}: {}", folder.path.display(), e);
            DeleteResult::Failed
        }
    }
}

#[allow(dead_code)]
pub fn batch_delete<'a>(
    folders: &'a [&'a FoundFolder],
    dry_run: bool,
) -> Vec<(&'a FoundFolder, DeleteResult)> {
    folders
        .iter()
        .map(|f| (*f, delete_folder(f, dry_run)))
        .collect()
}

pub fn get_total_size(folders: &[FoundFolder]) -> u64 {
    folders.iter().filter_map(|f| f.size).sum()
}

pub fn format_size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::BINARY)
}

#[allow(dead_code)]
pub fn check_path_writable(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or(path);
    fs::metadata(parent)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FolderStatus, RiskLevel, TargetKind};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_folder(path: &Path) {
        fs::create_dir_all(path.join("subdir")).unwrap();
        fs::write(path.join("file.txt"), "content").unwrap();
    }

    fn make_found_folder(path: PathBuf, size: u64) -> FoundFolder {
        FoundFolder {
            path,
            target: TargetKind::NodeModules,
            size: Some(size),
            last_modified: None,
            status: FolderStatus::Pending,
            risk: RiskLevel::Safe,
            risk_reason: None,
        }
    }

    #[test]
    fn test_delete_folder_success() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("node_modules");
        create_test_folder(&target);
        assert!(target.exists());

        let folder = make_found_folder(target.clone(), 100);
        let result = delete_folder(&folder, false);
        assert_eq!(result, DeleteResult::Success);
        assert!(!target.exists());
    }

    #[test]
    fn test_delete_folder_dry_run() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("node_modules");
        create_test_folder(&target);
        assert!(target.exists());

        let folder = make_found_folder(target.clone(), 100);
        let result = delete_folder(&folder, true);
        assert_eq!(result, DeleteResult::DryRun);
        assert!(target.exists());
    }

    #[test]
    fn test_delete_nonexistent() {
        let folder = make_found_folder(PathBuf::from("/nonexistent_path_xyz"), 100);
        let result = delete_folder(&folder, false);
        assert_eq!(result, DeleteResult::Failed);
    }

    #[test]
    fn test_batch_delete() {
        let dir = TempDir::new().unwrap();
        let target1 = dir.path().join("nm1");
        let target2 = dir.path().join("nm2");
        create_test_folder(&target1);
        create_test_folder(&target2);

        let folder1 = make_found_folder(target1, 100);
        let folder2 = make_found_folder(target2, 100);

        let refs = vec![&folder1, &folder2];
        let results = batch_delete(&refs, false);

        assert_eq!(results.len(), 2);
        for (_, result) in &results {
            assert_eq!(*result, DeleteResult::Success);
        }
        assert!(!dir.path().join("nm1").exists());
        assert!(!dir.path().join("nm2").exists());
    }

    #[test]
    fn test_get_total_size() {
        let f1 = make_found_folder(PathBuf::from("/a"), 100);
        let f2 = make_found_folder(PathBuf::from("/b"), 200);
        let f3 = make_found_folder(PathBuf::from("/c"), 50);

        let total = get_total_size(&[f1, f2, f3]);
        assert_eq!(total, 350);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert!(format_size(1024).contains("KiB"));
        assert!(format_size(1024 * 1024).contains("MiB"));
    }

    #[test]
    fn test_check_path_writable_existing() {
        let dir = TempDir::new().unwrap();
        let result = check_path_writable(dir.path());
        assert!(result.is_ok());
    }
}
