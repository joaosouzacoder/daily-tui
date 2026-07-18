# jirapending.ps1 — lista os tickets do Jira atribuídos a você e ainda abertos,
# agrupados por projeto e coloridos (ANSI), para o painel do daily-tui.
#
# Porta nativa Windows do helper bash `scripts/jirapending` (troca curl/jq pelo
# Invoke-RestMethod). Emite exatamente os mesmos escapes ANSI que o painel Rust
# (src/data/jira.rs + src/ansi.rs) espera.
#
# Configuração via variáveis de ambiente (veja scripts/daily-tui.env.example):
#   JIRA_EMAIL — seu e-mail Atlassian                     (obrigatório)
#   JIRA_CLOUD — domínio da instância, ex.: empresa.atlassian.net (obrigatório)
#   JIRA_TOKEN — API token do Jira                        (obrigatório)
#   JIRA_JQL   — sobrescreve a query JQL padrão            (opcional)
$ErrorActionPreference = 'Stop'
# O daily-tui lê o stdout como UTF-8; garante acentos corretos nos summaries.
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false

function Need($name) {
    $v = [Environment]::GetEnvironmentVariable($name)
    if ([string]::IsNullOrWhiteSpace($v)) {
        [Console]::Error.WriteLine("defina $name")
        exit 1
    }
    return $v
}

$email = Need 'JIRA_EMAIL'
$cloud = Need 'JIRA_CLOUD'
$token = Need 'JIRA_TOKEN'
# Aceita JIRA_CLOUD com ou sem esquema/barra (ex.: "https://x.atlassian.net/").
$cloud = $cloud -replace '^https?://', '' -replace '/+$', ''
$jql = $env:JIRA_JQL
if ([string]::IsNullOrWhiteSpace($jql)) {
    $jql = 'assignee = currentUser() AND statusCategory != Done ORDER BY project ASC, updated DESC'
}

$auth = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("${email}:${token}"))
$body = @{ jql = $jql; fields = @('summary', 'status', 'project'); maxResults = 100 } | ConvertTo-Json

$resp = Invoke-RestMethod -Method Post -Uri "https://$cloud/rest/api/3/search/jql" `
    -Headers @{ Authorization = "Basic $auth" } `
    -ContentType 'application/json' -Body $body

$e = [char]27
$groups = @($resp.issues) | Group-Object { $_.fields.project.key } | Sort-Object Name
foreach ($g in $groups) {
    Write-Output "$e[36;1m$($g.Name) ($($g.Count))$e[0m"
    foreach ($i in $g.Group) {
        Write-Output "  $e[33m$($i.key)$e[0m $e[90m[$($i.fields.status.name)]$e[0m $($i.fields.summary)"
    }
}
