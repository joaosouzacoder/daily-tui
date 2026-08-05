# daily-tui

Painel TUI para deixar sempre rodando num monitor, com o que importa no dia a dia:

- 🕐 **Relógio** em tempo real (`HH:MM:SS` + data por extenso em pt-BR).
- 📧 **E-mails** agregados das contas *work* + *personal* (via `himalaya`).
- 📅 **Agenda** dos próximos 7 dias, agregada das duas contas Google (via `gcalcli`).
- 🔀 **PRs/issues** pendentes nos repos monitorados (via `ghpending`).
- 🎫 **Jira** com suas issues, agrupadas por projeto ou por pai (via `jira`).
- 🔔 **Central de notificações** (`n`) com o que pede sua atenção — hoje as menções a você no Jira.
- ✅ **Tarefas** do Microsoft To Do, com criar/concluir/editar/apagar pela TUI (via `mstodo`).

Painel passivo com navegação leve: rola as listas e abre o corpo de um e-mail.
Os dados atualizam sozinhos a cada 5 minutos (ou na hora, com `r`).

> **Como funciona por baixo:** o daily-tui **não fala** com Gmail/Google/Jira/GitHub
> diretamente. Ele só executa CLIs já instaladas e autenticadas na sua máquina e
> formata a saída delas. Por isso a maior parte da configuração é *configurar e
> autenticar cada CLI* — o que você faz uma vez.

---

## Sumário

