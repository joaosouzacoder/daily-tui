#!/usr/bin/env bash
# setup-auth.sh — configura e valida TODAS as autenticações do daily-tui.
#
# A parte chata do daily-tui é autenticar as CLIs. Este script automatiza o que
# dá e te diz exatamente o que falta no resto.
#
# Subcomandos:
#   check          (default) — testa cada painel e mostra PASS/FAIL. NÃO altera nada.
#   email          — configura ortie + himalaya (Gmail/Workspace via OAuth2)
#   google         — configura gcalcli (agenda) via OAuth do GCP
#   mstodo         — configura mstodo (tarefas) via Microsoft To Do
#   all            — email, depois google, depois mstodo, depois check
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
  for c in himalaya gcalcli ghpending jira mstodo ortie secret-tool jq; do
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
  probe "jira (jira)" "jira issues" \
    "defina JIRA_EMAIL / JIRA_CLOUD / JIRA_TOKEN"
  probe "tasks (mstodo)" "mstodo list" \
    "rode: scripts/setup-auth.sh mstodo"
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
  step "Agenda (gcalcli) via OAuth do Google Cloud"
  have gcalcli || die "gcalcli ausente — rode scripts/install.sh"
  have jq || die "jq ausente — rode scripts/install.sh"

  cat <<'EOF'
    Pré-requisito MANUAL (uma vez, no Google Cloud Console):
      1. Crie/escolha um projeto em https://console.cloud.google.com
      2. Habilite a API: "Google Calendar API"
      3. APIs & Services > Credentials > Create OAuth client ID > tipo "Desktop app"
      4. Baixe o JSON do client.
EOF
  # Client OAuth compartilhado da agenda (gcalcli). O nome antigo
  # (gtasks-client-secret.json) é aceito para não quebrar instalações existentes.
  local secret_dir="$HOME/.config/daily-tui"
  local default_secret="$secret_dir/google-client-secret.json"
  [[ -f "$default_secret" ]] || [[ ! -f "$secret_dir/gtasks-client-secret.json" ]] \
    || default_secret="$secret_dir/gtasks-client-secret.json"
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

  # Guarda o client no caminho padrão, para o google-auth.ps1 e re-runs.
  mkdir -p "$secret_dir"
  [[ "$secret" -ef "$default_secret" ]] || cp "$secret" "$default_secret"
  info "client OAuth salvo em $default_secret"

  step "Validando Google"
  for dir in "${GCAL_DIRS[@]}"; do
    probe "agenda:$dir" "XDG_DATA_HOME=$GCAL_ROOT/$dir gcalcli list" "refaça o auth desta conta"
  done
}

# ---------------------------------------------------------------- mstodo ----
# setup_mstodo [tolerant] — em `all`, "tolerant" faz a falta do client id só
# avisar: com `set -e` um die() aqui mataria o script antes do doctor. Chamado
# direto (`setup-auth.sh mstodo`), a variável ausente continua sendo erro.
setup_mstodo() {
  local tolerant="${1:-}"
  step "Tarefas (mstodo) — Microsoft To Do, conta pessoal"
  have mstodo || die "mstodo ausente — rode scripts/install.sh"
  if [[ -z "${DAILY_TUI_TODO_CLIENT_ID:-}" ]]; then
    local msg="defina DAILY_TUI_TODO_CLIENT_ID (veja scripts/daily-tui.env.example)"
    [[ "$tolerant" == "tolerant" ]] || die "$msg"
    warn "$msg — pulando as tarefas"
    return 0
  fi

  cat <<'EOF'
    O client padrão do daily-tui.env.example é o client público first-party da
    Microsoft e não exige nenhum cadastro. O passo a seguir abre o device code:
    você digita o código exibido em https://www.microsoft.com/link.

    Plano B (só se o escopo Tasks.ReadWrite for restringido nesse client):
      registre um app no portal Entra — Personal Microsoft accounts only,
      "Allow public client flows: Yes", permissão delegada Tasks.ReadWrite —
      e troque DAILY_TUI_TODO_CLIENT_ID pelo Application (client) ID dele.
EOF
  mstodo auth || warn "mstodo auth falhou — repita depois"

  step "Validando tarefas"
  probe "tasks (mstodo)" "mstodo list" "refaça: mstodo auth"
}

# -------------------------------------------------------------------- main --
CMD="check"
for a in "$@"; do
  case "$a" in
    --force) FORCE=1 ;;
    check|email|google|mstodo|all) CMD="$a" ;;
    --help|-h) awk 'NR>1 && /^#/ {sub(/^# ?/,""); print; next} NR>1 {exit}' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) die "argumento desconhecido: $a" ;;
  esac
done

case "$CMD" in
  check)  doctor ;;
  email)  setup_email ;;
  google) setup_google ;;
  mstodo) setup_mstodo ;;
  all)    setup_email; echo; setup_google; echo; setup_mstodo tolerant; echo; doctor ;;
esac
