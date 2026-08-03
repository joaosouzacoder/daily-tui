# Trocar o Google Tasks pelo Microsoft To Do

**Data:** 2026-07-31
**Status:** aprovado, aguardando plano de implementação

## Objetivo

O painel de tarefas do daily-tui hoje lê o Google Tasks pelo helper `gtasks`.
Passa a ler o Microsoft To Do (conta pessoal `@outlook.com`) pela Graph API. O
comportamento visível na TUI não muda: mesma lista, mesmas teclas, mesmas ações.

## Escopo

**Dentro:**

- novo helper `scripts/mstodo` (Python via uv, PEP 723) + shim `scripts/mstodo.cmd`;
- `src/data/tasks.rs` passa a chamar `mstodo`;
- remoção completa do `gtasks` do repo, dos scripts de setup e da documentação;
- duas variáveis de configuração novas (client id e nome da lista).

**Fora:**

- agregar mais de uma conta Microsoft (só a pessoal);
- agregar mais de uma lista do To Do;
- mudar o painel, as teclas, o `TaskItem` ou o fluxo `msg.rs`/`worker.rs`;
- a agenda, que continua no Google via `gcalcli`.

## Arquitetura

A troca respeita o padrão do repo: cada integração é uma CLI externa que fala
JSON ou texto com o Rust. O helper novo é Python porque, como o `gtasks`, uma
única implementação cobre Windows e Linux — foi o `jirapending` que precisou de
duas portas (bash e PowerShell) por não ter esse runtime.

```
daily-tui (Rust)
  └── src/data/tasks.rs ──> mstodo {list|add|complete|reopen|edit|delete}
                              ├── msal      -> token (device code + refresh)
                              └── requests  -> Microsoft Graph /me/todo/...
```

`tasks.rs` mantém `TaskItem`, `parse_tasks` e as seis funções de ação. Muda o
nome do programa em `run()`, as mensagens de erro e os comentários do módulo.

O `force_utf8_stdout` introduzido em 2026-07-31 (`PYTHONIOENCODING=utf-8`) já
cobre o helper novo: ele também é Python e também serializa com
`ensure_ascii=False`, então sem isso títulos acentuados chegariam corrompidos.

## Contrato JSON

Idêntico ao de hoje — é o que permite manter `parse_tasks` e seus testes:

```json
[{"id":"AAMk...","title":"Comprar café","completed":false,"due":"2026-08-03","notes":"obs"}]
```

Mapeamento a partir do recurso `todoTask` do Graph:

| Campo JSON  | Origem no Graph                        | Observação                              |
| ----------- | -------------------------------------- | --------------------------------------- |
| `id`        | `id`                                   | muda se a tarefa trocar de lista        |
| `title`     | `title`                                | `.strip()`                              |
| `completed` | `status == "completed"`                | valores possíveis: `notStarted`, `inProgress`, `completed`, `waitingOnOthers`, `deferred` |
| `due`       | `dueDateTime.dateTime[:10]`            | `dueDateTime` é objeto `{dateTime, timeZone}`; ausente → `""` |
| `notes`     | `body.content`                         | `.strip()`; ausente → `""`              |

**Ordenação** (o Graph não tem equivalente ao `position` do Google Tasks):
pendentes primeiro, depois vencimento crescente com "sem data" no fim, depois
`createdDateTime` crescente.

O `list` devolve **também as tarefas concluídas** — sem `$filter` na consulta —
porque é o comportamento de hoje: o painel mostra o checkbox marcado e joga as
concluídas para o fim. Como `completed` vem de `status`, o `completedDateTime`
que o Graph mantém depois de um `reopen` é irrelevante para o contrato.

## Subcomandos e chamadas

Base: `https://graph.microsoft.com/v1.0`.

| Subcomando          | Chamada                                                  |
| ------------------- | -------------------------------------------------------- |
| `auth`              | device code flow (MSAL), grava o cache de token          |
| `list`              | `GET /me/todo/lists/{listId}/tasks?$top=100`, seguindo `@odata.nextLink` |
| `add "<título>"`    | `POST /me/todo/lists/{listId}/tasks` `{"title": ...}`    |
| `complete <id>`     | `PATCH .../tasks/{id}` `{"status":"completed"}`           |
| `reopen <id>`       | `PATCH .../tasks/{id}` `{"status":"notStarted"}`          |
| `edit <id> "<t>"`   | `PATCH .../tasks/{id}` `{"title": ...}`                   |
| `delete <id>`       | `DELETE .../tasks/{id}`                                  |

