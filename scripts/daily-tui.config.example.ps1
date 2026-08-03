# daily-tui.config.example.ps1 - modelo de configuracao do launcher Windows.
#
# Copie para `daily-tui.config.ps1` (mesma pasta) e preencha com os SEUS valores.
# O arquivo `daily-tui.config.ps1` fica no .gitignore (nao vai para o repo).
#
# Usado por: daily-tui-launch.ps1, google-auth.ps1, ghpending-add.ps1.

# --- Agenda: e-mail primario de cada conta Google (filtro --calendar) --------
$WorkEmail     = 'voce-work@suaempresa.com'
$PersonalEmail = 'voce@gmail.com'

# --- Jira --------------------------------------------------------------------
$JiraEmail = 'voce-work@suaempresa.com'
$JiraCloud = 'suaempresa.atlassian.net'   # com ou sem https://

# --- Microsoft To Do (tarefas) -----------------------------------------------
# Client publico first-party da Microsoft ("Microsoft Graph Command Line Tools"),
# autorizado por device code - nao exige app registration proprio.
$TodoClientId = '14d82eec-204b-4c2f-b7e8-296a70dab67e'
# Nome da lista do To Do; vazio = lista padrao ("Tarefas").
$TodoList     = ''

# --- Tokens via 1Password (referencia op://vault/item/campo + conta) ---------
# O launcher le esses segredos com `op read` e guarda em cache DPAPI local.
$JiraTokenRef    = 'op://Vault/Token Jira/credential'
$JiraTokenAcct   = 'suaempresa.1password.com'
$GithubTokenRef  = 'op://Vault/Token GitHub/password'
$GithubTokenAcct = 'my.1password.com'
