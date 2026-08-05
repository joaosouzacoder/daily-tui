# Compartilhar o daily-tui — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Outra pessoa clona o repo, liga só os painéis que usa, descobre pelo README como autenticar cada um, e roda — sem editar código.

**Architecture:** Um arquivo TOML no diretório de config do SO passa a ser a única fonte de verdade sobre *quais painéis existem* e *quais contas existem*. O binário carrega esse arquivo uma vez para um `OnceLock` e o resto do código consulta essa configuração no lugar das constantes de hoje (`ACCOUNTS`, `Panel::next`, layout fixo, `himalaya_name()`). Painel desligado não aparece, não é focável, não busca nada e não exige credencial nenhuma. A documentação de autenticação passa a ser por painel, e o `setup-auth.sh check` só cobra o que está ligado.

**Tech Stack:** Rust + ratatui (já no projeto), `toml` (novo), `serde` (já), helpers externos (himalaya, gcalcli, ghpending, jira, mstodo).

## Global Constraints

- **O repo é público.** Nenhum e-mail real, domínio do empregador, nome de cliente ou chave de ticket em arquivo versionado. Fixture reproduz a *forma* do real com conteúdo inventado.
- **Nenhum segredo no arquivo de config.** Tokens continuam em variável de ambiente / keychain. O `config.example.toml` só carrega valores públicos (o client id first-party da Microsoft) e placeholders óbvios (`voce@empresa.com`).
- **Sem config, nada muda para quem já usa.** Arquivo ausente = todos os painéis ligados e as duas contas `work`/`personal` como hoje. Arquivo inválido = erro no stderr e saída diferente de zero **antes** de entrar na TUI (nunca cair silenciosamente no default).
- **Chave do SQLite é o slot, não o nome da conta.** O nome do himalaya passa a ser configurável; o banco continua chaveado por `work`/`personal` (o slot), senão renomear a conta invalida o cache de pastas.
- `cargo test` verde e `cargo clippy --all-targets` na baseline de **3 warnings** ao fim de cada task.
- Commits em inglês, sem `Co-Authored-By` nem assinatura de AI.
- Verificação de escrita em serviço real (Graph, IMAP) sempre com item descartável, apagado depois.
- Não rodar `cargo build --release` com o `daily-tui.exe` em uso (o launcher do Windows aponta para ele).

---

## File Structure

| Arquivo | Responsabilidade |
|---|---|
| `src/config.rs` **(novo)** | Schema, defaults, carregamento, `OnceLock`, `--print-config`. Único lugar que conhece o formato do TOML. |
| `config.example.toml` **(novo)** | Config comentado. Serve de documentação e de fonte do `--init` (via `include_str!`). |
| `src/main.rs` | Lê os argumentos (`--config`, `--init`, `--print-config`, `--help`), carrega o config antes de tudo, monta o worker com o que está ligado. |
| `src/data/mod.rs` | `Account` deixa de ter strings fixas: `himalaya_name`/`gcalcli_dir`/`marker`/`primary_calendar` passam a ler o config. Ganha `slot_key()` (fixo) para o banco. `helper_command` injeta no helper o env que vem do config. |
| `src/app.rs` | `Panel` ganha o conjunto de habilitados: ciclo de foco, foco inicial. |
| `src/ui.rs` | Layout por peso normalizado sobre os painéis ligados. |
| `src/worker.rs` | Busca só o que está ligado; `ACCOUNTS` vem do config. |
| `src/store.rs` | Chaveia pastas por `slot_key()`. |
| `README.md` | Reestruturado: "comece aqui", tabela painel → CLI → credencial → teste, e uma seção de autenticação por painel. |
| `scripts/setup-auth.sh` | `check` consome `daily-tui --print-config` e só cobra painel ligado. |
| `scripts/install.ps1` **(novo, Fase 6, opcional)** | Equivalente do `install.sh` no Windows. |

---

### Task 0: Limpar o que é pessoal antes de convidar gente

**Files:**
- Modify: `src/data/mod.rs:109` (comentário com o nome do empregador)
- Modify: `docs/superpowers/specs/2026-06-09-daily-tui-design.md:15` (idem)
- Modify: `src/data/testdata/mstodo-proxy-traceback.txt` (caminhos com o usuário do Windows)
- Modify: `.gitignore`

**Interfaces:** nenhuma. É higiene.

- [ ] **Step 1: Achar tudo antes de tocar em nada**

Monte o padrão na hora, com os seus dados, e **não** o escreva em arquivo
versionado — foi exatamente esse o erro na primeira versão deste plano:

```bash
# Substitua pelos seus: usuário do SO, nome do empregador, domínio do Jira.
PAT="$(printf '%s|%s|%s' "$USER" "seu-empregador" "seu-dominio")"
git ls-files -z | xargs -0 grep -niE "$PAT"
```

Esperado hoje (medido em 2026-08-05): um comentário com o nome do empregador em
`src/data/mod.rs`, a mesma menção na linha 15 do spec de 2026-06-09, e o seu
usuário do Windows nos caminhos do fixture do traceback. **Nada de e-mail nem de
domínio Atlassian** em arquivo versionado — se aparecer algo novo, pare e mostre
antes de seguir.

