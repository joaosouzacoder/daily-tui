# Ações nos painéis: e-mail, Jira estruturado e subtarefas

**Data:** 2026-08-03
**Status:** aprovado, aguardando plano de implementação

## Objetivo

O daily-tui hoje só lê. Estas seis features o tornam operável sem sair do
terminal: agir sobre um e-mail, navegar do Jira para o navegador, filtrar e
fatiar as issues, e ver subtarefas tanto no Jira quanto no To Do.

## Escopo

**Dentro** — as seis features pedidas:

1. E-mail: marcar lido/não lido, mover para pasta, excluir.
2. Jira: abrir a issue selecionada no navegador.
3. Jira: filtrar entre assignee, reporter e ambas.
4. Jira: menções a mim, na tecla `n`.
5. To Do: ver e marcar as subtarefas de uma tarefa.
6. Jira: aba com as issues agrupadas pelo pai (ver "Revisões guiadas por dado").

**Fora:**

- suporte a mouse (o TUI é teclado; "clicar" nas features virou "selecionar e teclar");
- estado de lido/não-lido de notificações do Jira (ver "Menções");
- criar, editar ou apagar subtarefas — só visualizar e marcar;
- responder e-mail (a skill `email-reply` já cobre isso fora do TUI).

## Decisões registradas

- **O PR #1 (`luanhns`) será ignorado.** Ele implementa a feature 1 (+334/-2 nos
  mesmos cinco arquivos). Foi apontado que reimplementar descarta o trabalho dele
  e gera conflito; o João decidiu reimplementar. Consequência aceita: aquele PR
  ficará conflitado.
- **Uma consulta serve as duas abas.** Pedindo `parent` no `fields`, cada issue já
  traz seu pai; a aba de hierarquia é derivada em memória, sem consulta extra.
- **`Enter` no Jira abre o navegador**, não um overlay de detalhe.

## Revisões guiadas por dado (2026-08-03)

O desenho original foi testado contra o Jira real com o API token do João, antes
de virar plano. Duas features não sobreviveram ao teste e foram redesenhadas:

**A aba de subtarefas nasceria vazia.** As 6 issues abertas atribuídas a ele têm
`subtasks = 0`, todas. Não existe nenhuma issue do tipo subtarefa atribuída a ele
(`issuetype in subTaskIssueTypes() AND assignee = currentUser()` → 0) nem
relatada por ele (→ 0). O que tem estrutura são os **pais**: metade das issues tem
um, e três penduram no mesmo épico. Decisão: a aba inverte e passa a agrupar as
issues **pelo pai** (épico ou iniciativa), com um grupo "sem pai" para as
soltas.

**O filtro de relator inundava o painel.** `reporter = currentUser() AND
statusCategory != Done` bate o teto de 100; só nos últimos 7 dias são 75, sendo
74 de um único projeto. Variantes medidas: sem responsável → 96; fora desse projeto → 29;
**em andamento → 7**. Decisão: o modo relator passa a ser
`statusCategory = 'In Progress'` — o que ele abriu e alguém está de fato mexendo.

Observação factual registrada porque o painel vai mudar à vista: em 31/07 o
painel mostrava 27 issues do projeto DLE; hoje o total de issues abertas
atribuídas a ele é 6, nenhuma em DLE. Foram reatribuídas ou fechadas.

## Arquitetura

O padrão do repo se mantém: cada painel é alimentado por uma CLI externa que
fala JSON com o Rust.

O `jirapending` existe hoje em **duas** implementações — `scripts/jirapending`
(bash + curl) e `jirapending.ps1` (PowerShell) — porque não havia runtime comum.
Cada consulta nova nasceria duplicada, e essa duplicação já custou dois bugs
(o `cp` que abortava o setup e o encoding do stdout). As duas saem, substituídas
por um `scripts/jira` em Python via uv, como o `mstodo` — uma implementação
cobre Windows e Linux.

```
daily-tui (Rust)
  ├── src/data/jira.rs  ──> jira {issues --filter <modo> | mentions}   [JSON]
  ├── src/data/tasks.rs ──> mstodo {list|add|complete|reopen|edit|delete|check|uncheck}
  └── src/data/email.rs ──> himalaya {envelope list|message read|flag|message move}
```

