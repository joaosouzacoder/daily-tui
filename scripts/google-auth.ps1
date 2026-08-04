# google-auth.ps1 - autentica as CLIs Google do daily-tui via OAuth (abre o
# navegador). ASCII puro de proposito: o Windows PowerShell 5.1 le .ps1 como ANSI.
#
# No Windows o gcalcli guarda o token OAuth num unico caminho fixo (o platformdirs
# ignora env vars), entao NAO da para isolar contas por diretorio. Aqui cada conta
# e autenticada e o token e movido para um arquivo por conta; o daily-tui troca o
# token ativo antes de cada consulta (ver src/data/agenda.rs).
#
# Os e-mails de cada conta vem de daily-tui.config.ps1.
#
# Uso:  google-auth.ps1              # work + personal (agenda) + tarefas
#       google-auth.ps1 personal     # so a(s) conta(s) indicada(s): work|personal
#
# O passo das tarefas (Microsoft To Do) roda sempre no fim, e e pulado quando ja
# existe token - veja o comentario da secao no rodape.
param([string[]]$Accounts = @('work', 'personal'))
$ErrorActionPreference = 'Continue'
# Sem isto o Python segura a URL de login no buffer (stdout nao e um TTY) e ela
# nunca aparece, porque o processo trava esperando o consentimento.
$env:PYTHONUNBUFFERED = '1'

$config = Join-Path $PSScriptRoot 'daily-tui.config.ps1'
if (-not (Test-Path $config)) {
    Write-Error "config ausente: copie daily-tui.config.example.ps1 para daily-tui.config.ps1 e preencha."
    exit 1
}
. $config
$emails = @{ work = $WorkEmail; personal = $PersonalEmail }

$secretDir = "$env:USERPROFILE\.config\daily-tui"
$secret = "$secretDir\google-client-secret.json"
if (-not (Test-Path $secret)) { $secret = "$secretDir\gtasks-client-secret.json" }
if (-not (Test-Path $secret)) {
    Write-Error "client secret nao encontrado em $secretDir (google-client-secret.json)"
    exit 1
}
$c = (Get-Content $secret -Raw | ConvertFrom-Json).installed

$canonical = Join-Path $env:LOCALAPPDATA 'gcalcli\gcalcli\oauth'

foreach ($acc in $Accounts) {
    Write-Host ""
    Write-Host ">> Agenda [$acc] - faca login no navegador com: $($emails[$acc])" -ForegroundColor Green
    Write-Host "   (o gcalcli vai imprimir uma URL; abra no navegador e autorize)" -ForegroundColor DarkGray
    # Remove o token ativo para forcar um novo consentimento nesta conta.
    if (Test-Path $canonical) { Remove-Item $canonical -Force }
    & gcalcli --client-id $c.client_id --client-secret $c.client_secret agenda
    if (-not (Test-Path $canonical)) {
        Write-Warning "login [$acc] nao gerou token; refaca este passo."
        continue
    }
    $dst = Join-Path $env:LOCALAPPDATA ("gcalcli-accounts\" + $acc + "\oauth")
    New-Item -ItemType Directory -Force -Path (Split-Path $dst) | Out-Null
    Move-Item $canonical $dst -Force
    Write-Host "   token [$acc] salvo em $dst" -ForegroundColor DarkGray
}

# --- Tarefas: Microsoft To Do (mstodo) ---------------------------------------
# Nao e auth do Google. Mora aqui porque este script e o unico ponto de entrada
# de autenticacao interativa que o Windows tem: o setup-auth.sh e Linux/macOS.
# O client id vem do daily-tui.config.ps1, igual ao daily-tui-launch.ps1.
# (O PYTHONUNBUFFERED do topo tambem serve aqui: sem ele o codigo do device flow
# fica preso no buffer do Python enquanto o processo espera o consentimento.)
$todoToken = Join-Path $env:USERPROFILE '.local\share\daily-tui\mstodo-personal.json'
Write-Host ""
if (Test-Path $todoToken) {
    Write-Host ">> Tarefas [mstodo] - token ja existe, nada a fazer." -ForegroundColor DarkGray
    Write-Host "   (para refazer o consentimento, apague $todoToken)" -ForegroundColor DarkGray
}
elseif ([string]::IsNullOrWhiteSpace($TodoClientId)) {
    Write-Warning "TodoClientId vazio em daily-tui.config.ps1 - painel de tarefas fica sem token."
}
else {
    Write-Host ">> Tarefas [mstodo] - device code: abra a URL exibida e digite o codigo" -ForegroundColor Green
    $env:DAILY_TUI_TODO_CLIENT_ID = $TodoClientId
    & mstodo auth
    if (-not (Test-Path $todoToken)) {
        Write-Warning "mstodo auth nao gerou token; refaca este passo."
    }
}

Write-Host ""
Write-Host "Pronto. Feche esta janela e volte ao daily-tui." -ForegroundColor Cyan