- [ ] **Step 2: Trocar por descrição genérica**

Em `src/data/mod.rs`, os comentários das variantes:

```rust
pub enum Account {
    /// Primeira conta configurada (por convenção, a do trabalho).
    Work,
    /// Segunda conta configurada (por convenção, a pessoal).
    Personal,
}
```

No spec de 2026-06-09, a linha 15 nomeia o empregador ao descrever as contas:
troque a menção por `(a do trabalho + a pessoal)`.

Nos dois fixtures de erro — o traceback em `testdata/` e o do PowerShell dentro
de `mod.rs` — troque o seu usuário do Windows por `voce`. O que os testes
exercitam é a *forma* do erro (onde fica a mensagem que interessa), não o
caminho.

- [ ] **Step 3: Rodar os testes**

Run: `cargo test data::tests`
Expected: PASS — inclusive `picks_the_exception_of_a_python_traceback_not_its_header`, que lê o fixture.

- [ ] **Step 4: Ignorar o config local**

Em `.gitignore`, junto do bloco que já ignora `scripts/daily-tui.config.ps1`:

```gitignore
# Config local (painéis, contas, e-mails) — pessoal, nunca versionado.
/daily-tui.toml
```

> Mesmo com o config morando no diretório do SO, alguém vai criar um `daily-tui.toml` na raiz para testar com `--config`. Ignorar custa uma linha e evita o vazamento.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: remove employer and username traces from tracked files"
```

---

### Task 1: O módulo de config

**Files:**
- Create: `src/config.rs`
- Create: `config.example.toml`
- Modify: `src/main.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consumes: nada (é a base).
- Produces:
  - `config::load(path: Option<&Path>) -> Result<Config, String>`
  - `config::init(&Config)` — grava no `OnceLock`; chamada uma vez, no `main`.
  - `config::get() -> &'static Config` — panica se `init` não rodou (bug de programação, não de uso).
  - `config::default_path() -> PathBuf`
  - `Config { panels: Panels, accounts: Vec<AccountCfg>, email: EmailCfg, jira: JiraCfg, tasks: TasksCfg, refresh: RefreshCfg }`
  - `Panels { email: bool, jira: bool, agenda: bool, pulls: bool, tasks: bool }`
  - `AccountCfg { id: String, label: String, email: String, calendar: String }`
  - `Config::print_shell(&self) -> String`

- [ ] **Step 1: Adicionar a dependência**

```bash
cargo add toml
```

- [ ] **Step 2: Escrever o `config.example.toml`**

Ele é documentação: cada campo com um comentário do que muda na tela.

```toml
# daily-tui — config. Copie para:
#   Linux/mac  ~/.config/daily-tui/config.toml
#   Windows    %APPDATA%\daily-tui\config.toml
# ou rode `daily-tui --init`, que faz essa cópia.
#
# Nada aqui é segredo. Tokens continuam em variável de ambiente
# (JIRA_TOKEN, GITHUB_TOKEN) ou no keychain do sistema.

# Quais painéis existem. Painel desligado não aparece, não recebe foco,
# não busca nada — e você não precisa autenticar a ferramenta dele.
[panels]
email  = true   # himalaya (IMAP)
jira   = true   # helper `jira` (REST do Jira Cloud)
agenda = true   # gcalcli (Google Calendar)
pulls  = true   # ghpending (GitHub)
tasks  = true   # helper `mstodo` (Microsoft To Do)

# Até duas contas, usadas pelos painéis de e-mail e agenda.
# Quem tem uma conta só apaga o segundo bloco.
[[accounts]]
id       = "work"              # nome da conta no himalaya (`himalaya account list`)
label    = "W"                 # marcador na lista: [W]
email    = "voce@empresa.com"  # calendar primária no gcalcli
calendar = "work"              # subpasta em ~/.local/share/gcalcli-accounts

[[accounts]]
id       = "personal"
label    = "P"
email    = "voce@gmail.com"
calendar = "personal"

[email]
limit = 30   # envelopes buscados por conta

[jira]
cloud = "empresa.atlassian.net"   # domínio da sua instância
email = "voce@empresa.com"        # e-mail da conta Atlassian
# O token vem de JIRA_TOKEN no ambiente.

[tasks]
# Vazio = lista padrão do To Do ("Tarefas").
list = ""
# Client público first-party da Microsoft ("Microsoft Graph Command Line Tools").
# Não é segredo, e não exige app registration próprio.
client_id = "14d82eec-204b-4c2f-b7e8-296a70dab67e"

[refresh]
seconds = 300   # de quanto em quanto tempo tudo é rebuscado
```

- [ ] **Step 3: Escrever o teste que falha primeiro**

