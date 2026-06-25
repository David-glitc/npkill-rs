# npkill-rs

**Fast interactive tool to find and delete `node_modules` and `.next` directories — written in Rust.**

A Rust reimplementation of [npkill](https://github.com/voidcosmos/npkill) with added support for `.next` directories, mouse interactions, real-time search, sorting, and a polished TUI.

Created by [David Pere](https://kierkegaard.space) — [@davidpereishim](https://x.com/davidpereishim)

## Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/David-glitc/npkill-rs/main/install.sh | bash
```

This downloads the latest binary for your platform, installs it to `~/.local/bin`, and adds it to your `PATH`.

## Features

- **Dual target scanning** — finds both `node_modules` and `.next` directories
- **Interactive TUI** — keyboard + mouse navigation, scroll support
- **Real-time search** — press `/` to filter results by path
- **Multi-sort** — sort by Size, Date, or Path (press `s` to cycle, `S` to reverse)
- **Live animations** — spinner, progress bar, deletion effects
- **ETA display** — estimated time remaining during scans
- **Blacklist / Whitelist** — skip specific directories or force-include them
- **Risk analysis** — detects sensitive locations (`.config`, `AppData`, etc.)
- **Dry-run mode** — preview deletions without touching files
- **Batch delete** — `--delete-all` with optional `-y` confirmation
- **JSON output** — machine-readable output (use `--json`)

## Installation

### Quick (pre-built binary)

```bash
curl -fsSL https://raw.githubusercontent.com/David-glitc/npkill-rs/main/install.sh | bash
```

Supports Linux (x86_64, aarch64) and macOS (x86_64, arm64).

### From source

```bash
git clone https://github.com/David-glitc/npkill-rs
cd npkill-rs
cargo build --release
cp target/release/npkill-rs ~/.local/bin/
```

### With Cargo

```bash
cargo install --git https://github.com/David-glitc/npkill-rs
```

## Usage

```bash
npkill-rs [OPTIONS]
```

### Options

| Flag | Description |
|------|-------------|
| `-d, --directory <PATH>` | Root directory to scan (default: `.`) |
| `-t, --targets <LIST>` | Comma-separated targets: `node_modules`, `.next` |
| `-e, --exclude <LIST>` | Paths to exclude |
| `-b, --blacklist <LIST>` | Paths to always skip |
| `-w, --whitelist <LIST>` | Paths to never skip (overrides exclude/blacklist) |
| `--dry-run` | Simulate deletions |
| `--exclude-sensitive` | Skip sensitive locations |
| `--delete-all` | Delete all found folders (use with `-y`) |
| `--disable-size` | Skip size calculation |
| `--disable-age` | Skip age calculation |
| `--json` | Output in JSON format |

### Interactive Controls

| Key | Action |
|-----|--------|
| `↑/k` / `↓/j` | Move selection |
| `PgUp` / `PgDn` | Page scroll |
| `Home/g` / `End/G` | First / last item |
| `Enter` / `Space` | Delete selected |
| `d` | Delete all (with `--delete-all`) |
| `s` | Cycle sort (Size → Date → Path) |
| `S` | Reverse sort direction |
| `/` | Enter search mode |
| `h` / `?` | Toggle help |
| `q` / `Esc` | Quit |
| Mouse click | Select item |
| Right click | Delete selected |
| Scroll wheel | Navigate |

## Building

```bash
cargo build --release
```

## Tests

```bash
cargo test
```

## Author

**David Pere**
- Website: [kierkegaard.space](https://kierkegaard.space)
- X / Twitter: [@davidpereishim](https://x.com/davidpereishim)

## License

MIT
