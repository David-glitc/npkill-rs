use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use clap::Parser;

use npkill_rs::app::App;
use npkill_rs::config::CliArgs;
use npkill_rs::scanner::Scanner;

fn main() -> anyhow::Result<()> {
    let cli = CliArgs::parse();
    let json_mode = cli.json;
    let config: npkill_rs::types::ScanConfig = cli.into();

    if config.dry_run {
        eprintln!("[DRY RUN] No folders will actually be deleted.");
    }

    if json_mode {
        let scanner_config = config.clone();
        let scanner = Scanner::new(scanner_config);
        let results = scanner.scan();
        let json = serde_json::to_string_pretty(&results)?;
        println!("{json}");
        return Ok(());
    }

    let current_scan_path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    let mut app = Arc::new(Mutex::new(App::new(config)));

    let scanner_config = app.lock().unwrap().config.clone();
    let scanner = Scanner::new(scanner_config).with_current_path(current_scan_path.clone());
    let stop_flag = scanner.stop_flag();

    let app_clone = app.clone();
    let cp_clone = current_scan_path.clone();
    let scan_handle = std::thread::spawn(move || {
        let results = scanner.scan();
        let mut a = app_clone.lock().unwrap();
        a.current_scan_path = cp_clone.lock().unwrap().clone();
        a.process_scan_results(results);
    });

    let tui_result = npkill_rs::app::run_tui(&mut app, current_scan_path);

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