Em `src/config.rs`, ainda sem implementação:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_example_parses_into_the_defaults_people_expect() {
        // O exemplo é documentação: se ele não parseia, a documentação mente.
        let cfg = parse(include_str!("../config.example.toml")).expect("exemplo válido");
        assert!(cfg.panels.email && cfg.panels.jira && cfg.panels.tasks);
        assert_eq!(cfg.accounts.len(), 2);
        assert_eq!(cfg.accounts[0].id, "work");
        assert_eq!(cfg.accounts[1].label, "P");
        assert_eq!(cfg.email.limit, 30);
        assert_eq!(cfg.refresh.seconds, 300);
    }

    #[test]
    fn an_empty_file_means_everything_on_with_the_two_usual_accounts() {
        // Quem já usava o painel não pode perder nada por não ter config.
        let cfg = parse("").expect("vazio é válido");
        assert!(cfg.panels.email && cfg.panels.jira && cfg.panels.agenda);
        assert!(cfg.panels.pulls && cfg.panels.tasks);
        let ids: Vec<&str> = cfg.accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["work", "personal"]);
    }

    #[test]
    fn one_account_is_a_valid_setup() {
        let cfg = parse(
            r#"
            [[accounts]]
            id = "gmail"
            label = "G"
            email = "eu@exemplo.com"
            calendar = "gmail"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.accounts.len(), 1);
    }

    #[test]
    fn a_typo_is_an_error_instead_of_a_silent_default() {
        // `pannels` ligaria tudo em silêncio e a pessoa acharia que o config
        // não funciona.
        let err = parse("[pannels]\nemail = false\n").unwrap_err();
        assert!(err.contains("pannels"), "o erro nomeia o campo: {err}");
    }

    #[test]
    fn turning_every_panel_off_is_refused() {
        let err = parse(
            "[panels]\nemail=false\njira=false\nagenda=false\npulls=false\ntasks=false\n",
        )
        .unwrap_err();
        assert!(err.contains("nenhum painel"), "{err}");
    }

    #[test]
    fn no_account_is_refused_when_a_panel_needs_one() {
        // `accounts = []` deixaria e-mail e agenda vazios para sempre, sem dizer
        // por quê.
        let err = parse("accounts = []\n").unwrap_err();
        assert!(err.contains("conta"), "{err}");
    }

    #[test]
    fn three_accounts_are_refused_with_the_reason() {
        let one = r#"
            [[accounts]]
            id = "a"
            label = "A"
            email = "a@x.com"
            calendar = "a"
        "#;
        let err = parse(&one.repeat(3)).unwrap_err();
        assert!(err.contains("duas"), "{err}");
    }

    #[test]
    fn the_shell_dump_is_consumable_by_the_doctor_script() {
        let cfg = parse("[panels]\njira = false\n").unwrap();
        let dump = cfg.print_shell();
        assert!(dump.contains("PANEL_EMAIL=1"));
        assert!(dump.contains("PANEL_JIRA=0"));
        assert!(dump.contains("ACCOUNT_IDS=\"work personal\""));
    }
}
```

- [ ] **Step 4: Rodar e ver falhar**

Run: `cargo test config::`
Expected: falha de compilação (`parse` não existe).

- [ ] **Step 5: Implementar**

```rust
//! Config do usuário: quais painéis existem e quais contas existem.
//!
//! Um arquivo TOML no diretório de config do SO. Ausente é válido — vale o
//! default, que é o comportamento de antes deste arquivo existir. Inválido não
//! é: cair no default em silêncio faria a pessoa achar que o config não pega.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub panels: Panels,
    #[serde(default = "default_accounts")]
    pub accounts: Vec<AccountCfg>,
    #[serde(default)]
    pub email: EmailCfg,
    #[serde(default)]
    pub jira: JiraCfg,
    #[serde(default)]
    pub tasks: TasksCfg,
    #[serde(default)]
    pub refresh: RefreshCfg,
}
```

Os demais structs seguem o mesmo padrão: `#[serde(deny_unknown_fields)]`, `Default` implementado à mão (não derivado) para o default ser o de hoje — `Panels` todo `true`, `EmailCfg { limit: 30 }`, `RefreshCfg { seconds: 300 }`, `TasksCfg { list: "", client_id: <client público> }`, `JiraCfg { cloud: "", email: "" }` (vazio = herda do ambiente).

`default_accounts()` devolve os dois slots de hoje (`work`/`W` e `personal`/`P`, e-mail vazio e calendar igual ao id).

```rust
/// Parseia e valida. Erro é uma linha, para caber no stderr.
fn parse(raw: &str) -> Result<Config, String> {
    let cfg: Config = toml::from_str(raw).map_err(|e| format!("config inválido: {e}"))?;
    cfg.validate()?;
    Ok(cfg)
}

impl Config {
    fn validate(&self) -> Result<(), String> {
        if !self.panels.any() {
            return Err("nenhum painel ligado: o [panels] desligou todos".into());
        }
        match self.accounts.len() {
            0 => Err("nenhuma conta configurada: e-mail e agenda ficariam vazios".into()),
            1 | 2 => Ok(()),
            n => Err(format!("são no máximo duas contas, e o config traz {n}")),
        }
    }
}
```

