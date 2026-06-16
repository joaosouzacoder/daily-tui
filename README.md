# daily-tui

Painel TUI para deixar sempre rodando num monitor, com o que importa no dia a dia:

- 🕐 **Relógio** em tempo real (`HH:MM:SS` + data por extenso em pt-BR).
- 📧 **E-mails** agregados das contas *work* + *personal* (via `himalaya`).
- 📅 **Agenda** dos próximos 7 dias, agregada das duas contas Google (via `gcalcli`).
- 🔀 **PRs/issues** pendentes nos repos monitorados (via `ghpending`).
- 🎫 **Jira** com os tickets abertos atribuídos a você, agrupados por projeto (via `jirapending`).
- ✅ **Tarefas** do Google Tasks, com criar/concluir/editar/apagar pela TUI (via `gtasks`).

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
  - 2. Agenda + Tarefas (Google Cloud)
  - 3. PRs/issues (GITHUB_TOKEN)
  - 4. Jira (variáveis de ambiente)
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
6. copia os helpers `jirapending` e `gtasks` para `~/.local/bin`.

Depois, configure as autenticações com o script guiado
([Configuração das contas](#configuração-das-contas)):

```sh
./scripts/setup-auth.sh email     # e-mail
./scripts/setup-auth.sh google    # agenda + tarefas
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

---

## O que é instalado (e por quê)

| CLI           | Painel        | Para que serve                                              | Origem                         |
|---------------|---------------|------------------------------------------------------------|--------------------------------|
| `himalaya`    | E-mails       | lista envelopes e lê o corpo via IMAP                      | `cargo install himalaya`       |
| `ortie`       | E-mails       | broker de token OAuth que o himalaya usa para Gmail/Workspace | `cargo install ortie`       |
| `gcalcli`     | Agenda        | lê eventos do Google Calendar em TSV                       | `uv tool install gcalcli`      |
| `ghpending`   | PRs/issues    | digest de PRs/issues abertos nos repos que você acompanha  | `cargo install ghpending`      |
| `jirapending` | Jira          | script (bash) que consulta a API do Jira e colore a saída  | `scripts/jirapending` (repo)   |
| `gtasks`      | Tarefas       | CLI (Python/uv) para o Google Tasks com CRUD               | `scripts/gtasks` (repo)        |

Ferramentas de apoio:

| Ferramenta | Necessária para | Observação                                                        |
|------------|-----------------|-------------------------------------------------------------------|
| Rust/cargo | compilar tudo   | instalado via rustup pelo `install.sh`                            |
| `uv`       | `gcalcli`, `gtasks` | runner Python self-contained (não precisa de venv manual)     |
| `secret-tool` (libsecret) + keyring | `ortie`/e-mail | o `ortie` guarda o token OAuth no keyring (gnome-keyring/kwallet) |
| `jq`       | `jirapending`, `setup-auth.sh` | monta JSON e lê o client secret do Google            |
| `curl`     | `jirapending`   | chama a REST API do Jira                                           |
| `op`       | `jirapending` (opcional) | só no *fallback* do token via 1Password CLI              |

---

## Configuração das contas

Esta é a parte que dá trabalho — e onde tudo costuma travar. **Use o script
guiado**, que automatiza o que dá e valida o resto:

```sh
./scripts/setup-auth.sh email      # e-mail  (ortie + himalaya)
./scripts/setup-auth.sh google     # agenda + tarefas (OAuth do Google Cloud)
./scripts/setup-auth.sh check      # diagnóstico: PASS/FAIL de cada painel
```

GitHub e Jira são só variáveis de ambiente (seções 3 e 4 abaixo).

> 🩺 **Rode `./scripts/setup-auth.sh check` a qualquer momento.** Ele testa cada
> painel com o comando real e mostra uma tabela `PASS`/`FAIL` com a dica do que
> fazer. É a resposta para "está funcionando?".

> ⚠️ Os fluxos OAuth **abrem o navegador** — rode num ambiente gráfico (não num
> SSH puro). E você precisa de um **keyring rodando** (gnome-keyring ou kwallet),
> onde o `ortie` guarda o token do e-mail.

### São dois "mundos" de OAuth (entenda antes)

| Painel(éis)       | Como autentica                                                                 | Precisa de projeto GCP? |
|-------------------|--------------------------------------------------------------------------------|-------------------------|
| **E-mail**        | `ortie` usa o **client público do Thunderbird** + guarda o token no keyring     | ❌ não                   |
| **Agenda + Tarefas** | OAuth client **"Desktop app" do seu projeto no Google Cloud**               | ✅ sim (1 projeto)       |
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
> use um OAuth client próprio (mesma ideia do mundo "agenda/tarefas").
>
> Workspace em outro idioma? Ajuste `folder.aliases.*` no
> `~/.config/himalaya/config.toml` (os nomes das pastas do Gmail mudam por idioma).

Teste: `himalaya envelope list -a work --page-size 1 -o json` deve sair sem erro.

### 2. Agenda + Tarefas — Google Cloud (`setup-auth.sh google`)

A API do Calendar e do Tasks **não** aceita App Password nem o client do
Thunderbird: você precisa de **um** OAuth client próprio (serve para os dois).

**Passo manual (uma vez)**, no [Google Cloud Console](https://console.cloud.google.com/):

1. crie/escolha um projeto;
2. habilite **Google Calendar API** e **Google Tasks API**;
3. *APIs & Services → Credentials → Create OAuth client ID → tipo **Desktop app***;
4. baixe o JSON do client.

Depois, rode e informe o caminho do JSON:

```sh
./scripts/setup-auth.sh google
```

O script extrai o `client_id`/`client_secret` do JSON e:

- autentica o **gcalcli** em cada conta com diretório isolado
  (`XDG_DATA_HOME=~/.local/share/gcalcli-accounts/{personal,work}`), o que permite
  duas contas Google na mesma máquina sem conflito de token;
- copia o JSON para `~/.config/daily-tui/gtasks-client-secret.json` e roda
  `gtasks auth` (token salvo em `~/.local/share/daily-tui/gtasks-personal.json`).

> A agenda é filtrada pela sua calendar primária (`--calendar <e-mail>`), excluindo
> salas e calendários de colegas. O e-mail de cada conta fica em
> [`src/data/mod.rs`](src/data/mod.rs) (`primary_calendar`) — veja
> [Ajustando o daily-tui](#ajustando-o-daily-tui-ao-seu-perfil).

Teste: `XDG_DATA_HOME=~/.local/share/gcalcli-accounts/work gcalcli list` e
`gtasks list` devem sair sem erro.

> 🔁 **Re-execuções:** uma vez configurado, o JSON fica em
> `~/.config/daily-tui/gtasks-client-secret.json` e o `setup-auth.sh google`
> reusa esse caminho por padrão — não precisa reinformá-lo.
>
> 📦 **Compartilhar um único client?** O `client_secret` de um client *Desktop* não
> é segredo (o Google projeta para ser embutido), e um app **Published** dá token
> durável a qualquer conta. Se ele também estiver **verificado**, dá para distribuir
> com o client embutido. Se **não verificado**, funciona para até ~100 contas, porém
> elas veem o aviso de "app não verificado" e tudo consome a cota do *seu* projeto —
> nesse caso prefira que cada um traga o próprio client (os 4 passos acima).

### 3. PRs/issues — ghpending (`GITHUB_TOKEN`)

```sh
export GITHUB_TOKEN="ghp_xxx"   # PAT com escopo repo; coloque no shell rc
ghpending add                    # escolhe repos de um usuário/org (interativo)
ghpending list                   # confere a lista (~/.config/ghpending/config.toml)
```

Teste: `ghpending` deve imprimir o digest colorido.

### 4. Jira — jirapending (variáveis de ambiente)

```sh
export JIRA_EMAIL="voce@suaempresa.com"
export JIRA_CLOUD="suaempresa.atlassian.net"
export JIRA_TOKEN="seu_api_token"   # https://id.atlassian.com/manage-profile/security/api-tokens
```

> Não quer o token no shell? **Não** exporte `JIRA_TOKEN`: o script busca no
> 1Password (`op item get "Token JIRA"`). Customize com `JIRA_OP_ITEM` e `JIRA_JQL`.

Teste: `jirapending` deve listar seus tickets agrupados por projeto.

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

Os helpers `jirapending` e `gtasks` já são configuráveis por variáveis de ambiente
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
| `Enter`        | Abre o corpo do e-mail (painel E-mails) |
| `Esc`          | Fecha o detalhe / cancela o prompt      |
| `r`            | Atualiza os dados agora                 |
| `q` / `Ctrl-C` | Sai                                     |

No painel **Tarefas**: `Espaço` conclui/reabre · `a` cria · `e` edita · `d` apaga
(confirma com `y`/`n`).

---

## Arquitetura

- Estilo Elm/TEA do [`ratatui-tea`](https://crates.io/crates/ratatui-tea) +
  tema/componentes do [`ratatui-bubbletea`](https://github.com/akitaonrails/ratatui-bubbletea),
  mas com event loop próprio em `main.rs`.
- Uma **thread worker** (`worker.rs`) roda as CLIs e manda os resultados pelo
  canal do `ratatui-tea`, então nada bloqueia o relógio nem o teclado.
- Cada fonte de dado é um módulo em `src/data/` que só executa uma CLI e parseia a
  saída (`email.rs`, `agenda.rs`, `pulls.rs`, `jira.rs`, `tasks.rs`).

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
| `jirapending falhou`                             | `JIRA_EMAIL`/`JIRA_CLOUD`/`JIRA_TOKEN` ausentes ou token inválido.                    |
| `gtasks: sem credenciais — rode: gtasks auth`    | falta autorizar; rode `gtasks auth` (ou `setup-auth.sh google`).                     |
| `client secret não encontrado`                   | baixe o OAuth client (Google Cloud) e rode `setup-auth.sh google`.                   |
| Erro de compilação por OpenSSL                   | falta `libssl-dev`/`openssl-devel` — rode o `install.sh` sem `--skip-system`.        |
