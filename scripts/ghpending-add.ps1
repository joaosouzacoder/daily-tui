# ghpending-add.ps1 - roda o seletor `ghpending add` com o GITHUB_TOKEN do
# 1Password no ambiente (o painel de PRs precisa dele para listar repos privados).
#
# A referencia do token vem de daily-tui.config.ps1.
# Passe os mesmos argumentos do ghpending add, ex.:  ghpending-add.ps1 --all
#                                                     ghpending-add.ps1 --user minha-org
$ErrorActionPreference = 'Stop'

$config = Join-Path $PSScriptRoot 'daily-tui.config.ps1'
if (-not (Test-Path $config)) {
    Write-Error "config ausente: copie daily-tui.config.example.ps1 para daily-tui.config.ps1 e preencha."
    exit 1
}
. $config

$tok = (op read $GithubTokenRef --account $GithubTokenAcct 2>$null)
if ([string]::IsNullOrWhiteSpace($tok)) { Write-Error 'nao consegui ler o GITHUB_TOKEN do 1Password'; exit 1 }
$env:GITHUB_TOKEN = $tok.Trim()
& ghpending add @args
