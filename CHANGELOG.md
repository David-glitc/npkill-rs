# Changelog

## [0.5.0] - 2026-06-25

### Added
- **Hidden directory skip** (default): `.cache`, `.config`, `.npm`, `.yarn`, `.local` etc. skipped — Phase 1 ~700ms vs 136s before on `/home`
- **`--include-hidden`** flag to opt out (scan hidden dirs as well)
- **Checkbox selection**: `Space` toggles checkbox on selected folder, `Ctrl+D` batch-deletes checked folders
- **Confirmation popup** before batch delete: `y`/`Enter` to confirm, `n`/`Esc` to cancel
- **Two-phase scan**: TUI shows discovered folders immediately (Phase 1), sizes computed in background (Phase 2 via `du`)
- **Project marker detection**: Skips deep recursion into directories without `package.json`, `yarn.lock`, `bun.lock`, `deno.lockb`, or `pnpm-lock.yaml` beyond depth 4
- **Interactive max-depth** setting in Settings popup (cycles: 2 → 3 → 5 → 10 → ∞)
- **Search path** shown in status bar during scan

### Changed
- Phase 2 sizing uses `du -sb` instead of WalkDir (WalkDir fallback if `du` unavailable)
- Phase 2 runs sequentially (no Rayon) to avoid disk I/O contention from parallel `du` processes
- Mtime uses cheap single `stat` call instead of full WalkDir traversal
- `process_scan_results` called before Phase 2 — TUI no longer freezes during sizing

### Keybindings changed
- `Space` now toggles checkbox (was `Enter`/`Space` delete)
- `Enter` still deletes selected folder immediately
- `Ctrl+D` opens batch-delete confirmation for checked folders

## [0.4.0] - 2026-06-25

### Added
- Automatic update check on startup (GitHub API, 3s timeout, silent fail)
- `UPDATE vX.Y.Z` badge in TUI status bar when newer version exists
- Deletion ETA in stats header (rate × remaining)
- `run.sh` one-liner: `curl -fsSL https://git.io/npkill-rs | bash -s -- -d .`
- Telescope pass: upfront directory count for accurate scan progress (`dirs: 123/456`)

### Changed
- **Default `--max-depth` changed from unlimited to 5** — avoids deep system directory crawls, ~10× faster on large filesystems. Override with `-m N` or `-m 0`.
- Settings/Help popups are now truly modal (mouse clicks swallowed while open)
- Scan progress shows `dirs_visited/total_dirs` instead of fake ETA estimate

## [0.3.0] - 2026-06-25

### Added
- `--max-depth / -m` CLI flag to limit search depth (e.g. `--max-depth 2` only scans 2 levels deep)
- `max_depth` field in `ScanConfig` for programmatic use
- CHANGELOG.md

### Performance
- Depth-limited scans skip traversal beyond the specified depth, reducing Phase 1 time proportionally (e.g. `--max-depth 0` is ~2.3× faster on a 300-dir tree)

## [0.2.0] - 2026-06-25

### Added
- Non-TUI `--table` mode with interactive folder selection (`all`, `1,3,5`, `1-4`, `none`)
- `--yes --delete-all` auto-delete in table mode
- Settings and Help as centered popup overlays (Esc/click to close, Tab/Settings/Help to open)
- Incremental folder listing during scan (folders appear immediately as discovered)
- Live elapsed time with ETA during scan
- `<1ms` display for very short durations
- Deletion animation: progress bar `[████░░]` with gradual path name erasure
- Status bar badges for `SEARCH`, `DRY RUN`, `SCANNING` modes
- Layout adapts to short terminal heights
- Gauge shows 100% bar on scan completion with duration + folder count

### Changed
- Manual `read_dir` DFS for Phase 1 replaces WalkDir `filter_entry` (Phase 1: ~14ms → 108µs, full scan: ~113ms → ~81ms, 28% faster)
- All blocks use `BorderType::Rounded`
- Folder list uses alternating row background colors

## [0.1.2] - 2026-06-25

### Changed
- Phase 1 traversal replaced WalkDir with manual `fs::read_dir` DFS (Phase 1 dropped from ~14ms to 108µs)
- Full scan improved from ~113ms to ~81ms (28% faster)

## [0.1.1] - 2026-06-25

### Added
- Live scan path in TUI header
- `--json` output mode
- Grid layout with column headers
- Bordered scan summary box with scan time display

### Fixed
- Windows packaging uses zip instead of 7z

## [0.1.0] - 2026-06-25

### Added
- Interactive TUI for finding and deleting `node_modules` and `.next` directories
- Theme system: Catppuccino Mocha, Nord, Tokyo Night
- Dashboard layout with tabs (Scan, Settings, Help)
- Release workflow with cross-platform binaries (x86_64 Linux, aarch64 Linux, x86_64 Windows)
- Install script
