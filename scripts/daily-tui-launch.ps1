# daily-tui-launch.ps1 - lanca o daily-tui no Windows com os tokens do 1Password.
#
# Configuracao (e-mails + referencias 1Password) vem de daily-tui.config.ps1
# (copie de daily-tui.config.example.ps1). Nada de dado pessoal fica neste arquivo.
#
# Cache: os tokens sao buscados do 1Password e guardados criptografados com DPAPI
# (SecureString via Export-Clixml, atrelado a este usuario+maquina - NAO e texto
# plano). Enquanto o cache estiver fresco (< TTL), as proximas execucoes usam o
# cache e NAO chamam o 1Password (sem prompt de biometria). Para forcar refresh:
# apague o arquivo de cache.
$ErrorActionPreference = 'Stop'

$config = Join-Path $PSScriptRoot 'daily-tui.config.ps1'
if (-not (Test-Path $config)) {
    Write-Error "config ausente: copie daily-tui.config.example.ps1 para daily-tui.config.ps1 e preencha."
    exit 1
}
. $config

$CacheDir  = Join-Path $env:LOCALAPPDATA 'daily-tui'
$CacheFile = Join-Path $CacheDir 'tokens.clixml'
$TtlHours  = 12

function Read-OpSecret($ref, $account) {
    try {
        $v = op read $ref --account $account 2>$null
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($v)) { return $v.Trim() }
    }
    catch { }
    return $null
}

function ConvertFrom-Secure($sec) {
    if (-not $sec) { return $null }
    return [System.Net.NetworkCredential]::new('', $sec).Password
}

function ConvertTo-Secure($plain) {
    if ([string]::IsNullOrWhiteSpace($plain)) { return $null }
    return ConvertTo-SecureString $plain -AsPlainText -Force
}

# --- tenta o cache fresco antes de tocar no 1Password -----------------------
$cache = $null
if (Test-Path $CacheFile) {
    try { $cache = Import-Clixml $CacheFile } catch { $cache = $null }
}
$fresh = $cache -and $cache.Stamp -and ((Get-Date) - $cache.Stamp).TotalHours -lt $TtlHours

$jira = $null
$gh   = $null
if ($fresh) {
    $jira = ConvertFrom-Secure $cache.Jira
    $gh   = ConvertFrom-Secure $cache.Github
}
else {
    $jira = Read-OpSecret $JiraTokenRef   $JiraTokenAcct
    $gh   = Read-OpSecret $GithubTokenRef $GithubTokenAcct
    # Se o 1Password falhar (prompt recusado/offline), cai para o cache antigo.
    if (-not $jira -and $cache) { $jira = ConvertFrom-Secure $cache.Jira }
    if (-not $gh   -and $cache) { $gh   = ConvertFrom-Secure $cache.Github }
    if ($jira -or $gh) {
        New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
        [PSCustomObject]@{
            Stamp  = (Get-Date)
            Jira   = (ConvertTo-Secure $jira)
            Github = (ConvertTo-Secure $gh)
        } | Export-Clixml $CacheFile
    }
}

# --- variaveis de ambiente para os helpers ----------------------------------
$env:DAILY_TUI_WORK_EMAIL     = $WorkEmail
$env:DAILY_TUI_PERSONAL_EMAIL = $PersonalEmail
$env:JIRA_EMAIL = $JiraEmail
$env:JIRA_CLOUD = $JiraCloud
$env:DAILY_TUI_TODO_CLIENT_ID = $TodoClientId
$env:DAILY_TUI_TODO_LIST      = $TodoList
if ($jira) { $env:JIRA_TOKEN = $jira }
if ($gh)   { $env:GITHUB_TOKEN = $gh }

# --- lanca o binario (release) ----------------------------------------------
$exe = Join-Path $PSScriptRoot '..\target\release\daily-tui.exe'
& $exe