`load(path)`: caminho explícito ausente é **erro** (a pessoa pediu um arquivo específico); caminho default ausente é **ok** (usa `Config::default()`).

```rust
pub fn load(path: Option<&Path>) -> Result<Config, String> {
    let (path, required) = match path {
        Some(p) => (p.to_path_buf(), true),
        None => (default_path(), false),
    };
    match std::fs::read_to_string(&path) {
        Ok(raw) => parse(&raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => Ok(Config::default()),
        Err(e) => Err(format!("não deu para ler {}: {e}", path.display())),
    }
}
```

`default_path()`: `%APPDATA%\daily-tui\config.toml` no Windows; `$XDG_CONFIG_HOME/daily-tui/config.toml` ou `~/.config/...` fora dele — mesmo formato do `store::default_path()`, que já resolve isso para o banco.

`print_shell()`: uma linha `CHAVE="valor"` por item, chaves já seguras para `eval`:

```
PANEL_EMAIL=1
PANEL_JIRA=0
PANEL_AGENDA=1
PANEL_PULLS=1
PANEL_TASKS=1
ACCOUNT_IDS="work personal"
JIRA_CLOUD="empresa.atlassian.net"
TASKS_LIST=""
```

`init`/`get` sobre um `OnceLock<Config>`; `get()` cai no default quando ninguém chamou `init` — é o que faz os testes de outros módulos continuarem valendo sem preparar config:

```rust
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Fixa o config do processo. Chamado uma vez, no `main`, antes de qualquer
/// busca.
pub fn init(cfg: Config) {
    let _ = CONFIG.set(cfg);
}

/// Config do processo. Em teste, sem `init`, vale o default — que é o
/// comportamento histórico do painel.
pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}
```

- [ ] **Step 6: Rodar os testes**

Run: `cargo test config::`
Expected: PASS (8 testes).

- [ ] **Step 7: Argumentos no `main.rs`**

Sem `clap` — são quatro flags e `std::env::args` resolve:

```rust
mod config;

enum Cmd {
    Run { config: Option<PathBuf> },
    Init,
    PrintConfig { config: Option<PathBuf> },
    Help,
}
```

`--init` grava `include_str!("../config.example.toml")` em `config::default_path()`, criando o diretório, e **recusa sobrescrever** um arquivo existente (dizendo o caminho). `--print-config` imprime `print_shell()` e sai 0. Erro de config sai pelo stderr com código 1, **antes** de `ratatui::init()` — senão a mensagem morre junto com a tela alternativa.

- [ ] **Step 8: Verificar na mão**

```bash
cargo run -- --print-config
cargo run -- --config /caminho/que/nao/existe.toml   # deve falhar dizendo o caminho
```

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(config): load panels and accounts from a TOML file"
```

---

### Task 2: Contas vindas do config

**Files:**
- Modify: `src/data/mod.rs`
- Modify: `src/worker.rs`
- Modify: `src/store.rs`

**Interfaces:**
- Consumes: `config::get()`, `AccountCfg`.
- Produces:
  - `Account::slot_key(self) -> &'static str` — `"work"`/`"personal"`, **fixo**, para o banco.
  - `Account::cfg(self) -> Option<&'static AccountCfg>` — `None` quando o slot não foi configurado.
  - `Account::configured() -> Vec<Account>` — os slots que existem, na ordem do config.
  - `himalaya_name`, `gcalcli_dir`, `marker`, `primary_calendar` deixam de ser `const fn` e passam a ler o config.

- [ ] **Step 1: Teste primeiro**