Efeito colateral: o painel de Jira deixa de depender de `src/ansi.rs`, porque as
cores passam a ser aplicadas pelo Rust a partir dos campos estruturados. O
`ansi.rs` continua servindo o `ghpending` no painel de PRs.

## O helper `jira`

Python (uv, PEP 723), dependência única `requests` — o Jira usa Basic auth com
e-mail + API token, sem OAuth. Reaproveita as variáveis que já existem:
`JIRA_EMAIL`, `JIRA_CLOUD`, `JIRA_TOKEN`.

### Subcomandos

| Subcomando | O que faz |
| --- | --- |
| `issues [--filter assignee\|reporter\|both]` | JSON das minhas issues, com o pai de cada uma. Default: `assignee`. |
| `mentions` | JSON das issues onde fui mencionado nos últimos 30 dias. |

### JQL por modo de filtro

Ordenação comum: `ORDER BY project ASC, updated DESC`. Os três modos têm
condições de status **diferentes**, e isso é deliberado — ver "Revisões guiadas
por dado".

| Modo | Cláusula | Volume medido em 2026-08-03 |
| --- | --- | --- |
| `assignee` | `assignee = currentUser() AND statusCategory != Done` | 6 |
| `reporter` | `reporter = currentUser() AND statusCategory = 'In Progress'` | 7 |
| `both` | `(assignee = currentUser() AND statusCategory != Done) OR (reporter = currentUser() AND statusCategory = 'In Progress')` | ~13 |

O modo `reporter` não usa `statusCategory != Done` como os outros porque essa
combinação devolve mais de 100 issues, quase todas ruído de um projeto só.

`JIRA_JQL`, se definida, **substitui a consulta inteira** e o `--filter` passa a
não ter efeito — precedência documentada, para não quebrar quem já usa a
variável. Nesse caso a tecla de filtro no painel não muda nada, e o cabeçalho
mostra `jql` em vez do nome do modo.

### Menções

Não existe JQL de "notificação não lida": o JQL busca conteúdo, não o estado da
campainha. Verificado contra o Jira real (2026-08-03), esta consulta funciona e
devolveu 7 issues:

```
(comment ~ "<accountId>" OR description ~ "<accountId>") AND updated >= -30d
ORDER BY updated DESC
```

O `accountId` vem de `GET /rest/api/3/myself` — uma chamada, feita só no
`mentions`. Consequência aceita: a visão de menções é "onde me mencionaram
recentemente", sem noção de novo desde a última visita. Lido/não-lido só existe
na API interna do feed, não documentada pela Atlassian, e foi descartada.

### Contrato JSON

Idêntico entre `issues` e `mentions` — o mesmo parser serve as duas:

```json
[{"key":"ENG-101","summary":"[Painel] - Melhorias no dashboard","status":"Em andamento",
  "project":"ENG","url":"https://example.atlassian.net/browse/ENG-101",
  "parent":{"key":"ENG-42","summary":"Engenharia de Plataforma"}}]
```

`url` é montada pelo helper (`https://<JIRA_CLOUD>/browse/<key>`) e não pelo
Rust, para o painel não precisar conhecer o domínio. `parent` é `null` quando a
issue não tem pai. Em `mentions`, `parent` vem preenchido quando existir — o
parser é o mesmo, mesmo que a visão de menções não agrupe por pai.

Paginação: `maxResults=100` seguindo `nextPageToken` do endpoint
`POST /rest/api/3/search/jql`, o mesmo que o `jirapending` já usa.

### Erros

Uma linha no stderr, como os outros helpers: `defina JIRA_EMAIL`,
`Jira <status>: <mensagem do campo errorMessages>`.

## O painel de Jira

`Vec<String>` dá lugar a itens estruturados:

```rust
pub struct JiraItem {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub project: String,
    pub url: String,
    /// Épico ou iniciativa acima desta issue; `None` quando é solta.
    pub parent: Option<JiraParent>,
}

pub struct JiraParent {
    pub key: String,
    pub summary: String,
}
```

Três visões, e trocar entre `issues` e `por pai` não custa consulta nenhuma —
é o mesmo conjunto reagrupado em memória:

