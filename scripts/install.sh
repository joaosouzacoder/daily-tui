#!/usr/bin/env bash
# install.sh — instala o daily-tui e todas as CLIs que ele orquestra.
#
# O que faz (idempotente — pode rodar de novo sem medo):
#   1. instala dependências de sistema (curl, git, jq, toolchain C, openssl);
#   2. instala Rust (rustup) e uv, se faltarem;
#   3. `cargo install` do himalaya e do ghpending;
#   4. `uv tool install` do gcalcli;
#   5. compila o daily-tui em release e linka em ~/.local/bin;
#   6. copia os helpers jirapending e mstodo para ~/.local/bin.
#
# NÃO configura credenciais (contas de e-mail, OAuth do Google, token do Jira):
# isso é manual e está documentado no README.md ("Configuração das contas").
#
# Uso:
#   scripts/install.sh                  # instala tudo
#   scripts/install.sh --skip-system    # pula o passo de pacotes do SO
#   scripts/install.sh --skip-clis      # só compila/linka o daily-tui + helpers
#   scripts/install.sh --bin-dir DIR    # destino dos binários (default ~/.local/bin)
#   scripts/install.sh --help
set -euo pipefail

# ---- localização do repo -------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# ---- opções --------------------------------------------------------------
BIN_DIR="${HOME}/.local/bin"
SKIP_SYSTEM=0
SKIP_CLIS=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-system) SKIP_SYSTEM=1; shift ;;
    --skip-clis)   SKIP_CLIS=1; shift ;;
    --bin-dir)     BIN_DIR="$2"; shift 2 ;;
    --help|-h)
      awk 'NR>1 && /^#/ {sub(/^# ?/,""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"
      exit 0 ;;
    *) echo "opção desconhecida: $1" >&2; exit 1 ;;
  esac
done

# ---- logging -------------------------------------------------------------
if [[ -t 1 ]]; then B=$'\e[1m'; G=$'\e[32m'; Y=$'\e[33m'; R=$'\e[31m'; X=$'\e[0m'
else B=; G=; Y=; R=; X=; fi
step() { echo "${B}${G}==>${X} ${B}$*${X}"; }
info() { echo "    $*"; }
warn() { echo "${Y}!!  $*${X}" >&2; }
die()  { echo "${R}xx  $*${X}" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# ---- sudo (só se não for root) ------------------------------------------
SUDO=""
if [[ "$(id -u)" -ne 0 ]]; then
  have sudo && SUDO="sudo" || warn "sem sudo e não-root: pacotes de sistema podem falhar"
fi

# ---- detecção do gerenciador de pacotes ----------------------------------
detect_pm() {
  if have apt-get; then echo apt
  elif have pacman; then echo pacman
  elif have dnf;    then echo dnf
  elif have zypper; then echo zypper
  elif have apk;    then echo apk
  elif have brew;   then echo brew
  else echo unknown; fi
}

install_system_deps() {
  local pm; pm="$(detect_pm)"
  step "Dependências de sistema (gerenciador: $pm)"
  case "$pm" in
    apt)
      $SUDO apt-get update -qq
      $SUDO apt-get install -y --no-install-recommends \
        curl git jq ca-certificates build-essential pkg-config libssl-dev \
        libsecret-tools gnome-keyring ;;
    pacman)
      $SUDO pacman -Sy --needed --noconfirm curl git jq base-devel openssl pkgconf \
        libsecret gnome-keyring ;;
    dnf)
      $SUDO dnf install -y curl git jq gcc make openssl-devel pkgconf-pkg-config \
        libsecret gnome-keyring ;;
    zypper)
      $SUDO zypper --non-interactive install curl git jq gcc make libopenssl-devel pkg-config \
        libsecret-tools gnome-keyring ;;
    apk)
      $SUDO apk add --no-cache curl git jq build-base openssl-dev pkgconf \
        libsecret gnome-keyring ;;
    brew)
      brew install jq openssl@3 pkg-config libsecret ;;
    *)
      warn "gerenciador não reconhecido — instale: curl git jq, toolchain C, openssl-dev, pkg-config, libsecret (secret-tool) + um keyring (gnome-keyring/kwallet)" ;;
  esac
}

install_rust() {
  if have cargo; then info "Rust já presente ($(cargo --version))"; return; fi
  step "Instalando Rust (rustup)"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  # shellcheck disable=SC1091
  source "${CARGO_HOME:-$HOME/.cargo}/env"
}

install_uv() {
  if have uv; then info "uv já presente ($(uv --version))"; return; fi
  step "Instalando uv"
  curl -LsSf https://astral.sh/uv/install.sh | sh
  export PATH="$HOME/.local/bin:$PATH"
}

install_clis() {
  step "himalaya (e-mail) — cargo install"
  have himalaya && info "já instalado" || cargo install himalaya --locked

  step "ortie (broker OAuth do e-mail) — cargo install"
  have ortie && info "já instalado" || cargo install ortie --locked

  step "ghpending (PRs/issues) — cargo install"
  have ghpending && info "já instalado" || cargo install ghpending --locked

  step "gcalcli (agenda) — uv tool install"
  have gcalcli && info "já instalado" || uv tool install gcalcli
}

build_daily_tui() {
  step "Compilando o daily-tui (release)"
  ( cd "$REPO_ROOT" && cargo build --release )
  mkdir -p "$BIN_DIR"
  ln -sf "$REPO_ROOT/target/release/daily-tui" "$BIN_DIR/daily-tui"
  info "linkado: $BIN_DIR/daily-tui -> target/release/daily-tui"
}

install_helpers() {
  step "Instalando helpers (jirapending, mstodo)"
  mkdir -p "$BIN_DIR"
  install -m 0755 "$SCRIPT_DIR/jirapending" "$BIN_DIR/jirapending"
  install -m 0755 "$SCRIPT_DIR/mstodo"      "$BIN_DIR/mstodo"
  info "copiados para $BIN_DIR"
}

check_path() {
  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR não está no PATH — adicione: export PATH=\"$BIN_DIR:\$PATH\"" ;;
  esac
}

# ---- execução ------------------------------------------------------------
[[ "$SKIP_SYSTEM" -eq 1 ]] || install_system_deps
install_rust
install_uv
[[ "$SKIP_CLIS" -eq 1 ]] || install_clis
build_daily_tui
install_helpers
check_path

echo
step "Pronto! Binários em: $BIN_DIR"
cat <<EOF
    Agora configure as AUTENTICAÇÕES (a parte que faz tudo funcionar):

      ./scripts/setup-auth.sh email     # e-mail (ortie + himalaya, abre o navegador)
      ./scripts/setup-auth.sh google    # agenda (OAuth do Google Cloud)
      ./scripts/setup-auth.sh mstodo    # tarefas (Microsoft To Do)
      export GITHUB_TOKEN=... && ghpending add     # PRs/issues
      # Jira: defina JIRA_EMAIL / JIRA_CLOUD / JIRA_TOKEN (veja scripts/daily-tui.env.example)

    Para checar o que já está funcionando:
      ./scripts/setup-auth.sh check

    Detalhes no README, seção "Configuração das contas". Depois: daily-tui
EOF
