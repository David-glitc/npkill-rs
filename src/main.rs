use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use clap::Parser;
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use npkill_rs::app::App;
use npkill_rs::config::CliArgs;
use npkill_rs::scanner::Scanner;
use npkill_rs::types::ScanProgress;

fn format_duration(secs: f64) -> String {
    if secs < 1.0 {
        format!("{:.0}ms", secs * 1000.0)
    } else if secs < 60.0 {
        format!("{:.1}s", secs)
    } else {
        let m = (secs as u64) / 60;
        let s = (secs as u64) % 60;
        format!("{}m {:02}s", m, s)
    }
}

fn print_table(results: &[npkill_rs::types::FoundFolder], config: &npkill_rs::types::ScanConfig) {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let total_size: u64 = results.iter().filter_map(|f| f.size).sum();

    println!(
        "npkill-rs v{} — {} target(s) in {}",
        env!("CARGO_PKG_VERSION"),
        results.len(),
        config.root_path.display(),
    );

    let sep = "─".repeat(72);
    println!("{}", sep);
    println!(" {:<3} {:<6} {:>8} {:>5}  Path", "S", "Type", "Size", "Age");
    println!(
        " {} {} {:>8} {:>5}  {}",
        "─".repeat(3),
        "─".repeat(6),
        "─".repeat(8),
        "─".repeat(5),
        "─".repeat(40)
    );

    for f in results {
        let tag = match f.target {
            npkill_rs::types::TargetKind::NodeModules => "NM",
            npkill_rs::types::TargetKind::NextDotNext => "NX",
        };
        let sc = match f.status {
            npkill_rs::types::FolderStatus::Pending => " ",
            npkill_rs::types::FolderStatus::Deleting => ">",
            npkill_rs::types::FolderStatus::Deleted => "D",
            npkill_rs::types::FolderStatus::Error => "E",
        };
        let sz = f
            .size
            .map(npkill_rs::deleter::format_size)
            .unwrap_or_else(|| "?".into());
        let age = f
            .last_modified
            .map_or_else(String::new, |ts| format!("{}d", (now_secs - ts) / 86400));
        let risk = if f.risk == npkill_rs::types::RiskLevel::Sensitive {
            "!"
        } else {
            " "
        };
        println!(
            " {:<3} {:<6} {:>8} {:>5}  {}{}",
            sc,
            tag,
            sz,
            age,
            risk,
            f.path.display()
        );
    }

    println!("{}", sep);
    println!(
        " Found: {}  |  Reclaimable: {}  |  Scan time: {}",
        results.len(),
        npkill_rs::deleter::format_size(total_size),
        format_duration(0.0),
    );
}

fn parse_indices(input: &str, max: usize) -> Vec<usize> {
    let mut indices = Vec::new();
    for part in input.split([',', ' ']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let lo = a.trim().parse::<usize>().unwrap_or(0).saturating_sub(1);
            let hi = b.trim().parse::<usize>().unwrap_or(max).saturating_sub(1);
            for i in lo..=hi.min(max.saturating_sub(1)) {
                if !indices.contains(&i) {
                    indices.push(i);
                }
            }
        } else if let Ok(n) = part.parse::<usize>() {
            let idx = n.saturating_sub(1);
            if idx < max && !indices.contains(&idx) {
                indices.push(idx);
            }
        }
    }
    indices.sort();
    indices
}