- [Instalação rápida](#instalação-rápida)
- [O que é instalado (e por quê)](#o-que-é-instalado-e-por-quê)
- [Configuração das contas](#configuração-das-contas) — **a parte que dá trabalho**
  - 1. E-mail (ortie + himalaya)
  - 2. Agenda (Google Cloud)
  - 3. Tarefas (Microsoft To Do)
  - 4. PRs/issues (GITHUB_TOKEN)
  - 5. Jira (variáveis de ambiente)
- [Ajustando o daily-tui ao seu perfil](#ajustando-o-daily-tui-ao-seu-perfil)
- [Rodando](#rodando)
- [Teclas](#teclas)
- [Arquitetura](#arquitetura)
- [Solução de problemas](#solução-de-problemas)

---

## Instalação rápida

Pré-requisito único: `git` e uma conexão. O resto o script instala.

```sh
git clone https://github.com/joaosouzacoder/daily-tui
cd daily-tui
./scripts/install.sh
```

O `install.sh` é **idempotente** (pode rodar de novo) e faz:

1. instala dependências de sistema (`curl`, `git`, `jq`, toolchain C, `openssl`,
   `libsecret`/`secret-tool` + keyring) pelo gerenciador do seu SO
   (apt / pacman / dnf / zypper / apk / brew);
2. instala **Rust** (rustup) e **uv**, se faltarem;
3. `cargo install himalaya ortie ghpending`;
4. `uv tool install gcalcli`;
5. compila o `daily-tui` em release e cria o link `~/.local/bin/daily-tui`;
6. copia os helpers `jira` e `mstodo` para `~/.local/bin`.

Depois, configure as autenticações com o script guiado
([Configuração das contas](#configuração-das-contas)):

```sh
./scripts/setup-auth.sh email     # e-mail
./scripts/setup-auth.sh google    # agenda
./scripts/setup-auth.sh mstodo    # tarefas
./scripts/setup-auth.sh check     # diagnóstico PASS/FAIL
```

Flags úteis:

```sh
./scripts/install.sh --skip-system   # já tenho os pacotes do SO
./scripts/install.sh --skip-clis     # só (re)compilar o daily-tui + helpers
./scripts/install.sh --bin-dir ~/bin # outro destino para os binários
./scripts/install.sh --help
```

> ⚠️ O script instala e **compila** as CLIs, mas **não configura credenciais**.
> Autenticar cada conta é manual — siga a [Configuração das contas](#configuração-das-contas).

Garanta que `~/.local/bin` e `~/.cargo/bin` estão no seu `PATH` (o script avisa se não estiverem).

> 🪟 **Windows:** `install.sh` é só Linux/macOS. Depois de compilar (`cargo build
> --release`), copie os helpers manualmente para `%USERPROFILE%\.local\bin`
> (que já entra no `PATH`, e é de lá que o daily-tui os invoca por nome via
> `cmd /C`). Os comandos abaixo são do **Prompt de Comando** (`cmd`) — não rode
> no PowerShell, onde `if not exist` é erro de sintaxe e `%USERPROFILE%` não
> expande:
> ```bat
> if not exist "%USERPROFILE%\.local\bin" mkdir "%USERPROFILE%\.local\bin"
> copy scripts\mstodo* %USERPROFILE%\.local\bin\
> copy scripts\jira* %USERPROFILE%\.local\bin\
> ```
> São dois helpers, cada um com o seu shim `.cmd` — que roda o script Python
> irmão (`mstodo`, `jira`) via `uv run --script`, localizando-o pelo `%~dp0`.
> (O `copy` do `cmd` não aceita múltiplas origens soltas — só concatenação com
> `+` — por isso o `*`; o `mkdir` evita o `copy` criar um arquivo chamado `bin`
> se a pasta ainda não existir.)
>
> **Autenticar no Windows:** os `*.sh` não servem. Rode
> `scripts\google-auth.cmd` (ou `powershell -File scripts\google-auth.ps1`): ele
> faz o OAuth da **agenda** nas duas contas e, no fim, o **device code das
> tarefas** (`mstodo auth`), com o client id vindo do `$TodoClientId` em
> `scripts\daily-tui.config.ps1` (copie de `daily-tui.config.example.ps1`). O
> passo das tarefas é pulado quando o token já existe.

---

## O que é instalado (e por quê)

| CLI           | Painel        | Para que serve                                              | Origem                         |
|---------------|---------------|------------------------------------------------------------|--------------------------------|
| `himalaya`    | E-mails       | lista envelopes e lê o corpo via IMAP                      | `cargo install himalaya`       |
| `ortie`       | E-mails       | broker de token OAuth que o himalaya usa para Gmail/Workspace | `cargo install ortie`       |
| `gcalcli`     | Agenda        | lê eventos do Google Calendar em TSV                       | `uv tool install gcalcli`      |
| `ghpending`   | PRs/issues    | digest de PRs/issues abertos nos repos que você acompanha  | `cargo install ghpending`      |
| `jira`        | Jira          | CLI (Python/uv) que consulta a API do Jira e emite JSON    | `scripts/jira` (repo)          |
| `mstodo`      | Tarefas       | CLI (Python/uv) para o Microsoft To Do com CRUD            | `scripts/mstodo` (repo)        |

Ferramentas de apoio:

| Ferramenta | Necessária para | Observação                                                        |
|------------|-----------------|-------------------------------------------------------------------|
| Rust/cargo | compilar tudo   | instalado via rustup pelo `install.sh`                            |
| `uv`       | `gcalcli`, `jira`, `mstodo` | runner Python self-contained (não precisa de venv manual) |
| `secret-tool` (libsecret) + keyring | `ortie`/e-mail | o `ortie` guarda o token OAuth no keyring (gnome-keyring/kwallet) |
| `jq`       | `setup-auth.sh` | lê o client secret do Google                                       |

---

## Configuração das contas

Esta é a parte que dá trabalho — e onde tudo costuma travar. **Use o script
guiado**, que automatiza o que dá e valida o resto:

```sh
./scripts/setup-auth.sh email      # e-mail  (ortie + himalaya)
./scripts/setup-auth.sh google     # agenda (OAuth do Google Cloud)
./scripts/setup-auth.sh mstodo     # tarefas (Microsoft To Do)
./scripts/setup-auth.sh check      # diagnóstico: PASS/FAIL de cada painel
```

GitHub e Jira são só variáveis de ambiente (seções 4 e 5 abaixo).

> 🩺 **Rode `./scripts/setup-auth.sh check` a qualquer momento.** Ele testa cada
> painel com o comando real e mostra uma tabela `PASS`/`FAIL` com a dica do que
> fazer. É a resposta para "está funcionando?".

> ⚠️ Os fluxos OAuth **abrem o navegador** — rode num ambiente gráfico (não num
> SSH puro). E você precisa de um **keyring rodando** (gnome-keyring ou kwallet),
> onde o `ortie` guarda o token do e-mail.

### São três "mundos" de OAuth (entenda antes)

| Painel(éis)       | Como autentica                                                                 | Precisa de projeto/registro próprio? |
|-------------------|--------------------------------------------------------------------------------|-------------------------|
| **E-mail**        | `ortie` usa o **client público do Thunderbird** + guarda o token no keyring     | ❌ não                   |
| **Agenda**        | OAuth client **"Desktop app" do seu projeto no Google Cloud**                   | ✅ sim (1 projeto GCP)   |
| **Tarefas**       | client público **first-party da Microsoft**, via device code (`mstodo auth`)   | ❌ não                   |
| **PRs/issues**    | `GITHUB_TOKEN` (Personal Access Token)                                          | —                       |
| **Jira**          | API token da Atlassian em variável de ambiente                                  | —                       |

> 💡 Tokens (GitHub/Jira) ficam centralizados em
> [`scripts/daily-tui.env.example`](scripts/daily-tui.env.example) — copie os
> `export …` que usar para o `~/.bashrc`/`~/.zshrc` (ou `set -gx` no fish).

### 1. E-mail — ortie + himalaya (`setup-auth.sh email`)

O daily-tui espera duas contas himalaya chamadas `work` e `personal`. A
autenticação **não usa senha**: o himalaya pede o token ao `ortie`, que faz o
fluxo OAuth uma vez e guarda o resultado no keyring.

```sh
./scripts/setup-auth.sh email
```

O script:

1. escreve `~/.config/ortie/config.toml` a partir de
   [`scripts/templates/ortie.toml`](scripts/templates/ortie.toml) (client público
   do Thunderbird — não é segredo);
2. roda `ortie -a gmail-personal auth` e `ortie -a gmail-work auth` (abre o
   navegador; faça login na conta certa em cada um) e salva os tokens no keyring;
3. escreve `~/.config/himalaya/config.toml` a partir de
   [`scripts/templates/himalaya.toml`](scripts/templates/himalaya.toml),
   perguntando os e-mails e nomes de cada conta.

> O client público do Thunderbird funciona para qualquer `@gmail.com` e para
> Google Workspace — **a menos que o admin do Workspace bloqueie apps OAuth de
> terceiros**. Se o login da conta de trabalho for barrado, fale com o admin ou
> use um OAuth client próprio (mesma ideia da seção de Agenda, que já usa um
> OAuth client seu no Google Cloud).
>
> Workspace em outro idioma? Ajuste `folder.aliases.*` no
> `~/.config/himalaya/config.toml` (os nomes das pastas do Gmail mudam por idioma).

Teste: `himalaya envelope list -a work --page-size 1 -o json` deve sair sem erro.

### 2. Agenda — Google Cloud (`setup-auth.sh google`)

A API do Calendar **não** aceita App Password nem o client do Thunderbird:
você precisa de um OAuth client próprio.

**Passo manual (uma vez)**, no [Google Cloud Console](https://console.cloud.google.com/):

1. crie/escolha um projeto;
2. habilite a **Google Calendar API**;
3. *APIs & Services → Credentials → Create OAuth client ID → tipo **Desktop app***;
4. baixe o JSON do client.

Depois, rode e informe o caminho do JSON:

```sh
./scripts/setup-auth.sh google
```

O script extrai o `client_id`/`client_secret` do JSON e autentica o **gcalcli**
em cada conta com diretório isolado
(`XDG_DATA_HOME=~/.local/share/gcalcli-accounts/{personal,work}`), o que permite
duas contas Google na mesma máquina sem conflito de token.

> A agenda é filtrada pela sua calendar primária (`--calendar <e-mail>`), excluindo
> salas e calendários de colegas. O e-mail de cada conta fica em
> [`src/data/mod.rs`](src/data/mod.rs) (`primary_calendar`) — veja
> [Ajustando o daily-tui](#ajustando-o-daily-tui-ao-seu-perfil).

Teste: `XDG_DATA_HOME=~/.local/share/gcalcli-accounts/work gcalcli list` deve
sair sem erro.

> 🔁 **Re-execuções:** uma vez configurado, o JSON fica em
> `~/.config/daily-tui/google-client-secret.json` e o `setup-auth.sh google`
> reusa esse caminho por padrão — não precisa reinformá-lo. (Instalações antigas
> com `gtasks-client-secret.json` continuam funcionando: o script aceita o nome
> antigo como *fallback*.)
>
> 📦 **Compartilhar um único client?** O `client_secret` de um client *Desktop* não
> é segredo (o Google projeta para ser embutido), e um app **Published** dá token
> durável a qualquer conta. Se ele também estiver **verificado**, dá para distribuir
> com o client embutido. Se **não verificado**, funciona para até ~100 contas, porém
> elas veem o aviso de "app não verificado" e tudo consome a cota do *seu* projeto —
> nesse caso prefira que cada um traga o próprio client (os 4 passos acima).

### 3. Tarefas — Microsoft To Do (`setup-auth.sh mstodo`)

Conta pessoal Microsoft, **sem app registration nem client secret**: a
autenticação usa o client público **first-party da própria Microsoft**
(`14d82eec-204b-4c2f-b7e8-296a70dab67e`, "Microsoft Graph Command Line
Tools") via **device code** — o mesmo arranjo que este repo já usa para o
Gmail com o client público do Thunderbird (seção 1). Chegamos nesse desenho
porque o login direto no portal Entra esbarrou numa política de MFA do
*tenant*; o device code autentica no endpoint de consumidor e não depende
dele.

```sh
export DAILY_TUI_TODO_CLIENT_ID="14d82eec-204b-4c2f-b7e8-296a70dab67e"
./scripts/setup-auth.sh mstodo
```

O script roda `mstodo auth`: ele imprime um código e o endereço
`https://www.microsoft.com/link` — abra no navegador (ou no celular), digite
o código e faça login com a conta pessoal. O token fica em
`~/.local/share/daily-tui/mstodo-personal.json`.

Teste: `mstodo list` deve sair sem erro.

> 🪟 **No Windows** o `setup-auth.sh` não roda: use `scripts\google-auth.cmd`,
> que termina neste mesmo `mstodo auth` (o client id vem do `$TodoClientId` do
> `scripts\daily-tui.config.ps1`, então não precisa exportar nada).

> 🧹 **Vindo da versão com Google Tasks?** O painel de tarefas usava o helper
> `gtasks` (Google Tasks) antes desta migração. O token dele **continua no disco,
> com refresh token válido** — apague-o e revogue o acesso:
>
> 1. apague `~/.local/share/daily-tui/gtasks-personal.json`
>    (`%USERPROFILE%\.local\share\daily-tui\gtasks-personal.json` no Windows);
> 2. desabilite a **Google Tasks API** no seu projeto do Google Cloud
>    (*APIs & Services → Enabled APIs*) — a Calendar API continua necessária;
> 3. revogue o escopo do Tasks em
>    [myaccount.google.com/permissions](https://myaccount.google.com/permissions).

> 🔌 **Plano B.** O client acima é de terceiro (da Microsoft, não deste
> projeto). Se um dia a Microsoft restringir o escopo `Tasks.ReadWrite` nele,
> registre o seu, no portal Entra:
>
> 1. *App registrations* → **New registration**;
> 2. *Supported account types*: **Personal Microsoft accounts only**;
> 3. *Authentication* → **Allow public client flows: Yes** (necessário para device code);
> 4. *API permissions* → Microsoft Graph → **Delegated** → **Tasks.ReadWrite** → *Grant consent*;
> 5. copie o **Application (client) ID** para `DAILY_TUI_TODO_CLIENT_ID`.
>
> Trocar de client invalida o cache de token — rode `mstodo auth` de novo.

### 4. PRs/issues — ghpending (`GITHUB_TOKEN`)

```sh
export GITHUB_TOKEN="ghp_xxx"   # PAT com escopo repo; coloque no shell rc
ghpending add                    # escolhe repos de um usuário/org (interativo)
ghpending list                   # confere a lista (~/.config/ghpending/config.toml)
```

Teste: `ghpending` deve imprimir o digest colorido.

### 5. Jira — jira (variáveis de ambiente)

```sh
export JIRA_EMAIL="voce@suaempresa.com"
export JIRA_CLOUD="suaempresa.atlassian.net"
export JIRA_TOKEN="seu_api_token"   # https://id.atlassian.com/manage-profile/security/api-tokens
```

O helper `jira` (Python via `uv`) tem dois subcomandos, ambos emitindo JSON:

```sh
jira issues                          # minhas issues (assignee), com o pai de cada uma
jira issues --filter reporter        # issues em que sou o relator
jira issues --filter both            # os dois filtros combinados
jira mentions                        # issues onde fui mencionado nos últimos 30 dias
```

> O modo `reporter` filtra por `statusCategory = 'In Progress'`, diferente dos
> outros dois (`statusCategory != Done`): a versão simétrica devolvia mais de
> 100 issues, quase todas ruído de um único projeto, contra 7 em andamento.
>
> `mentions` é "onde fui mencionado nos últimos 30 dias" — **não** é
> notificação não lida: o Jira não tem JQL para status de leitura.

Cada issue no JSON traz um campo `"role"`, comparado com o `accountId` do
token: `"assignee"` (sou o responsável), `"reporter"` (só relatei) ou
`"both"` (os dois). Ausência do campo (ex.: `JIRA_JQL` customizada) equivale
a `"assignee"`. No painel, o marcador (`[A]`/`[R]`/`[AR]`) só aparece no
filtro `ambas` — nos outros filtros toda issue tem o mesmo papel.

`JIRA_JQL`, se definida, substitui a consulta inteira do `issues` e o
`--filter` deixa de ter efeito:

```sh
export JIRA_JQL="project = ENG AND statusCategory != Done ORDER BY updated DESC"
```

Teste: `jira issues` deve listar suas issues em JSON.

---

## Ajustando o daily-tui ao seu perfil

**E-mails da agenda — por variável de ambiente (sem recompilar):**

```sh
export DAILY_TUI_PERSONAL_EMAIL="voce@gmail.com"
export DAILY_TUI_WORK_EMAIL="voce@suaempresa.com"
```

São os e-mails usados no filtro `--calendar` do gcalcli. Sem eles, a agenda usa um
placeholder e não acha seus eventos. (Ficam em
[`scripts/daily-tui.env.example`](scripts/daily-tui.env.example).)

**Nomes das contas — fixos em código** (`work`/`personal`), em
[`src/data/mod.rs`](src/data/mod.rs). Só precisa mexer se quiser outros nomes;
recompile depois com `./scripts/install.sh --skip-system --skip-clis`:

- `himalaya_name` — precisa bater com os nomes em `himalaya account configure`;
- `gcalcli_dir` — subdiretório de cada conta sob `~/.local/share/gcalcli-accounts`.

Os helpers `jira` e `mstodo` já são configuráveis por variáveis de ambiente
(não exigem editar código).

---

## Rodando

Depois do `install.sh`, com `~/.local/bin` no PATH:

```sh
daily-tui
```

Ou direto do repo, sem instalar o link:

```sh
cargo run --release
```

Painéis sem credencial configurada mostram o erro da CLI no lugar dos dados — o
resto do painel continua funcionando.

---

## Teclas

| Tecla          | Ação                                    |
|----------------|-----------------------------------------|
| `Tab` / `⇧Tab` | Troca o painel focado                   |
| `j` / `k`      | Rola para baixo / cima                  |
| `g` / `G`      | Topo / fim da lista                     |
| `Enter`        | Abre o item do painel focado             |
| `Esc`          | Fecha o detalhe / cancela o prompt      |
| `r`            | Atualiza os dados agora                 |
| `q` / `Ctrl-C` | Sai                                     |

O rodapé mostra as teclas do painel em foco, então não é preciso decorar.

No painel **E-mails**: `Enter` abre o corpo · `Espaço` marca lido/não lido ·
`m` move para uma pasta · `d` exclui · `Shift`+`↑`/`↓` marca vários · `x` marca ou
desmarca o de baixo do cursor · `Esc` limpa a marcação toda. **Excluir move para a Lixeira**, não apaga do servidor — é
recuperável, e pede confirmação (`y`/`n`).

**Ação em lote:** `Shift` com as setas marca uma faixa (as marcadas ganham `✓` e
o título do painel mostra a contagem), e a partir daí `Espaço`, `m` e `d` agem
sobre todas de uma vez, com uma re-busca só no fim. Num lote com estados mistos,
`Espaço` marca tudo como lido — basta um não lido para a ação virar essa. Só as
setas funcionam com `Shift`: o terminal entrega `Shift+j` como `J`, sem
modificador. O `Shift` só marca — para tirar um item da faixa sem desfazer o
resto, use `x`.

**As pastas do seletor vêm do servidor**, não de uma lista fixa — no Gmail isso
inclui todas as suas etiquetas (40 na conta do autor), com as canônicas primeiro
e o resto em ordem alfabética. São listadas **em segundo plano no arranque** e
relistadas a cada 10 minutos, então o seletor abre pronto; a listagem tem cadência
própria porque etiqueta nova é raro e cada listagem é uma ida ao IMAP por conta.

O seletor mostra as pastas de **todas as contas** presentes nos alvos, cada uma
com o marcador da conta (`[W]`/`[P]`). Escolher uma pasta move só os alvos daquela
conta — uma etiqueta do work não existe na pessoal, então o resto do lote fica
onde está.

**O corpo é buscado em segundo plano.** Um segundo depois de o cursor parar num
e-mail, o corpo é buscado e guardado em cache — então `Enter` costuma abrir
instantâneo. A busca usa `--preview`, que impede o himalaya de marcar o e-mail
como lido só por ter sido aberto: marcar é decisão sua, com `Espaço`.

E-mail que só tem parte HTML é convertido para texto legível (tags fora, `script`
e `style` descartados, blocos virando linhas, entidades decodificadas — inclusive
as acentuadas do português). Quando existe parte texto, o himalaya já a prefere e
o corpo passa intacto.

As escritas aparecem na tela **na hora** e a re-busca reconcilia depois. Se a
escrita falhar, o erro aparece no painel e a lista volta ao que o servidor diz.

No painel **Tarefas**: `Espaço` conclui/reabre · `a` cria · `A` cria subtarefa ·
`e` edita · `d` apaga (confirma com `y`/`n`) · `Enter` expande/recolhe as
subtarefas.

A lista é agrupada por prazo — **ATRASADAS**, **HOJE**, **ESTA SEMANA**,
**ESTE MÊS**, **DEPOIS**, **SEM DATA** —, e é essa a ordem de prioridade: o que
passou da data primeiro, o que não tem data no fim. As janelas são móveis (7 e 30
dias a partir de hoje), não o calendário: numa sexta, "esta semana" pelo
calendário mostraria dois dias e jogaria o resto para o mês. Faixa vazia não
aparece, e o cursor pula os cabeçalhos.

Prioridade alta marca a linha com `!` (baixa com `↓`, normal não marca nada) e
tarefa que repete ganha `↻` ao lado do prazo.

`e` numa tarefa abre um formulário com os quatro campos de uma vez — título,
vencimento, repetição e prioridade. `Tab`/`Shift+Tab` (ou `↑`/`↓`) andam pelos
campos, texto se digita, e `Espaço`/`←`/`→` circulam repetição
(nenhuma → diária → semanal → mensal) e prioridade (normal → alta → baixa).
`Enter` grava tudo numa chamada só; `Esc` cancela.

No campo de vencimento vale `AAAA-MM-DD`, `hoje`, `amanhã` e `+3d`; **vazio
limpa a data**. Data que não dá para entender não fecha o formulário — ele volta
com o motivo no lugar da linha de ajuda, sem perder o que você digitou.

> Repetição exige data: o Graph recusa recorrência sem vencimento e pede a data
> no mesmo pedido, então o formulário manda as duas juntas. Uma tarefa que repete
> de um jeito que este painel não oferece (criada no app do To Do) aparece como
> `outra (do app)` — dá para tirar a repetição, não para reproduzi-la.

`A` cria a subtarefa na tarefa da linha onde você está — inclusive quando o
cursor já está sobre uma subtarefa, aí a nova entra como irmã dela. A mãe abre
sozinha, senão a subtarefa nova chegaria escondida numa tarefa recolhida.

`e` e `d` seguem a linha: sobre uma tarefa agem na tarefa, sobre uma subtarefa
agem na subtarefa (renomear e apagar a etapa, sem tocar na mãe).

Subtarefa no Microsoft To Do é o que o app chama de **etapa** (`checklistItem` no
Graph) — quem procurar "subtarefa" na interface da Microsoft não acha. Uma tarefa
com etapas mostra `▸` quando recolhida e `▾` quando expandida; sem etapas, não
mostra marca nenhuma. Com as etapas à vista, o cursor anda por elas e o `Espaço`
age na linha sob o cursor: a tarefa, ou a etapa. Criar, editar e apagar continuam
valendo só para tarefas.

`n` abre a **central de notificações** de qualquer painel: um overlay com o que
pede sua atenção, cada linha marcada pela fonte (`[JIRA]` hoje). `j`/`k` navegam,
`Espaço` marca como lida, `Enter` abre no navegador, `Esc` ou `n` fecham. O que
você marca como lido é gravado num banco SQLite local e **não volta** — a
identidade é a issue, então uma menção dispensada fica dispensada. Hoje ela lista as menções a você
no Jira dos últimos 30 dias; foi desenhada para receber outras fontes — convites
de agenda para aceitar, menções no GitHub — sem mudar o overlay.

Cada linha do painel de Jira abre com o tipo da issue: `[S]` história, `[E]`
épico, `[I]` iniciativa, `[O]` objetivo, `[R]` requisição (o Pedido de Serviço,
que a API devolve como `[System] Service request`). O nome do tipo vem no idioma
da sua instância (a do autor responde `História`/`Iniciativa`), e os dois idiomas
estão no mapa; tipo fora dele aparece como `[?]` em vez de virar uma letra
chutada pela inicial — `Subtarefa` ficaria igual a história.

No painel **Jira**: `Enter` abre a issue selecionada no navegador · `f` circula
o filtro (`minhas` → `relator` → `ambas`) · `p` mostra as issues agrupadas por
pai · `Esc` volta para a visão de issues. O rodapé mostra essas teclas sempre que
o painel de Jira (ou o de Tarefas) está em foco. No filtro `ambas`, cada issue
ganha um marcador esmaecido — `[A]` assignee, `[R]` reporter, `[AR]` os dois —
porque só ali a pergunta "sou responsável ou apenas relator disto?" tem resposta
ambígua.

---

## Arquitetura

- Estilo Elm/TEA do [`ratatui-tea`](https://crates.io/crates/ratatui-tea) +
  tema/componentes do [`ratatui-bubbletea`](https://github.com/akitaonrails/ratatui-bubbletea),
  mas com event loop próprio em `main.rs`.
- Uma **thread worker** (`worker.rs`) roda as CLIs e manda os resultados pelo
  canal do `ratatui-tea`, então nada bloqueia o relógio nem o teclado.
- Cada fonte de dado é um módulo em `src/data/` que só executa uma CLI e parseia a
  saída (`email.rs`, `agenda.rs`, `pulls.rs`, `jira.rs`, `tasks.rs`).
- O que precisa sobreviver ao fechar o programa fica num SQLite local
  (`src/store.rs`): as notificações já lidas e o cache das pastas do e-mail —
  listar etiquetas no IMAP leva segundos, e o seletor de "mover" abre cheio com o
  que estava em cache enquanto o worker relista em segundo plano. Só a thread da
  UI fala com o banco.

  | Sistema | Caminho do banco                          |
  |---------|-------------------------------------------|
  | Windows | `%LOCALAPPDATA%\daily-tui\daily-tui.db`    |
  | Linux   | `~/.local/share/daily-tui/daily-tui.db`   |

  Apagar o arquivo é seguro: as notificações lidas voltam a aparecer e as pastas
  são relistadas.

Veja o design em [`docs/superpowers/specs/2026-06-09-daily-tui-design.md`](docs/superpowers/specs/2026-06-09-daily-tui-design.md).

Testes: `cargo test`.

---

## Solução de problemas

Primeiro recurso, sempre: **`./scripts/setup-auth.sh check`** — ele aponta qual painel está quebrado e o que fazer.

| Sintoma                                          | Causa provável / o que checar                                                        |
|--------------------------------------------------|--------------------------------------------------------------------------------------|
| `falha ao executar <cli>`                        | a CLI não está no PATH. Confira `~/.local/bin` / `~/.cargo/bin` no `PATH`.            |
| Painel de e-mail vazio ou com erro               | token do `ortie` expirou ou não existe — rode `ortie -a gmail-work token show`; se falhar, `setup-auth.sh email`. |
| `ortie`: erro de keyring / `secret-tool`         | não há keyring rodando. Inicie o gnome-keyring/kwallet (sessão gráfica) e refaça o auth. |
| `Workspace`: login do e-mail recusado            | o admin do Workspace bloqueia apps OAuth de terceiros — use um OAuth client próprio.  |
| Agenda vazia                                     | token do gcalcli daquela conta expirou — `XDG_DATA_HOME=... gcalcli list` ou `setup-auth.sh google`. |
| `jira falhou`                                     | `JIRA_EMAIL`/`JIRA_CLOUD`/`JIRA_TOKEN` ausentes ou token inválido.                    |
| `mstodo: sem credenciais — rode: mstodo auth`    | falta autorizar; rode `mstodo auth` (ou `setup-auth.sh mstodo`; no Windows, `scripts\google-auth.cmd`). |
| `client secret não encontrado`                   | baixe o OAuth client (Google Cloud) e rode `setup-auth.sh google`.                   |
| Erro de compilação por OpenSSL                   | falta `libssl-dev`/`openssl-devel` — rode o `install.sh` sem `--skip-system`.        |
| `banco indisponível: …` na central de notificações | o SQLite não abriu (permissão, disco cheio). O painel segue funcionando, só sem memória entre execuções. |
