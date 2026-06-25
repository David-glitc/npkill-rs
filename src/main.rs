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
            npkill_rs::deleter::batch_delete(&paths, config.dry_run);
            println!("Done.");
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
