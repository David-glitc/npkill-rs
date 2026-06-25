use std::path::PathBuf;

use clap::Parser;

use crate::types::{ScanConfig, TargetKind};

#[derive(Parser, Debug)]
#[command(
    name = "npkill-rs",
    about = "Fast interactive tool to find and delete node_modules and .next directories",
    version,
    styles = clap::builder::styling::Styles::styled()
)]
pub struct CliArgs {
    #[arg(short = 'd', long, default_value = ".")]
    pub directory: PathBuf,

    #[arg(short, long)]
    pub targets: Option<String>,

    #[arg(short, long)]
    pub exclude: Vec<String>,

    #[arg(short = 'b', long)]
    pub blacklist: Vec<String>,

    #[arg(short = 'w', long)]
    pub whitelist: Vec<String>,

    #[arg(long)]
    pub dry_run: bool,

    #[arg(long)]
    pub disable_size: bool,

    #[arg(long)]
    pub disable_age: bool,

    #[arg(long)]
    pub exclude_sensitive: bool,

    #[arg(long)]
    pub delete_all: bool,

    #[arg(short = 'y', long)]
    pub yes: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(long)]
    pub table: bool,

    #[arg(short = 'm', long)]
    pub max_depth: Option<usize>,

    #[arg(long)]
    pub include_hidden: bool,
}

impl From<CliArgs> for ScanConfig {
    fn from(args: CliArgs) -> Self {
        let targets = if let Some(t) = &args.targets {
            t.split(',')
                .map(|s| match s.trim().to_lowercase().as_str() {
                    "node_modules" => TargetKind::NodeModules,
                    ".next" | "next" => TargetKind::NextDotNext,
                    _ => TargetKind::NodeModules,
                })
                .collect()
        } else {
            vec![TargetKind::NodeModules, TargetKind::NextDotNext]
        };

        let mut exclude = args.exclude;
        exclude.push(".git".to_string());
        // Skip common cache/config/system dirs that never have project node_modules
        for d in &[".cache", ".config", ".local", "Library", "AppData", "snap", "flatpak"] {
            exclude.push(d.to_string());
        }

        ScanConfig {
            root_path: args.directory,
            targets,
            exclude,
            blacklist: args.blacklist,
            whitelist: args.whitelist,
            dry_run: args.dry_run,
            disable_size: args.disable_size,
            disable_age: args.disable_age,
            exclude_sensitive: args.exclude_sensitive,
            delete_all: args.delete_all,
            yes: args.yes,
            max_depth: args.max_depth.or(Some(5)),
            include_hidden: args.include_hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_targets() {
        let args = CliArgs {
            directory: PathBuf::from("."),
            targets: None,
            exclude: vec![],
            blacklist: vec![],
            whitelist: vec![],
            dry_run: false,
            disable_size: false,
            disable_age: false,
            exclude_sensitive: false,
            delete_all: false,
            yes: false,
            json: false,
            table: false,
            max_depth: None,
            include_hidden: false,
        };
        let config: ScanConfig = args.into();
        assert_eq!(config.targets.len(), 2);
        assert!(config.targets.contains(&TargetKind::NodeModules));
        assert!(config.targets.contains(&TargetKind::NextDotNext));
    }

    #[test]
    fn test_blacklist_whitelist_roundtrip() {
        let args = CliArgs {
            directory: PathBuf::from("."),
            targets: None,
            exclude: vec![],
            blacklist: vec!["/tmp".to_string()],
            whitelist: vec!["/important".to_string()],
            dry_run: false,
            disable_size: false,
            disable_age: false,
            exclude_sensitive: false,
            delete_all: false,
            yes: false,
            json: false,
            table: false,
            max_depth: None,
            include_hidden: false,
        };
        let config: ScanConfig = args.into();
        assert_eq!(config.blacklist, vec!["/tmp"]);
        assert_eq!(config.whitelist, vec!["/important"]);
    }
}