Só o `list` escreve no stdout. As escritas são silenciosas em caso de sucesso,
como no `gtasks` — o Rust re-busca a lista depois de cada ação.

## Resolução da lista

`DAILY_TUI_TODO_LIST` guarda o **nome** da lista (ex.: `Trabalho`). Vazio ou
ausente significa a lista padrão do To Do, que o Graph marca com
`wellknownListName: defaultList`.

Resolver nome → id custa um `GET /me/todo/lists` por invocação, e o painel
recarrega periodicamente. Para evitar essa chamada extra, o id resolvido fica
num arquivo sidecar `<home>/.local/share/daily-tui/mstodo-list.json`
(`{"name": "...", "id": "..."}`) — sidecar, e não dentro do cache do MSAL, que
tem formato próprio e não deve ser misturado. O cache é refeito quando o nome
configurado difere do gravado ou quando o id devolve 404.

## Autenticação

Conta pessoal Microsoft, sem client secret: public client com device code.

**Decisão revista em 2026-08-03.** O plano original pedia um app registration
próprio. Na prática o login no portal Entra falhou com `AADSTS500121` — desafio
de MFA recusado por política de *tenant*, porque o portal autentica a conta
pessoal contra um diretório no qual ela é convidada. Recuperar esse acesso é
problema da conta, não do projeto.

O device code flow autentica no endpoint de consumidor e não passa por tenant
nenhum. Então o client em uso é o **first-party público da Microsoft**
`14d82eec-204b-4c2f-b7e8-296a70dab67e` ("Microsoft Graph Command Line Tools") —
o mesmo arranjo que o himalaya já usa neste projeto com o client público do
Thunderbird para o Gmail.

O trade-off, explícito: o client não é nosso. Se a Microsoft restringir
`Tasks.ReadWrite` nele, o painel para e o caminho passa a ser um registro
próprio. Nada no código depende de qual dos dois foi usado — o client id sempre
vem de `DAILY_TUI_TODO_CLIENT_ID` — mas trocar de client invalida o cache de
token e exige rodar `mstodo auth` de novo.

O roteiro do portal fica registrado como plano B:

1. *App registrations* → **New registration**;
2. *Supported account types*: **Personal Microsoft accounts only**;
3. *Authentication* → **Allow public client flows: Yes** (necessário para device code);
4. *API permissions* → Microsoft Graph → **Delegated** → **Tasks.ReadWrite** → *Grant consent* (só para você);
5. copiar o **Application (client) ID** para `DAILY_TUI_TODO_CLIENT_ID`.

- **Authority:** `https://login.microsoftonline.com/consumers`
- **Escopo:** `Tasks.ReadWrite` (o MSAL acrescenta `offline_access`, `openid`, `profile`)
- **Fluxo:** `initiate_device_flow` → imprime o código para digitar em `microsoft.com/devicelogin` → `acquire_token_by_device_flow`
- **Renovação:** `acquire_token_silent` usa o refresh token do cache
- **Cache:** `msal.SerializableTokenCache` em `<home>/.local/share/daily-tui/mstodo-personal.json`, `chmod 0600` no Unix

