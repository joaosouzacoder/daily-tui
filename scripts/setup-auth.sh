#!/usr/bin/env bash
# setup-auth.sh — configura e valida TODAS as autenticações do daily-tui.
#
# A parte chata do daily-tui é autenticar as CLIs. Este script automatiza o que
# dá e te diz exatamente o que falta no resto.
#
# Subcomandos:
#   check          (default) — testa cada painel e mostra PASS/FAIL. NÃO altera nada.
#   email          — configura ortie + himalaya (Gmail/Workspace via OAuth2)
#   google         — configura gcalcli (agenda) + gtasks (tarefas) via OAuth do GCP
#   all            — email, depois google, depois check
#
# Flags:
#   --force        — sobrescreve configs existentes (ortie/himalaya) sem perguntar
#
# Os fluxos OAuth abrem o NAVEGADOR — rode num ambiente gráfico (não via SSH puro).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TPL="$SCRIPT_DIR/templates"
FORCE=0

# Contas — precisam bater com src/data/mod.rs (himalaya_name / gcalcli_dir).
HIMALAYA_ACCOUNTS=(personal work)              # nomes no himalaya
GCAL_DIRS=(personal work)                       # subdirs do gcalcli
GCAL_ROOT="$HOME/.local/share/gcalcli-accounts"