```rust
#[test]
fn an_account_reads_its_names_from_the_config() {
    // Sem config, os nomes são os de sempre — quem já usava não sente.
    assert_eq!(Account::Work.himalaya_name(), "work");
    assert_eq!(Account::Personal.marker(), "[P]");
    assert_eq!(Account::Work.slot_key(), "work");
}

#[test]
fn the_slot_key_does_not_follow_a_renamed_account() {
    // O banco chaveia pastas pelo slot: renomear a conta no himalaya não pode
    // invalidar o cache.
    assert_eq!(Account::Personal.slot_key(), "personal");
    assert_ne!(Account::Personal.slot_key(), Account::Work.slot_key());
}
```

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test data::tests::an_account_reads`
Expected: falha (`slot_key` não existe).

- [ ] **Step 3: Implementar**

```rust
impl Account {
    /// Nome estável do slot, para persistência. **Não** muda com o config: o
    /// cache de pastas no banco é chaveado por ele.
    pub const fn slot_key(self) -> &'static str {
        match self {
            Account::Work => "work",
            Account::Personal => "personal",
        }
    }

    /// Índice do slot na lista de contas do config.
    const fn slot(self) -> usize {
        match self {
            Account::Work => 0,
            Account::Personal => 1,
        }
    }

    /// Config desta conta, ou `None` quando ela não existe no arquivo.
    pub fn cfg(self) -> Option<&'static crate::config::AccountCfg> {
        crate::config::get().accounts.get(self.slot())
    }

    /// Contas que existem, na ordem do config.
    pub fn configured() -> Vec<Account> {
        [Account::Work, Account::Personal]
            .into_iter()
            .filter(|a| a.cfg().is_some())
            .collect()
    }

    /// Nome da conta no himalaya.
    pub fn himalaya_name(self) -> &'static str {
        self.cfg().map(|c| c.id.as_str()).unwrap_or(self.slot_key())
    }

    /// Marcador curto exibido na lista (`[W]`, `[P]`).
    pub fn marker(self) -> String {
        match self.cfg() {
            Some(c) => format!("[{}]", c.label),
            None => format!("[{}]", self.slot_key().to_uppercase().chars().next().unwrap()),
        }
    }
}
```

`primary_calendar()` passa a ler `cfg().email`, mantendo o fallback nas variáveis `DAILY_TUI_*_EMAIL` (quem já as usa não quebra) e, por último, o placeholder.

`marker()` devolvendo `String` obriga a ajustar as chamadas em `ui.rs` (`theme.muted(e.account.marker())` continua compilando, `Account::marker` deixa de ser `const`).

- [ ] **Step 4: `worker.rs` sobre as contas configuradas**

`const ACCOUNTS: [Account; 2]` sai. `fetch_emails`/`fetch_agenda`/`refresh_folders` iteram `Account::configured()`:

```rust
fn fetch_emails() -> Result<Vec<email::EmailItem>, String> {
    let mut all = Vec::new();
    for account in Account::configured() {
        all.extend(email::fetch(account, config::get().email.limit)?);
    }
    email::sort_recent_first(&mut all);
    Ok(all)
}
```

- [ ] **Step 5: `store.rs` chaveando por slot**

`save_folders`/`folders` trocam `account.himalaya_name()` por `account.slot_key()`, e `Account::from_himalaya_name` vira `Account::from_slot_key` (as mesmas duas strings fixas, agora com o nome honesto).

- [ ] **Step 6: Rodar tudo**

Run: `cargo test`
Expected: PASS. Os testes de `store` que gravam `Account::Work` continuam valendo — a chave não mudou de valor, só de origem.

- [ ] **Step 7: Verificar com as contas de verdade**

```bash
cargo run -- --print-config   # ACCOUNT_IDS="work personal"
```

E um config de teste com uma conta só, para provar que o painel não erra pela conta que não existe:

```bash
printf '[[accounts]]\nid="personal"\nlabel="P"\nemail="%s"\ncalendar="personal"\n' "$DAILY_TUI_PERSONAL_EMAIL" > /tmp/uma-conta.toml
cargo run -- --config /tmp/uma-conta.toml --print-config
```

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(config): take account names, labels and calendars from the config"
```

---

### Task 3: Painéis ligáveis, com reflow

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui.rs`
- Modify: `src/worker.rs`

**Interfaces:**
- Consumes: `config::get().panels`.
- Produces:
  - `Panel::enabled() -> Vec<Panel>` — na ordem de leitura do layout.
  - `Panel::is_on(self) -> bool`
  - `Panel::next(self)` / `prev(self)` andam só entre os ligados.
  - `ui::columns(enabled: &[Panel]) -> (Vec<(Panel, u16)>, Vec<(Panel, u16)>)` — função pura, testável, que devolve os painéis de cada coluna com o peso de cada um.

- [ ] **Step 1: Testes primeiro**

Em `src/app.rs`:

```rust
#[test]
fn the_focus_cycle_skips_panels_that_are_off() {
    // Não dá para testar via config global (é OnceLock, e o processo é um só),
    // então o ciclo é testado pela função pura que ele usa.
    let on = vec![Panel::Email, Panel::Agenda];
    assert_eq!(Panel::next_in(&on, Panel::Email), Panel::Agenda);
    assert_eq!(Panel::next_in(&on, Panel::Agenda), Panel::Email, "o ciclo fecha");
    assert_eq!(Panel::prev_in(&on, Panel::Email), Panel::Agenda);
}

#[test]
fn a_single_panel_cycle_stays_put() {
    let on = vec![Panel::Tasks];
    assert_eq!(Panel::next_in(&on, Panel::Tasks), Panel::Tasks);
}
```

Em `src/ui.rs`:

```rust
#[test]
fn the_columns_keep_todays_proportions_when_everything_is_on() {
    let (left, right) = columns(&[
        Panel::Email, Panel::Jira, Panel::Agenda, Panel::Pulls, Panel::Tasks,
    ]);
    assert_eq!(left, vec![(Panel::Email, 60), (Panel::Jira, 40)]);
    assert_eq!(
        right,
        vec![(Panel::Agenda, 40), (Panel::Pulls, 30), (Panel::Tasks, 30)]
    );
}

#[test]
fn a_column_left_empty_gives_its_width_to_the_other() {
    let (left, right) = columns(&[Panel::Agenda, Panel::Tasks]);
    assert!(left.is_empty(), "nada na esquerda");
    assert_eq!(right.len(), 2);
}

