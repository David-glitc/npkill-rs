#!/usr/bin/env bash
set -euo pipefail

REPO="David-glitc/npkill-rs"
BIN_NAME="npkill-rs"
INSTALL_DIR="${HOME}/.local/bin"

# ── Color helpers ──────────────────────────────────────────────
BOLD='\033[1m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

info()  { printf "${CYAN}%s${NC}\n" "$*"; }
ok()    { printf "${GREEN}%s${NC}\n" "$*"; }
bold()  { printf "${BOLD}%s${NC}\n" "$*"; }

# ── Detect platform ────────────────────────────────────────────
detect_target() {
  local arch os
  arch="$(uname -m)"
  os="$(uname -s)"

  case "$os" in
    Linux)  os="unknown-linux-gnu" ;;
    Darwin) os="apple-darwin" ;;
    *)      echo "Unsupported OS: $os"; exit 1 ;;
  esac

  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    aarch64 | arm64) arch="aarch64" ;;
    *)               echo "Unsupported arch: $arch"; exit 1 ;;
  esac

  echo "${arch}-${os}"
}

# ── Get latest release tag ─────────────────────────────────────
latest_tag() {
  curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | cut -d'"' -f4
}

# ── Main ───────────────────────────────────────────────────────
main() {
  bold "npkill-rs installer"
  echo

  local target tag url
  target="$(detect_target)"
  info "Detected target: ${target}"

  tag="$(latest_tag)"
  info "Latest release: ${tag}"

  url="https://github.com/${REPO}/releases/download/${tag}/${BIN_NAME}-${target}.tar.gz"

  mkdir -p "$INSTALL_DIR"

  bold "Downloading ${BIN_NAME} ${tag}..."
  curl -fsSL "$url" | tar xz -C "$INSTALL_DIR" "${BIN_NAME}"
  chmod +x "${INSTALL_DIR}/${BIN_NAME}"

  ok "Installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"

  # ── Add to PATH if missing ───────────────────────────────────
  local rc=""
  case "${SHELL:-}" in
    */zsh)  rc="${HOME}/.zshrc" ;;
    */bash) rc="${HOME}/.bashrc" ;;
  esac

  if [ -n "$rc" ]; then
    if grep -q 'PATH.*\.local/bin' "$rc" 2>/dev/null; then
      info "${INSTALL_DIR} already in PATH (${rc})"
    else
      printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc"
      ok "Added ${INSTALL_DIR} to PATH in ${rc}"
    fi
  else
    info "Add ${INSTALL_DIR} to your PATH manually or restart your shell."
  fi

  echo
  bold "Usage:"
  echo "  ${BIN_NAME} --help"
  echo "  ${BIN_NAME} -d /path/to/scan"
  echo
  ok "Done! Restart your shell or run: source ${rc:-~/.bashrc}"
}

main "$@"
