use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use clap::Parser;

use npkill_rs::app::App;
use npkill_rs::config::CliArgs;
use npkill_rs::scanner::Scanner;

fn main() -> anyhow::Result<()> {
    let cli = CliArgs::parse();
    let config: npkill_rs::types::ScanConfig = cli.into();

    if config.dry_run {
        eprintln!("[DRY RUN] No folders will actually be deleted.");
    }

    let mut app = Arc::new(Mutex::new(App::new(config)));

    let scanner_config = app.lock().unwrap().config.clone();
    let scanner = Scanner::new(scanner_config);
    let stop_flag = scanner.stop_flag();

    let app_clone = app.clone();
    let scan_handle = std::thread::spawn(move || {
        let results = scanner.scan();
        let mut a = app_clone.lock().unwrap();
        a.process_scan_results(results);
    });

    let tui_result = npkill_rs::app::run_tui(&mut app);

    stop_flag.store(true, Ordering::SeqCst);

    let _ = scan_handle.join();

    let a = app.lock().unwrap();
    println!();
    println!("--- Summary ---");
    println!("  Found: {}", a.stats.total_found);
    println!("  Deleted: {}", a.stats.total_deleted);
    println!(
        "  Space freed: {}",
        npkill_rs::deleter::format_size(a.stats.total_size_freed)
    );
    if a.stats.total_errors > 0 {
        println!("  Errors: {}", a.stats.total_errors);
    }
    if a.config.dry_run {
        println!("  (Dry run — no files were actually deleted)");
    }

    if let Err(e) = tui_result {
        eprintln!("TUI error: {e}");
    }

    Ok(())
}