if [[ -t 1 ]]; then B=$'\e[1m'; G=$'\e[32m'; Y=$'\e[33m'; R=$'\e[31m'; X=$'\e[0m'
else B=; G=; Y=; R=; X=; fi
step() { echo "${B}${G}==>${X} ${B}$*${X}"; }
info() { echo "    $*"; }
warn() { echo "${Y}!!  $*${X}" >&2; }
die()  { echo "${R}xx  $*${X}" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

ask() { local p="$1" d="${2:-}" a; read -r -p "    $p${d:+ [$d]}: " a; echo "${a:-$d}"; }

confirm_overwrite() {
  local f="$1"
  [[ ! -e "$f" ]] && return 0
  [[ "$FORCE" -eq 1 ]] && return 0
  local a; read -r -p "    $f já existe. Sobrescrever? [y/N]: " a
  [[ "$a" =~ ^[Yy]$ ]]
}

# ---------------------------------------------------------------- doctor ----
# probe NOME COMANDO DICA — roda o comando e imprime PASS/FAIL.
probe() {
  local name="$1" cmd="$2" hint="$3"
  if eval "$cmd" >/dev/null 2>&1; then
    printf "    ${G}PASS${X}  %-22s\n" "$name"
  else
    printf "    ${R}FAIL${X}  %-22s  ${Y}%s${X}\n" "$name" "$hint"
  fi
}

doctor() {
  step "Diagnóstico das autenticações (painéis do daily-tui)"
  echo "    CLIs no PATH:"
  for c in himalaya gcalcli ghpending jirapending gtasks ortie secret-tool jq; do
    have "$c" && printf "      ${G}✓${X} %s\n" "$c" || printf "      ${R}✗${X} %s (ausente)\n" "$c"
  done
  echo
  for acc in "${HIMALAYA_ACCOUNTS[@]}"; do
    probe "email:$acc" \
      "himalaya envelope list -a $acc --page-size 1 -o json" \
      "rode: scripts/setup-auth.sh email"
  done
  for dir in "${GCAL_DIRS[@]}"; do
    probe "agenda:$dir" \
      "XDG_DATA_HOME=$GCAL_ROOT/$dir gcalcli list" \
      "rode: scripts/setup-auth.sh google"
  done
  probe "pulls (ghpending)" "ghpending" \
    "defina GITHUB_TOKEN e rode: ghpending add"
  probe "jira (jirapending)" "jirapending" \
    "defina JIRA_EMAIL / JIRA_CLOUD / JIRA_TOKEN"
  probe "tasks (gtasks)" "gtasks list" \
    "rode: scripts/setup-auth.sh google (gtasks auth)"
}

# ----------------------------------------------------------------- email ----
setup_email() {
  step "E-mail: ortie (broker OAuth) + himalaya"
  have ortie || die "ortie ausente — rode scripts/install.sh"
  have secret-tool || die "secret-tool ausente — instale libsecret (e tenha gnome-keyring/kwallet rodando)"
  have himalaya || die "himalaya ausente — rode scripts/install.sh"

  # 1) config do ortie
  local ortie_cfg="$HOME/.config/ortie/config.toml"
  mkdir -p "$(dirname "$ortie_cfg")"
  if confirm_overwrite "$ortie_cfg"; then
    cp "$TPL/ortie.toml" "$ortie_cfg"
    info "ortie config escrito em $ortie_cfg"
  else
    info "mantendo $ortie_cfg existente"
  fi

  # 2) consentimento OAuth de cada conta (abre o navegador, salva no keyring)
  info "Vai abrir o navegador para cada conta. Faça login na conta CERTA em cada uma."
  for acc in gmail-personal gmail-work; do
    step "ortie auth — $acc"
    ortie -a "$acc" auth || warn "ortie auth ($acc) falhou — repita depois"
  done

  # 3) config do himalaya (preenche e-mails/nomes no template)
  local hcfg="$HOME/.config/himalaya/config.toml"
  mkdir -p "$(dirname "$hcfg")"
  if confirm_overwrite "$hcfg"; then
    local ep en ew nw
    ep="$(ask 'E-mail da conta PESSOAL (@gmail.com)')"
    en="$(ask 'Nome de exibição pessoal' "$ep")"
    ew="$(ask 'E-mail da conta WORK (Workspace)')"
    nw="$(ask 'Nome de exibição work' "$ew")"
    sed -e "s|__EMAIL_PERSONAL__|$ep|g" -e "s|__NAME_PERSONAL__|$en|g" \
        -e "s|__EMAIL_WORK__|$ew|g"     -e "s|__NAME_WORK__|$nw|g" \
        "$TPL/himalaya.toml" > "$hcfg"
    info "himalaya config escrito em $hcfg"
    warn "Workspace em outro idioma? Ajuste os folder.aliases em $hcfg."
  else
    info "mantendo $hcfg existente"
  fi

  step "Validando e-mail"
  for acc in "${HIMALAYA_ACCOUNTS[@]}"; do
    probe "email:$acc" "himalaya envelope list -a $acc --page-size 1 -o json" "verifique o login/token"
  done
}

# ---------------------------------------------------------------- google ----
setup_google() {
  step "Agenda (gcalcli) + Tarefas (gtasks) via OAuth do Google Cloud"
  have gcalcli || die "gcalcli ausente — rode scripts/install.sh"
  have jq || die "jq ausente — rode scripts/install.sh"

  cat <<'EOF'
    Pré-requisito MANUAL (uma vez, no Google Cloud Console):
      1. Crie/escolha um projeto em https://console.cloud.google.com
      2. Habilite as APIs: "Google Calendar API" e "Google Tasks API"
      3. APIs & Services > Credentials > Create OAuth client ID > tipo "Desktop app"
      4. Baixe o JSON do client.
EOF
  # Reusa o client já salvo, se houver (re-runs não precisam reinformar o caminho).
  local default_secret="$HOME/.config/daily-tui/gtasks-client-secret.json"
  local secret
  if [[ -f "$default_secret" ]]; then
    info "Já existe um client salvo. Enter aceita; ou informe outro caminho."
    secret="$(ask 'Caminho do JSON do OAuth client (Desktop app)' "$default_secret")"
  else
    secret="$(ask 'Caminho do JSON do OAuth client (Desktop app)')"
  fi
  [[ -f "$secret" ]] || die "arquivo não encontrado: $secret"
  local cid csec
  cid="$(jq -r '.installed.client_id // .web.client_id' "$secret")"
  csec="$(jq -r '.installed.client_secret // .web.client_secret' "$secret")"
  [[ -n "$cid" && "$cid" != "null" ]] || die "client_id não encontrado no JSON"

  # gcalcli — auth por conta, com diretório isolado (XDG_DATA_HOME)
  for dir in "${GCAL_DIRS[@]}"; do
    step "gcalcli auth — conta '$dir' (abre o navegador)"
    mkdir -p "$GCAL_ROOT/$dir"
    XDG_DATA_HOME="$GCAL_ROOT/$dir" \
      gcalcli --client-id "$cid" --client-secret "$csec" list \
      || warn "gcalcli auth ($dir) falhou — repita depois"
  done

  # gtasks — guarda o client secret no caminho padrão e autoriza
  step "gtasks — Google Tasks (conta pessoal)"
  local gt_secret="$HOME/.config/daily-tui/gtasks-client-secret.json"
  mkdir -p "$(dirname "$gt_secret")"
  cp "$secret" "$gt_secret"
  info "client secret copiado para $gt_secret"
  gtasks auth || warn "gtasks auth falhou — repita depois"

  step "Validando Google"
  for dir in "${GCAL_DIRS[@]}"; do
    probe "agenda:$dir" "XDG_DATA_HOME=$GCAL_ROOT/$dir gcalcli list" "refaça o auth desta conta"
  done
  probe "tasks (gtasks)" "gtasks list" "refaça: gtasks auth"
}

# -------------------------------------------------------------------- main --
CMD="check"
for a in "$@"; do
  case "$a" in
    --force) FORCE=1 ;;
    check|email|google|all) CMD="$a" ;;
    --help|-h) awk 'NR>1 && /^#/ {sub(/^# ?/,""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "argumento desconhecido: $a" ;;
  esac
done

case "$CMD" in
  check)  doctor ;;
  email)  setup_email ;;
  google) setup_google ;;
  all)    setup_email; echo; setup_google; echo; doctor ;;
esac
