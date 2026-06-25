use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn create_fake_project(dir: &Path, name: &str, has_nm: bool, has_next: bool) {
    let project = dir.join(name);
    fs::create_dir_all(&project).unwrap();

    if has_nm {
        fs::create_dir_all(project.join("node_modules/some-dep")).unwrap();
        fs::write(
            project.join("node_modules/some-dep/index.js"),
            "module.exports = {};",
        )
        .unwrap();
    }

    if has_next {
        fs::create_dir_all(project.join(".next/server")).unwrap();
        fs::write(project.join(".next/server/index.js"), "// build output").unwrap();
    }

    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(project.join("src/index.js"), "console.log('hello');").unwrap();
}

#[test]
fn test_scanner_finds_both_targets_integration() {
    let dir = TempDir::new().unwrap();
    create_fake_project(dir.path(), "proj1", true, true);
    create_fake_project(dir.path(), "proj2", true, false);
    create_fake_project(dir.path(), "proj3", false, true);

    let config = npkill_rs::types::ScanConfig {
        root_path: dir.path().to_path_buf(),
        ..Default::default()
    };
    let scanner = npkill_rs::scanner::Scanner::new(config);
    let results = scanner.scan();

    assert_eq!(results.len(), 4);

    let nm_count = results
        .iter()
        .filter(|f| f.target == npkill_rs::types::TargetKind::NodeModules)
        .count();
    let next_count = results
        .iter()
        .filter(|f| f.target == npkill_rs::types::TargetKind::NextDotNext)
        .count();

    assert_eq!(nm_count, 2);
    assert_eq!(next_count, 2);
}

#[test]
fn test_scanner_skips_git_integration() {
    let dir = TempDir::new().unwrap();
    create_fake_project(dir.path(), "my-project", true, false);
    fs::create_dir_all(dir.path().join("my-project/.git/node_modules")).unwrap();
    fs::write(
        dir.path().join("my-project/.git/node_modules/foo.js"),
        "bar",
    )
    .unwrap();

    let config = npkill_rs::types::ScanConfig {
        root_path: dir.path().to_path_buf(),
        ..Default::default()
    };
    let scanner = npkill_rs::scanner::Scanner::new(config);
    let results = scanner.scan();

    assert_eq!(results.len(), 1);
    assert!(results[0].path.ends_with("my-project/node_modules"));
}

#[test]
fn test_delete_actually_removes_directory() {
    let dir = TempDir::new().unwrap();
    create_fake_project(dir.path(), "to-delete", true, true);

    let nm_path = dir.path().join("to-delete/node_modules");
    let next_path = dir.path().join("to-delete/.next");

    assert!(nm_path.exists());
    assert!(next_path.exists());

    let folder = npkill_rs::types::FoundFolder {
        path: nm_path.clone(),
        target: npkill_rs::types::TargetKind::NodeModules,
        size: Some(100),
        last_modified: None,
        status: npkill_rs::types::FolderStatus::Pending,
        risk: npkill_rs::types::RiskLevel::Safe,
        risk_reason: None,
    };

    let result = npkill_rs::deleter::delete_folder(&folder, false);
    assert_eq!(result, npkill_rs::deleter::DeleteResult::Success);
    assert!(!nm_path.exists());
    assert!(next_path.exists());
}

#[test]
fn test_delete_dry_run_does_not_remove() {
    let dir = TempDir::new().unwrap();
    create_fake_project(dir.path(), "dry-run-test", true, false);

    let nm_path = dir.path().join("dry-run-test/node_modules");
    assert!(nm_path.exists());

    let folder = npkill_rs::types::FoundFolder {
        path: nm_path.clone(),
        target: npkill_rs::types::TargetKind::NodeModules,
        size: Some(100),
        last_modified: None,
        status: npkill_rs::types::FolderStatus::Pending,
        risk: npkill_rs::types::RiskLevel::Safe,
        risk_reason: None,
    };

    let result = npkill_rs::deleter::delete_folder(&folder, true);
    assert_eq!(result, npkill_rs::deleter::DeleteResult::DryRun);
    assert!(nm_path.exists());
}

#[test]
fn test_risk_safe_for_normal_projects() {
    let p = Path::new("/home/user/work/my-project/node_modules");
    let (risk, _) = npkill_rs::risk::analyze(p);
    assert_eq!(risk, npkill_rs::types::RiskLevel::Safe);
}

#[test]
fn test_cli_binary_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_npkill-rs"))
        .arg("--help")
        .output()
        .expect("Failed to run binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("npkill-rs"));
    assert!(stdout.contains("--directory"));
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn test_cli_binary_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_npkill-rs"))
        .arg("--version")
        .output()
        .expect("Failed to run binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.2"));
}