#[test]
fn a_panel_that_is_off_is_not_drawn_and_the_rest_uses_the_space() {
    // Com Jira desligado, o painel de e-mail ocupa a coluna esquerda inteira.
    let app = test_app_with(&[Panel::Email, Panel::Agenda]);
    let out = render_to_string(&app, 120, 40);
    assert!(out.contains("E-MAILS"));
    assert!(out.contains("AGENDA"));
    assert!(!out.contains("JIRA"), "painel desligado não aparece");
    assert!(!out.contains("TAREFAS"));
}
```

`test_app_with(&[Panel])` é um helper novo nos testes de `ui`: monta um `App` e sobrescreve o campo de painéis ligados (ver Step 3), sem depender do `OnceLock`.

- [ ] **Step 2: Rodar e ver falhar**

Run: `cargo test the_focus_cycle_skips`
Expected: falha (`next_in` não existe).

- [ ] **Step 3: Implementar**

`App` ganha um campo com os painéis ligados, lido do config na construção — assim o teste injeta um conjunto sem tocar no `OnceLock`:

```rust
pub struct App {
    /// Painéis ligados, na ordem de leitura do layout. Vem do config; os testes
    /// sobrescrevem.
    pub panels: Vec<Panel>,
    ...
}
```

`Panel::ORDER` fixa a ordem de leitura (`Email, Jira, Agenda, Pulls, Tasks`). `Panel::enabled()` filtra `ORDER` pelo `config::get().panels`. `next_in`/`prev_in` andam sobre a lista dada:

```rust
impl Panel {
    /// Ordem de leitura do layout: coluna esquerda e depois a direita.
    pub const ORDER: [Panel; 5] =
        [Panel::Email, Panel::Jira, Panel::Agenda, Panel::Pulls, Panel::Tasks];

    /// Próximo painel ligado, circulando.
    pub fn next_in(on: &[Panel], from: Panel) -> Panel {
        Self::step_in(on, from, 1)
    }

    fn step_in(on: &[Panel], from: Panel, delta: isize) -> Panel {
        if on.is_empty() {
            return from;
        }
        let at = on.iter().position(|p| *p == from).unwrap_or(0) as isize;
        on[(at + delta).rem_euclid(on.len() as isize) as usize]
    }
}
```

O foco inicial é `app.panels[0]`. `Tab`/`Shift+Tab` passam a chamar `next_in(&self.panels, self.focus)`.

Em `ui.rs`, a coluna de cada painel e o peso são a única tabela:

```rust
/// Coluna e peso de cada painel. Os pesos são os de hoje; com painel
/// desligado eles são normalizados sobre os que sobraram, e é isso que faz o
/// layout se redistribuir sozinho.
const LAYOUT: [(Panel, Side, u16); 5] = [
    (Panel::Email, Side::Left, 60),
    (Panel::Jira, Side::Left, 40),
    (Panel::Agenda, Side::Right, 40),
    (Panel::Pulls, Side::Right, 30),
    (Panel::Tasks, Side::Right, 30),
];
```

`render_body` usa `Constraint::Fill(peso)` — que já normaliza — e, quando uma coluna fica vazia, dá 100% da largura à outra:

```rust
let (left, right) = columns(&app.panels);
let cols = match (left.is_empty(), right.is_empty()) {
    (false, false) => split_h(area, &[50, 50]),
    (true, false) => [Rect::ZERO, area],
    (false, true) => [area, Rect::ZERO],
    (true, true) => return, // config inválido não chega aqui (Task 1 valida)
};
```

Cada painel é desenhado por uma função que já existe; o `match` que escolhe qual chamar entra num laço sobre a coluna.

- [ ] **Step 4: Rodar os testes**

Run: `cargo test`
Expected: PASS, incluindo os testes de render que já existem (com tudo ligado, o layout é idêntico ao de hoje — é o que o primeiro teste do Step 1 garante).

- [ ] **Step 5: O worker só busca o que está ligado**

```rust
fn refresh_all(ui: &ProgramHandle<Msg>, jira_filter: JiraFilter, mentions_loaded: bool) {
    let panels = &config::get().panels;
    if panels.email {
        let _ = ui.send(Msg::EmailsLoaded(fetch_emails()));
    }
    if panels.agenda {
        let _ = ui.send(Msg::AgendaLoaded(fetch_agenda()));
    }
    ...
}
```

Mesmo tratamento para: `refresh_folders` (só com e-mail ligado), o prefetch de corpo (`App::prefetch_cursor_body` retorna cedo sem e-mail), e as menções do Jira (`open_notifications` não pede menção com Jira desligado — e a central passa a mostrar "Nada pedindo sua atenção" em vez de ficar em "Buscando…").

- [ ] **Step 6: Verificar na mão, com config de verdade**

```bash
printf '[panels]\ntasks = false\npulls = false\n' > /tmp/sem-tarefas.toml
cargo run -- --config /tmp/sem-tarefas.toml
```

Confirmar: TAREFAS e PRs não aparecem, `Tab` circula só entre os três restantes, o layout preenche a tela, e **nenhum** processo `mstodo`/`ghpending` é executado (checar com o Gerenciador de Tarefas ou `Get-Process mstodo`).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(config): turn panels on and off, reflowing the layout"
```

