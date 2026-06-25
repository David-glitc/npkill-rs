# Contributing to npkill-rs

## Development Setup

**Prerequisites:** Rust 1.96.0+ (edition 2021)

```bash
git clone https://github.com/David-glitc/npkill-rs
cd npkill-rs
cargo build
```

## Project Structure

| Path | Purpose |
|------|---------|
| `src/main.rs` | Entry point — CLI dispatch (TUI / `--json` / `--table`) |
| `src/app.rs` | TUI app state, keyboard/mouse event handling, all draw functions |
| `src/scanner.rs` | Two-phase scan: Phase 1 (traversal) + Phase 2 (sizing) |
| `src/readdir.rs` | Raw `getdents64` syscall on Linux (fallback to `fs::read_dir`) |
| `src/deleter.rs` | Folder deletion, size formatting, batch operations |
| `src/risk.rs` | Sensitive-location analysis for confirmation prompts |
| `src/config.rs` | `CliArgs` (clap derive) → `ScanConfig` conversion |
| `src/types.rs` | Shared types: `ScanConfig`, `FoundFolder`, `AppStats`, etc. |
| `src/update.rs` | GitHub API update check (non-blocking, 3s timeout) |

## Development Workflow

### Build

```bash
cargo build                    # debug
cargo build --release          # release (stripped, optimized)
```

### Test

```bash
cargo test                     # 53 tests (46 unit + 7 integration)
```

### Lint

```bash
cargo clippy                   # zero warnings policy
```

### Run locally

```bash
cargo run -- --dry-run -d .                    # TUI mode (dry run)
cargo run -- --dry-run -d ~/projects --json    # JSON output
cargo run -- --dry-run -d ~/projects --table   # Table mode
```

## Architecture Notes

### Two-Phase Scan

- **Phase 1** (~700ms on `/home`): Traverses the directory tree using raw `getdents64` syscalls + parallel Rayon top-level walk. Skips hidden dirs (opt-in with `--include-hidden`) and dirs without project markers (`package.json`, `yarn.lock`, etc.) past depth 4. Results appear in the TUI immediately.
- **Phase 2**: Computes sizes via `du -sb` subprocess (WalkDir fallback). Runs in the background — the TUI is never blocked.

### Key Optimizations

- **`getdents64`** — raw Linux syscall replaces `std::fs::read_dir` for 8KB batched reads with zero per-entry allocations
- **Hidden dir skip** — `.cache`, `.config`, `.npm`, `.yarn` etc. skipped unless `--include-hidden`
- **Project marker detection** — dirs without `package.json`/`yarn.lock`/`bun.lock`/`deno.lockb`/`pnpm-lock.yaml` aren't recursed past depth 4
- **Sequential sizing** — `du -sb` runs sequentially (not parallel) to avoid disk I/O contention
- **`mimalloc`** global allocator for reduced memory fragmentation

### Platform Support

| Platform | Binary | Sizing |
|----------|--------|--------|
| Linux x86_64 | `npkill-rs-linux-x86_64.tar.gz` | `du -sb` |
| Linux aarch64 | `npkill-rs-linux-aarch64.tar.gz` | `du -sb` |
| Windows x86_64 | `npkill-rs-windows-x86_64.zip` | WalkDir fallback |

## Release Process

1. Update version in `Cargo.toml` and `CHANGELOG.md`
2. Commit and tag: `git tag vX.Y.Z`
3. Push: `git push && git push --tags`
4. GitHub Actions builds and uploads binaries for all platforms

The release workflow is at `.github/workflows/release.yml`.

## PR Guidelines

- All tests must pass: `cargo test`
- Zero clippy warnings: `cargo clippy`
- New features should include tests
- Keep commits focused and messages concise

## License

MIT — see `LICENSE`.