Os dois caminhos usam `Path.home()`, a mesma convenção do `gtasks` — nos dois
sistemas, inclusive no Windows (`%USERPROFILE%\.local\share\daily-tui\`), como o
`google-auth.ps1` já pressupõe hoje.

Device code em vez de redirect para `localhost` por decisão deliberada: o
incidente de hoje com o himalaya mostrou que o fluxo de redirect no Windows
depende de o navegador receber a URL inteira, o que falha quando quem abre é o
`cmd`. O device code não tem essa dependência.

Quando não há credencial válida, o helper sai com código diferente de zero e
uma linha no stderr: `sem credenciais — rode: mstodo auth`.

## Configuração

| Variável                    | Obrigatória | Significado                                        |
| --------------------------- | ----------- | -------------------------------------------------- |
| `DAILY_TUI_TODO_CLIENT_ID`  | sim         | Application (client) ID do app registration        |
| `DAILY_TUI_TODO_LIST`       | não         | nome da lista; vazio = lista padrão do To Do       |
| `MSTODO_TOKEN`              | não         | sobrescreve o caminho do cache de token            |

Documentadas em `scripts/daily-tui.env.example` (Linux) e exportadas pelo
`scripts/daily-tui-launch.ps1` a partir do `daily-tui.config.ps1` (Windows),
seguindo o que já é feito com `JIRA_*` e `DAILY_TUI_*_EMAIL`.

## Erros

Toda falha é uma linha só no stderr, sem traceback — o painel exibe o resumo
produzido por `stderr_summary`:

- `sem credenciais — rode: mstodo auth`
- `defina DAILY_TUI_TODO_CLIENT_ID`
- `lista '<nome>' não encontrada`
- `Graph <status>: <mensagem do campo error.message>`

## Testes e verificação

- Os testes de `parse_tasks` em `tasks.rs` continuam válidos, porque o contrato
  JSON não muda. Acrescento **um caso derivado da saída real** de `mstodo list`
  — capturada da conta de verdade, não escrita à mão.
- O helper é verificado por smoke test ao vivo dos sete subcomandos, na ordem
  `auth → add → list → complete → list → reopen → edit → delete → list`,
  conferindo o efeito de cada um na saída do `list`.
- Não haverá teste unitário Python: o repo não tem infra de pytest, o `gtasks`
  também não tinha, e montá-la para um script de ~150 linhas é escopo além do
  pedido. O mapeamento fica numa função pura para permitir isso depois.
- `cargo test` e `cargo clippy --all-targets` limpos antes de concluir.

## Remoção do Google Tasks

| Arquivo                          | Ação                                                        |
| -------------------------------- | ----------------------------------------------------------- |
| `scripts/gtasks`, `gtasks.cmd`   | apagar                                                      |
| `scripts/install.sh`             | instalar `mstodo` no lugar do `gtasks`                      |
| `scripts/setup-auth.sh`          | o alvo `google` passa a configurar só o gcalcli; novo trecho para o `mstodo`; tirar do preflight e dos probes |
| `scripts/google-auth.ps1`        | remover o trecho de tarefas (fica só a agenda)              |
| client OAuth do Google           | renomear `~/.config/daily-tui/gtasks-client-secret.json` → `google-client-secret.json`, com fallback para o nome antigo |
| `scripts/daily-tui.env.example`  | trocar a seção `GTASKS_*` pelas variáveis novas             |
| `README.md`                      | feature list, tabela de helpers, seção de setup, tabela de troubleshooting; sai a menção à "Google Tasks API" |
| `src/data/tasks.rs`              | nome do programa, mensagens, comentários                    |
| `src/data/mod.rs`                | comentários de `helper_command` e `force_utf8_stdout`        |
| `src/worker.rs`, `src/msg.rs`    | comentários que dizem "Google Tasks"                        |

O nome do módulo `tasks.rs` e o tipo `TaskItem` continuam — são genéricos e
renomear não serve a nada aqui.

## Riscos

- **App registration bloqueado.** Materializou-se: o portal Entra recusou o login
  da conta pessoal (`AADSTS500121`). Contornado com o client público
  first-party; ver "Autenticação".
- **Client público restringido pela Microsoft.** É de terceiro, não nosso. Se o
  escopo `Tasks.ReadWrite` deixar de ser consentível nele, o painel para. Saída:
  registrar um app próprio (plano B documentado) e rodar `mstodo auth` de novo.
- **Nome de lista com acento ou renomeado.** O cache sidecar detecta divergência
  pelo nome e re-resolve; se o nome não existir mais, o erro diz qual lista faltou.
- **Limite de requisições do Graph.** O painel recarrega periodicamente e cada
  ciclo faz uma chamada (duas na primeira, para resolver a lista). Longe de
  qualquer throttling relevante.
