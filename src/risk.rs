use std::path::Path;

use crate::types::RiskLevel;

pub fn analyze(path: &Path) -> (RiskLevel, Option<String>) {
    let path_str = path.to_string_lossy().to_lowercase();

    if path_str.contains("/.config/")
        || path_str.contains("/.local/share/")
        || path_str.contains("/appdata/roaming/")
        || path_str.contains("/appdata/local/")
        || path_str.contains("/program files/")
        || path_str.contains("/program files (x86)/")
    {
        return (
            RiskLevel::Sensitive,
            Some("Located in a system application directory".to_string()),
        );
    }

    if let Some(home) = dirs_next::home_dir() {
        let home_str = home.to_string_lossy().to_lowercase();
        if path_str.starts_with(&home_str) {
            let rel = path_str[home_str.len()..].trim_start_matches('/');

            if rel.is_empty() {
                return (
                    RiskLevel::Sensitive,
                    Some("Home directory root".to_string()),
                );
            }

            let top_level = rel.split('/').next().unwrap_or("");

            if top_level.starts_with('.')
                && top_level != ".npm"
                && top_level != ".pnpm"
                && top_level != "."
                && top_level != ".."
            {
                return (
                    RiskLevel::Sensitive,
                    Some("Inside a hidden folder in home directory".to_string()),
                );
            }
        }
    }

    (RiskLevel::Safe, None)
}

#[allow(dead_code)]
pub fn is_safe_to_delete(path: &Path) -> bool {
    let (risk, _) = analyze(path);
    risk == RiskLevel::Safe
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_normal_node_modules_is_safe() {
        let p = PathBuf::from("/home/user/project/node_modules");
        let (risk, _) = analyze(&p);
        assert_eq!(risk, RiskLevel::Safe);
    }

    #[test]
    fn test_normal_next_dot_is_safe() {
        let p = PathBuf::from("/home/user/project/.next");
        let (risk, _) = analyze(&p);
        assert_eq!(risk, RiskLevel::Safe);
    }

    #[test]
    fn test_config_is_sensitive() {
        let p = PathBuf::from("/home/user/.config/some-app/node_modules");
        let (risk, reason) = analyze(&p);
        assert_eq!(risk, RiskLevel::Sensitive);
        assert!(reason.is_some());
    }

    #[test]
    fn test_dot_local_is_sensitive() {
        let p = PathBuf::from("/home/user/.local/share/node_modules");
        let (risk, _) = analyze(&p);
        assert_eq!(risk, RiskLevel::Sensitive);
    }

    #[test]
    fn test_appdata_is_sensitive() {
        let p = PathBuf::from("/c/Users/user/AppData/Roaming/node_modules");
        let (risk, _) = analyze(&p);
        assert_eq!(risk, RiskLevel::Sensitive);
    }

    #[test]
    fn test_dir_in_home_hidden_not_npm() {
        if let Some(home) = dirs_next::home_dir() {
            let p = home.join(".vscode/node_modules");
            let (risk, _) = analyze(&p);
            assert_eq!(risk, RiskLevel::Sensitive);
        }
    }

    #[test]
    fn test_npm_in_home_is_safe() {
        let p = PathBuf::from("/home/user/.npm/node_modules");
        let (risk, _) = analyze(&p);
        assert_eq!(risk, RiskLevel::Safe);
    }

    #[test]
    fn test_is_safe_to_delete_true() {
        let p = PathBuf::from("/home/user/project/node_modules");
        assert!(is_safe_to_delete(&p));
    }

    #[test]
    fn test_is_safe_to_delete_false() {
        let p = PathBuf::from("/home/user/.config/foo/node_modules");
        assert!(!is_safe_to_delete(&p));
    }
}