| Visão | Conteúdo | Agrupamento |
| --- | --- | --- |
| `issues` (default) | as issues do filtro atual | por projeto, como hoje |
| `por pai` | as mesmas issues do filtro atual | pelo pai, com um grupo "sem pai" no fim |
| `menções` | resultado de `jira mentions` | por projeto |

A visão de menções tem seus próprios dados, então trocar para ela dispara uma
busca; a primeira vez custa uma chamada, e o resultado fica em memória até o
próximo refresh.

Cabeçalho: `JIRA · minhas · [issues] por-pai menções`, com a visão ativa entre
colchetes e o modo de filtro em texto (`minhas`, `relator`, `ambas`, `jql`).

**O que o cursor seleciona.** Em todas as três visões as linhas de grupo — nome
de projeto ou `<CHAVE> <resumo>` do pai — são cabeçalhos, e o cursor só para nas
issues. `Enter` abre a URL da issue sob o cursor. A visão `por pai` não muda o
conjunto de issues, só o agrupamento: nenhuma issue desaparece ao trocar de
visão, o que torna a troca previsível.

## Teclas

Todas com escopo de painel — a mesma tecla pode significar coisas diferentes em
painéis diferentes, como `d` já faz hoje.

| Painel | Tecla | Ação |
| --- | --- | --- |
| Jira | `Enter` | abre `url` da issue selecionada no navegador |
| Jira | `f` | circula o filtro: minhas → relator → ambas |
| Jira | `p` | visão por pai |
| Jira | `n` | visão menções |
| Jira | `Esc` | volta para a visão issues |
| E-mail | `Space` | marca lido / não lido |
| E-mail | `m` | mover: abre a lista de pastas |
| E-mail | `d` | excluir: move para a Lixeira, com confirmação |
| Tarefas | `Enter` | expande / recolhe as subtarefas da tarefa |
| Tarefas | `Space` | marca a linha sob o cursor — tarefa ou subtarefa |

`Enter` hoje abre o detalhe do e-mail e não faz nada nos outros painéis; passa a
ter significado por painel, mantendo o comportamento atual no de E-mail.

### Abrir no navegador

Sem dependência nova: `cmd /C start "" <url>` no Windows, `xdg-open` no Unix.
A URL de uma issue (`https://<cloud>/browse/ENG-101`) não contém `&`, então não
sofre o truncamento do `cmd` que quebrou o fluxo OAuth do himalaya em 31/07 — o
`start ""` com a URL entre aspas é a forma segura de qualquer modo.

## To Do com subtarefas

No To Do, subtarefa é `checklistItem` (o que o app chama de "etapa").
Confirmado nos dados reais em 2026-08-03: 3 das 8 tarefas têm subtarefas, uma
delas com 14 itens, com estado de concluído.

`mstodo list` passa a pedir `$expand=checklistItems` — a **mesma** chamada, sem
custo por tarefa. O contrato ganha um campo:

```json
[{"id":"AAMk...","title":"...","completed":false,"due":"","notes":"",
  "subtasks":[{"id":"abc-123","title":"Medir a fiação","completed":false}]}]
```

Dois subcomandos novos, espelhando `complete`/`reopen`:

| Subcomando | Chamada |
| --- | --- |
| `check <tarefa> <item>` | `PATCH /me/todo/lists/{lid}/tasks/{tid}/checklistItems/{cid}` `{"isChecked":true}` |
| `uncheck <tarefa> <item>` | idem com `false` |

`TaskItem` ganha `subtasks: Vec<SubTask>`. O painel mantém o conjunto de tarefas
expandidas; quando expandida, as subtarefas aparecem indentadas abaixo.

**Como o cursor lida com isso.** Hoje o cursor indexa tarefas. Passa a indexar
**linhas**, e uma linha é ou uma tarefa ou uma subtarefa — a lista renderizada é
o achatamento das tarefas com as subtarefas das expandidas intercaladas. Expandir
ou recolher muda quantas linhas existem, então o cursor precisa ser reancorado na
mesma tarefa após a operação, e não no mesmo índice. Isso vale também para a
lógica de scroll que `ui.rs` já tem: ela opera sobre a contagem de linhas.