---

### Task 4: O config alimenta os helpers

**Files:**
- Modify: `src/data/mod.rs`
- Modify: `src/data/jira.rs`, `src/data/tasks.rs` (apenas a chamada que monta o comando)

**Interfaces:**
- Consumes: `config::get().jira`, `config::get().tasks`.
- Produces: `helper_command(program)` passa a injetar o env do config.

**Por quê:** hoje `jira` e `mstodo` só leem variáveis de ambiente, então quem clona precisa exportar quatro variáveis num shell rc — e no Windows isso vive dentro do launcher PowerShell do autor. Com o config, a pessoa escreve `cloud` e `email` uma vez no TOML.

- [ ] **Step 1: Teste primeiro**

```rust
#[test]
fn the_helper_gets_the_jira_settings_from_the_config() {
    let cmd = helper_command("jira");
    let envs: Vec<_> = cmd.get_envs().collect();
    // Sem config, nada é injetado: o ambiente de quem já usa manda.
    assert!(!envs.iter().any(|(k, _)| *k == std::ffi::OsStr::new("JIRA_CLOUD")));
}
```

- [ ] **Step 2: Implementar**

```rust
/// Injeta no helper o que o config souber. Valor vazio no config = não injeta,
/// e o helper cai no ambiente do processo — que é como o launcher do Windows e
/// o `.bashrc` de quem já usa o painel entregam essas variáveis.
fn helper_env(cmd: &mut std::process::Command, program: &str) {
    let cfg = crate::config::get();
    let pairs: &[(&str, &str)] = match program {
        "jira" => &[("JIRA_CLOUD", &cfg.jira.cloud), ("JIRA_EMAIL", &cfg.jira.email)],
        "mstodo" => &[
            ("DAILY_TUI_TODO_CLIENT_ID", &cfg.tasks.client_id),
            ("DAILY_TUI_TODO_LIST", &cfg.tasks.list),
        ],
        _ => &[],
    };
    for (key, value) in pairs {
        if !value.is_empty() {
            cmd.env(key, value);
        }
    }
}
```

Chamado por `helper_command` antes de devolver o `Command`. Token nenhum entra aqui — `JIRA_TOKEN` e `GITHUB_TOKEN` continuam vindo só do ambiente, e o `config.example.toml` diz isso.

- [ ] **Step 3: Rodar e verificar de verdade**

Run: `cargo test data::tests`
Expected: PASS.

```bash
printf '[jira]\ncloud = "%s"\nemail = "%s"\n' "$JIRA_CLOUD" "$JIRA_EMAIL" > /tmp/jira.toml
env -u JIRA_CLOUD -u JIRA_EMAIL cargo run -- --config /tmp/jira.toml
```
Esperado: o painel de Jira carrega mesmo sem as variáveis no ambiente (só `JIRA_TOKEN` continua sendo necessário).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(config): feed jira and mstodo settings to the helpers"
```

---

### Task 5: A documentação de autenticação, por painel

**Files:**
- Modify: `README.md`
- Modify: `scripts/setup-auth.sh`
- Modify: `scripts/daily-tui-launch.ps1` e `scripts/daily-tui.config.example.ps1` (o caminho 1Password vira opcional)

**Interfaces:** nenhuma de código. O contrato aqui é: **cada painel se autentica sem ler a seção dos outros.**

- [ ] **Step 1: Abrir o README com o caminho curto**

Uma seção "Comece aqui" de cinco passos, antes de qualquer detalhe:

```markdown
## Comece aqui

1. `scripts/install.sh` (Linux/mac) — instala o binário e as CLIs.
2. `daily-tui --init` — escreve o config no lugar certo do seu SO.
3. Abra o config e **desligue o que você não usa**. Cada painel desligado é
   uma autenticação que você não precisa fazer.
4. Autentique só os painéis que sobraram: a tabela abaixo diz o que cada um
   pede e onde está o passo a passo.
5. `scripts/setup-auth.sh check` — ele lê o seu config e cobra só o que falta.
```

- [ ] **Step 2: A tabela que substitui a leitura do README inteiro**

```markdown
| Painel | Ferramenta | Credencial | Como testar | Desligar |
|---|---|---|---|---|
| E-mails | `himalaya` | OAuth2 do Gmail (navegador), token no keychain | `himalaya envelope list -a work` | `panels.email = false` |
| Jira | helper `jira` | API token da Atlassian em `JIRA_TOKEN` | `jira issues` | `panels.jira = false` |
| Agenda | `gcalcli` | OAuth do Google Cloud (projeto seu) | `cal-work agenda` | `panels.agenda = false` |
| PRs | `ghpending` | PAT do GitHub em `GITHUB_TOKEN` | `ghpending` | `panels.pulls = false` |
| Tarefas | helper `mstodo` | device code da Microsoft | `mstodo list` | `panels.tasks = false` |
```

- [ ] **Step 3: Uma seção por painel, autocontida**

Reorganizar o que já existe em "Configuração das contas" para que cada seção tenha, na ordem: **o que é**, **o que você precisa ter antes**, **os comandos exatos**, **como saber que deu certo**, **os erros comuns**. Cada uma abre com a linha de escape:

```markdown
> Pule esta seção se você deixou `panels.agenda = false`.
```

O conteúdo técnico não precisa ser reescrito — os cinco fluxos já estão documentados. O trabalho é: cortar o que é específico do autor, tornar cada seção independente (hoje a de agenda depende de ter lido a de e-mail), e explicitar o que é **opcional** (o 1Password, o `ortie`).

- [ ] **Step 4: O `check` só cobra painel ligado**

```bash
# Config do usuário: quais painéis existem. Sem o binário compilado, cobra tudo.
if command -v daily-tui >/dev/null 2>&1; then
  eval "$(daily-tui --print-config)"