fn main() -> anyhow::Result<()> {
    let cli = CliArgs::parse();
    let is_json = cli.json;
    let is_table = cli.table;
    let config: npkill_rs::types::ScanConfig = cli.into();

    if config.dry_run {
        eprintln!("[DRY RUN] No folders will actually be deleted.");
    }

    // ── JSON mode ──
    if is_json {
        let scanner_config = config.clone();
        let scanner = Scanner::new(scanner_config);
        let results = scanner.scan();
        let json = serde_json::to_string_pretty(&results)?;
        println!("{json}");
        return Ok(());
    }

    // ── Table mode (CLI, no TUI) ──
    if is_table {
        let scanner_config = config.clone();
        let scanner = Scanner::new(scanner_config);
        let results = scanner.scan();
        print_table(&results, &config);

        if config.yes && config.delete_all {
            println!();
            println!("Deleting all {} folder(s)...", results.len());
            let paths: Vec<&npkill_rs::types::FoundFolder> = results.iter().collect();
            let del_results = npkill_rs::deleter::batch_delete(&paths, config.dry_run);
            let deleted = del_results
                .iter()
                .filter(|r| {
                    matches!(
                        r.1,
                        npkill_rs::deleter::DeleteResult::Success
                            | npkill_rs::deleter::DeleteResult::DryRun
                    )
                })
                .count();
            let total_sz: u64 = results.iter().filter_map(|f| f.size).sum();
            let mode = if config.dry_run {
                "would free"
            } else {
                "freed"
            };
            println!(
                "Deleted {}/{} folder(s) — {} {mode}",
                deleted,
                results.len(),
                npkill_rs::deleter::format_size(total_sz)
            );
        } else if config.yes && !config.delete_all {
            // --yes without --delete-all: prompt per folder
            for f in results.iter() {
                let tag = match f.target {
                    npkill_rs::types::TargetKind::NodeModules => "NM",
                    npkill_rs::types::TargetKind::NextDotNext => "NX",
                };
                print!(
                    "Delete [{}] {} ({})? [y/N] ",
                    tag,
                    f.path.display(),
                    f.size
                        .map(npkill_rs::deleter::format_size)
                        .unwrap_or_default()
                );
                std::io::Write::flush(&mut std::io::stdout()).ok();
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).ok();
                if input.trim().eq_ignore_ascii_case("y") {
                    let _ = npkill_rs::deleter::delete_folder(f, config.dry_run);
                }
            }
        } else {
            // Interactive prompt: let user choose
            println!();
            println!("Delete folders?");
            println!("  all          — delete everything");
            println!("  none / Enter — skip");
            println!("  1,3,5        — delete by index (comma/space separated)");
            println!("  1-4          — range");
            print!("> ");
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            let input = input.trim().to_lowercase();

            let indices: Vec<usize> = if input == "all" {
                (0..results.len()).collect()
            } else if input.is_empty() || input == "none" {
                Vec::new()
            } else {
                parse_indices(&input, results.len())
            };

            if !indices.is_empty() {
                let mode = if config.dry_run { " (dry run)" } else { "" };
                println!(
                    "Deleting {}/{} folder(s){mode}...",
                    indices.len(),
                    results.len()
                );
                for &i in &indices {
                    if i < results.len() {
                        let f = &results[i];
                        println!("  Deleting: {}", f.path.display());
                        let _ = npkill_rs::deleter::delete_folder(f, config.dry_run);
                    }
                }
                let total_sz: u64 = indices
                    .iter()
                    .filter_map(|&i| results.get(i))
                    .filter_map(|f| f.size)
                    .sum();
                println!(
                    "Done — {} {}freed.",
                    npkill_rs::deleter::format_size(total_sz),
                    if config.dry_run { "would be " } else { "" }
                );
            }
        }
        return Ok(());
    }

    // ── TUI mode ──
    let progress = ScanProgress::new();

    let mut app = Arc::new(Mutex::new(App::new(config)));

    let scanner_config = app.lock().unwrap().config.clone();
    let scanner = Scanner::new(scanner_config).with_progress(progress.clone());
    let stop_flag = scanner.stop_flag();

    let app_clone = app.clone();
    let prog_clone = progress.clone();
    let scan_handle = std::thread::spawn(move || {
        let results = scanner.scan();
        let mut a = app_clone.lock().unwrap();
        a.process_scan_results(results);
        // Final sync of incremental progress
        let p = prog_clone.lock().unwrap();
        a.current_scan_path = p.current_path.clone();
        a.stats.total_found = a.stats.total_found.max(p.folders_found);
        a.stats.total_size_reclaimable =
            a.stats.total_size_reclaimable.max(p.total_size_reclaimable);
    });

    let tui_result = npkill_rs::app::run_tui(&mut app, progress);

    stop_flag.store(true, Ordering::SeqCst);

    let _ = scan_handle.join();

    let a = app.lock().unwrap();
    println!();
    println!("{}", "─".repeat(48));
    println!(
        "  Summary  ({})",
        format_duration(a.stats.scan_duration_secs)
    );
    println!("{}", "─".repeat(48));
    println!("  Found:      {}", a.stats.total_found);
    println!("  Deleted:    {}", a.stats.total_deleted);
    println!(
        "  Space freed: {}",
        npkill_rs::deleter::format_size(a.stats.total_size_freed)
    );
    if a.stats.total_errors > 0 {
        println!("  Errors:     {}", a.stats.total_errors);
    }
    if a.config.dry_run {
        println!("  (Dry run — no files were actually deleted)");
    }

    if let Err(e) = tui_result {
        eprintln!("TUI error: {e}");
    }

    Ok(())
}