`Enter` numa tarefa sem subtarefas não faz nada — nem expande vazio, nem pisca.
`Space` age sobre a linha sob o cursor, escolhendo entre `complete`/`reopen`
(tarefa) e `check`/`uncheck` (subtarefa).

## E-mail com ações

Sintaxe verificada no himalaya 1.2.0 instalado:

| Ação | Comando |
| --- | --- |
| marcar lido | `himalaya flag add <id> seen -a <conta>` |
| marcar não lido | `himalaya flag remove <id> seen -a <conta>` |
| | (o `Space` alterna: escolhe `add` ou `remove` pelo campo `unread` do envelope sob o cursor) |
| mover | `himalaya message move <pasta> <id> -a <conta>` |
| excluir | `himalaya message move trash <id> -a <conta>` |

Excluir é mover para `trash` — o alias que a config de cada conta já define
(`[Gmail]/Lixeira` na work, `[Gmail]/Trash` na personal). Escolhido em vez de
`message delete` porque o efeito é explícito e recuperável.

O `m` abre uma lista com as pastas que a config declara: `inbox`, `sent`,
`drafts`, `trash`, `spam`, `all`. O `d` pede confirmação, reusando o prompt de
confirmação que o painel de Tarefas já tem.

Cada escrita segue o padrão existente do painel de Tarefas: executa e re-busca a
lista, para o painel refletir o servidor e não um palpite local.

## Erros

Sem mudança de padrão: uma linha por painel, via `stderr_summary`. As ações de
escrita que falharem mostram o erro no painel e deixam a lista como estava —
nenhuma atualização otimista.

## Testes

- **Funções puras primeiro:** parse do JSON do `jira` e do `mstodo`, derivação
  das três visões (agrupar por projeto, agrupar por pai, lista vazia), montagem
  da JQL por modo de filtro, e a decisão de qual comando o `Space` dispara dada a
  linha sob o cursor.
- **Teclas** no estilo que `app.rs` já usa (`app.update(key(...))`), cobrindo o
  escopo por painel: `d` no E-mail não apaga tarefa, `f` fora do Jira não faz
  nada.
- **Fixtures derivadas de saída real** do `scripts/jira` e do `mstodo list` —
  não JSON escrito à mão. Convenção do projeto.
- **Render** estendendo os testes de `ui.rs` que já existem, incluindo o
  cabeçalho com filtro e visão ativa e as subtarefas indentadas.
- Sem infra de teste nova. O helper Python continua sem pytest, verificado por
  smoke test ao vivo dos subcomandos, como o `mstodo`.

## Fases

Cada uma entrega algo funcionando por si:

1. **Helper `jira`** + contrato JSON; saem os dois `jirapending`. Verificável por
   `jira issues --filter both | python -m json.tool`.
2. **Painel estruturado:** visão issues, filtro `f`, `Enter` abrindo o navegador.
3. **Visões por-pai e menções**, com o cabeçalho de três estados.
4. **Subtarefas do To Do:** `$expand`, `check`/`uncheck`, expandir e marcar.
5. **Ações de e-mail:** lido, mover, excluir.

A fase 1 é pré-requisito de 2 e 3. As fases 4 e 5 são independentes de todas as
outras.

## Riscos

- **Divergência de contagem no Jira — investigada e fechada.** A suspeita era
  escopo de OAuth do MCP; não era: o API token devolve os mesmos 6. As 27 issues
  de DLE que o painel mostrava em 31/07 deixaram de estar atribuídas a ele.
  Nenhuma ação no código; registrado para não ser reaberto.
- **O volume dos filtros pode mudar.** Os números que justificam a semântica do
  modo relator (7 em andamento contra 100+ abertas) são de 2026-08-03. Se o fluxo
  de trabalho dele mudar, a escolha merece ser revisitada — não é uma verdade
  permanente, é uma calibragem.
- **`comment ~` é busca textual.** Depende do índice do Jira e pode atrasar
  alguns segundos para menções recém-criadas. Aceito: a alternativa é a API
  interna do feed.
- **Mover e-mail no Gmail é etiqueta, não pasta.** Mover para `sent` ou `all`
  pode não se comportar como um "mover" clássico. A lista oferece o que a config
  declara; se algum alias se mostrar inútil na prática, sai da lista.
