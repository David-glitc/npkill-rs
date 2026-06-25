#!/usr/bin/env bash
set -euo pipefail

REPO="David-glitc/npkill-rs"
BIN_NAME="npkill-rs"
CACHE_DIR="${HOME}/.cache/npkill-rs"

# ── Detect platform ────────────────────────────────────────────
detect_target() {
  local arch os
  arch="$(uname -m)"
  os="$(uname -s)"

  case "$os" in
    Linux)  os="unknown-linux-gnu"    ext="tar.gz" ;;
    Darwin) os="apple-darwin"         ext="tar.gz" ;;
    *)      echo "Unsupported OS: $os" >&2; exit 1 ;;
  esac

  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *)               echo "Unsupported arch: $arch" >&2; exit 1 ;;
  esac

  echo "${arch}-${os}" "$ext"
}

# ── Get latest release tag ─────────────────────────────────────
latest_tag() {
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | cut -d'"' -f4
}

# ── Main ───────────────────────────────────────────────────────
main() {
  local target ext tag binary
  read -r target ext < <(detect_target)

  tag="$(latest_tag)"
  binary="${CACHE_DIR}/${BIN_NAME}-${tag}"

  if [ ! -x "$binary" ]; then
    mkdir -p "$CACHE_DIR"
    local url="https://github.com/${REPO}/releases/download/${tag}/${BIN_NAME}-${target}.tar.gz"
    curl -fsSL "$url" | tar xz -C "$CACHE_DIR" "${BIN_NAME}" 2>/dev/null
    mv "${CACHE_DIR}/${BIN_NAME}" "$binary"
    chmod +x "$binary"
  fi

  exec "$binary" "$@"
}

main "$@"