else
  PANEL_EMAIL=1 PANEL_JIRA=1 PANEL_AGENDA=1 PANEL_PULLS=1 PANEL_TASKS=1
fi

check_panel() {  # nome, ligado?, comando de teste
  if [[ "$2" != 1 ]]; then
    info "$1: desligado no config"
    return
  fi
  ...
}
```

- [ ] **Step 5: O launcher do Windows deixa de ser obrigatório**

`daily-tui-launch.ps1` existe para puxar tokens do 1Password. Para quem clona, isso é uma receita entre outras. Duas mudanças:

- o README passa a documentar o caminho simples primeiro (variáveis no perfil do PowerShell), e o launcher como "se você usa 1Password";
- o `daily-tui.config.example.ps1` ganha um comentário no topo dizendo que o arquivo só é necessário para esse caminho.

- [ ] **Step 6: Verificar seguindo o próprio README**

O teste honesto é um usuário novo. O mais perto disso sem outra máquina:

```bash
printf '[panels]\nemail=false\njira=false\nagenda=false\npulls=true\ntasks=false\n' > /tmp/so-prs.toml
cargo run -- --config /tmp/so-prs.toml --print-config
scripts/setup-auth.sh check   # deve dizer "desligado no config" para quatro painéis
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "docs: per-panel authentication guide and a config-aware doctor"
```

---

### Task 6 (opcional): Instalação no Windows

**Files:**
- Create: `scripts/install.ps1`

**Por quê:** o `install.sh` é bash. Hoje, no Windows, a instalação está documentada como passos manuais (compilar, copiar os shims `.cmd`, instalar as CLIs). Se as pessoas com quem você vai compartilhar estão no Windows, isso é o maior atrito que sobra — maior do que a autenticação.

**Recomendação:** só faça esta task se alguém for de fato instalar no Windows. Ela não é pré-requisito de nenhuma outra, e o README continua correto sem ela.

- [ ] **Step 1: Espelhar o `install.sh`**

Mesmos passos, com `winget`/`scoop` no lugar do gerenciador de pacotes, `cargo install` do himalaya e do ghpending, `uv tool install` do gcalcli, `cargo build --release`, e a cópia dos shims (`scripts\jira*`, `scripts\mstodo*`) para `~\.local\bin`, criando o diretório.

- [ ] **Step 2: Rodar numa pasta limpa e conferir**

```powershell
scripts\install.ps1 -SkipSystem
daily-tui --print-config
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "chore: add a Windows install script"
```

---

## Self-Review

**Cobertura do pedido**

| Pedido | Onde |
|---|---|
| "eles precisam saber como autenticar em cada coisa" | Task 5 (tabela + seção por painel + `check` por painel) |
| "baseado em config quais painéis eles querem ou não" | Task 1 (schema) + Task 3 (foco, layout, worker) |
| "alguém não usa as tarefas, fica de fora" | Task 3, Step 5 e 6 — painel desligado não desenha, não foca e não executa o helper |
| Compartilhar um repo público sem vazar dado seu | Task 0 |

**O que este plano deliberadamente não faz**

- **Três ou mais contas.** Ficou em duas vagas configuráveis, por decisão sua. Se aparecer alguém com três caixas, o custo é trocar o `Account` de enum para id — toca e-mail, agenda, banco, chave de e-mail e marcadores.
- **Tema/cores no config.** Ninguém pediu.
- **Escolher a posição de cada painel.** O reflow é automático, também por decisão sua.

**Riscos**

1. `Account::marker()` deixa de ser `const fn` e passa a devolver `String` — é o único ponto do Task 2 que respinga em `ui.rs`. Alocar uma `String` por linha renderizada é irrelevante nesta escala (dezenas de linhas por frame).
2. `deny_unknown_fields` transforma erro de digitação em falha de arranque. É de propósito, e a mensagem do `toml` nomeia o campo — mas é a mudança com mais chance de irritar alguém no primeiro uso. Vale o teste que já está no Task 1, Step 3.
3. O `OnceLock` do config é global. Isso é o que evita passar `&Config` por doze assinaturas, mas obriga os testes de painel a injetar o conjunto pelo campo `App::panels` em vez do config — está explícito no Task 3, Step 1.
