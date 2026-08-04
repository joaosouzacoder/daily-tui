# Jira Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transformar o painel de Jira de texto colorido morto em uma lista interativa: issues estruturadas, filtro por assignee/reporter, abrir no navegador, agrupamento por pai e visão de menções.

**Architecture:** Um helper `scripts/jira` (Python via uv) substitui as duas implementações do `jirapending` (bash e PowerShell) e emite JSON. O painel Rust passa de `PanelData<String>` para `PanelData<JiraItem>`, com as três visões derivadas em memória do mesmo conjunto — só a visão de menções faz consulta própria.

**Tech Stack:** Rust (ratatui, serde_json), Python 3.10+ via `uv` (PEP 723) com `requests`, Jira Cloud REST v3 (`POST /rest/api/3/search/jql`).

**Spec:** `docs/superpowers/specs/2026-08-03-panel-interactions-design.md`

## Global Constraints

- Comentários e mensagens de usuário em **português**; mensagens de commit em **inglês**.
- **Fixtures: forma do real, conteúdo inventado.** O repo é público e as issues
  são da empresa. Derive a estrutura da saída real (formato de chave, nomes de
  status como a API devolve, `parent` presente e ausente), mas use chaves e
  resumos inventados e o domínio `example.atlassian.net`. Copiar título interno
  para arquivo versionado conta a estranhos o que a empresa faz.
- Erros dos helpers: **uma linha no stderr**, sem traceback. Mensagens exatas: `defina JIRA_EMAIL` (idem `JIRA_CLOUD`, `JIRA_TOKEN`), `Jira <status>: <mensagem>`, `filtro inválido: <valor>`.
- JQL por modo, verbatim do spec:
  - `assignee` → `assignee = currentUser() AND statusCategory != Done`
  - `reporter` → `reporter = currentUser() AND statusCategory = 'In Progress'`
  - `both` → `(assignee = currentUser() AND statusCategory != Done) OR (reporter = currentUser() AND statusCategory = 'In Progress')`
  - ordenação comum: `ORDER BY project ASC, updated DESC`
  - `JIRA_JQL`, se definida, **substitui a consulta inteira** e o `--filter` não tem efeito.
- Menções: `(comment ~ "<accountId>" OR description ~ "<accountId>") AND updated >= -30d ORDER BY updated DESC`, com o `accountId` vindo de `GET /rest/api/3/myself`.
- `url` é montada pelo helper: `https://<JIRA_CLOUD>/browse/<key>`.
- Cabeçalho do painel: `JIRA · <filtro> · [issues] por-pai menções`, filtro em `minhas` / `relator` / `ambas` / `jql`.
- Teclas do painel Jira: `Enter` abre no navegador, `f` circula o filtro, `p` visão por pai, `n` visão menções, `Esc` volta para issues.
- O helper Python é executado via `super::helper_command` com `super::force_utf8_stdout` aplicado, e os erros passam por `super::stderr_summary`.
- `cargo test` e `cargo clippy --all-targets` sem regressão. Baseline atual: **70 testes**, e exatamente **3 warnings pré-existentes** (`src/app.rs:115`, `src/app.rs:119`, `src/data/email.rs:90`) que não devem ser corrigidos aqui.
- Não rodar `cargo build --release` nem o TUI: pode haver um `daily-tui.exe` em execução segurando o binário.

## File Structure

| Arquivo | Responsabilidade |
| --- | --- |
| `scripts/jira` (criar) | Única coisa que conhece a API do Jira: auth, JQL, paginação, mapeamento para o contrato JSON. |
| `scripts/jira.cmd` (criar) | Shim Windows: `uv run --script`. |
| `src/data/jira.rs` (reescrever) | `JiraItem`/`JiraParent`, parse do JSON, e as funções puras que agrupam itens em linhas. Nada de renderização. |
| `src/app.rs` (modificar) | Estado do painel: filtro ativo, visão ativa, cursor sobre issues. Teclas do painel Jira. |
| `src/ui.rs` (modificar) | Render do painel: cabeçalho com filtro/visão, cabeçalhos de grupo, destaque do cursor. |
| `src/msg.rs`, `src/worker.rs` (modificar) | Comandos e mensagens novas: buscar issues com filtro, buscar menções. |
| `scripts/jirapending`, `.ps1`, `.cmd` (apagar) | Substituídos pelo helper novo. |

**A decisão estrutural que atravessa o plano:** hoje `PanelData.cursor` indexa itens e cada item é exatamente uma linha renderizada. Com cabeçalhos de grupo isso deixa de valer — as linhas passam a ser cabeçalhos **ou** issues. A solução é uma função pura que achata itens em linhas (`Vec<JiraRow>`) e um mapeamento do índice do item para o índice da linha, que é o que a função `window` de `ui.rs` precisa. Isso vive em `jira.rs`, testável sem terminal.

---

### Task 1: Helper `jira`

Deliverable: `jira issues --filter both` e `jira mentions` devolvem JSON válido do Jira real.

**Files:**
- Create: `scripts/jira`
- Create: `scripts/jira.cmd`

**Interfaces:**
- Consumes: nada.
- Produces: a CLI `jira` com dois subcomandos. `issues [--filter assignee|reporter|both]` (default `assignee`) e `mentions` escrevem no stdout um array JSON de objetos:
  `{"key": str, "summary": str, "status": str, "project": str, "url": str, "parent": {"key": str, "summary": str} | null}`.
  Qualquer falha: uma linha no stderr e exit 1.

- [ ] **Step 1: Criar `scripts/jira`**

```python
#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "requests",
# ]
# ///
"""jira — CLI mínima do Jira Cloud para o painel do daily-tui.

Subcomandos:
  jira issues [--filter assignee|reporter|both]   # minhas issues, com o pai de cada uma
  jira mentions                                   # issues onde me mencionaram (30 dias)

Configuração (as mesmas do jirapending que este helper substitui):
  JIRA_EMAIL — e-mail da conta Atlassian        (obrigatório)
  JIRA_CLOUD — domínio, ex.: empresa.atlassian.net (obrigatório)
  JIRA_TOKEN — API token                        (obrigatório)
  JIRA_JQL   — substitui a consulta inteira do `issues`; o --filter passa a
               não ter efeito (precedência documentada no README)

Todo erro sai como UMA linha no stderr: o painel mostra só o resumo (ver
`stderr_summary` em src/data/mod.rs), então traceback não ajuda.
"""
import json
import os
import re
import sys
from functools import cache

import requests

TIMEOUT = 30
ORDER = "ORDER BY project ASC, updated DESC"
FIELDS = ["summary", "status", "project", "parent"]

# Cláusulas por modo de filtro. O modo `reporter` NÃO usa `statusCategory != Done`
# como os outros: essa combinação devolve mais de 100 issues, quase todas de um
# projeto só (medido em 2026-08-03). Ver o spec, seção "Revisões guiadas por dado".
FILTERS = {
    "assignee": "assignee = currentUser() AND statusCategory != Done",
    "reporter": "reporter = currentUser() AND statusCategory = 'In Progress'",
    "both": "(assignee = currentUser() AND statusCategory != Done)"
            " OR (reporter = currentUser() AND statusCategory = 'In Progress')",
}


def die(msg):
    """Sai com código 1 e a mensagem no stderr."""
    sys.exit(str(msg))


@cache
def config():
    """(email, cloud, token), validados. Aceita JIRA_CLOUD com ou sem esquema."""
    email = os.environ.get("JIRA_EMAIL", "").strip()
    cloud = os.environ.get("JIRA_CLOUD", "").strip()
    token = os.environ.get("JIRA_TOKEN", "").strip()
    for name, value in (("JIRA_EMAIL", email), ("JIRA_CLOUD", cloud), ("JIRA_TOKEN", token)):
        if not value:
            die(f"defina {name}")
    cloud = re.sub(r"^https?://", "", cloud).rstrip("/")
    return email, cloud, token


def error_message(resp):
    try:
        body = resp.json()
        msgs = body.get("errorMessages") or []
        if msgs:
            return "; ".join(msgs)
        return json.dumps(body.get("errors") or body)[:200]
    except ValueError:
        return resp.text[:200]


def api(method, path, **kw):
    email, cloud, token = config()
    resp = requests.request(method, f"https://{cloud}{path}",
                            auth=(email, token), timeout=TIMEOUT, **kw)
    if not resp.ok:
        die(f"Jira {resp.status_code}: {error_message(resp).replace(chr(10), ' ')}")
    return resp


def search(jql):
    """Todas as issues da consulta, seguindo a paginação por nextPageToken."""
    issues, page_token = [], None
    while True:
        body = {"jql": jql, "fields": FIELDS, "maxResults": 100}
        if page_token:
            body["nextPageToken"] = page_token
        page = api("POST", "/rest/api/3/search/jql", json=body).json()
        issues.extend(page.get("issues") or [])
        page_token = page.get("nextPageToken")
        if not page_token:
            break
    return issues


def to_item(issue, cloud):
    """Converte a issue crua do Jira no contrato que o painel espera."""
    fields = issue.get("fields") or {}
    parent = fields.get("parent")
    item = {
        "key": issue["key"],
        "summary": (fields.get("summary") or "").strip(),
        "status": ((fields.get("status") or {}).get("name") or "").strip(),
        "project": ((fields.get("project") or {}).get("key") or "").strip(),
        "url": f"https://{cloud}/browse/{issue['key']}",
        "parent": None,
    }
    if parent:
        item["parent"] = {
            "key": parent.get("key", ""),
            "summary": ((parent.get("fields") or {}).get("summary") or "").strip(),
        }
    return item


def emit(issues):
    _, cloud, _ = config()
    print(json.dumps([to_item(i, cloud) for i in issues], ensure_ascii=False))


def do_issues(mode):
    override = os.environ.get("JIRA_JQL", "").strip()
    if override:
        jql = override
    else:
        if mode not in FILTERS:
            die(f"filtro inválido: {mode}")
        jql = f"{FILTERS[mode]} {ORDER}"
    emit(search(jql))


def do_mentions():
    account_id = api("GET", "/rest/api/3/myself").json().get("accountId")
    if not account_id:
        die("Jira não devolveu o accountId em /myself")
    emit(search(
        f'(comment ~ "{account_id}" OR description ~ "{account_id}")'
        " AND updated >= -30d ORDER BY updated DESC"))


def main():
    args = sys.argv[1:]
    if not args:
        die("uso: jira {issues [--filter assignee|reporter|both]|mentions}")
    cmd, rest = args[0], args[1:]
    if cmd == "issues":
        mode = "assignee"
        if rest:
            if rest[0] != "--filter" or len(rest) < 2:
                die("uso: jira issues [--filter assignee|reporter|both]")
            mode = rest[1]
        do_issues(mode)
    elif cmd == "mentions":
        do_mentions()
    else:
        die(f"comando desconhecido: {cmd}")


if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except Exception as exc:  # honra o contrato de uma linha no stderr
        die(f"{type(exc).__name__}: {exc}")
```

- [ ] **Step 2: Criar `scripts/jira.cmd`**

```bat
@echo off
rem Shim Windows: o daily-tui chama `jira`; roda o script Python via uv (PEP 723).
uv run --script "%~dp0jira" %*
```

- [ ] **Step 3: Marcar o script como executável e verificar o erro de config**

```bash
git update-index --add --chmod=+x scripts/jira
JIRA_EMAIL= uv run --script scripts/jira issues; echo "exit=$?"
```

Esperado: stderr com exatamente `defina JIRA_EMAIL` e `exit=1`.

- [ ] **Step 4: Verificar filtro inválido**

```bash
uv run --script scripts/jira issues --filter chutado; echo "exit=$?"
```

Esperado: `filtro inválido: chutado`, `exit=1`. (Note que essa validação acontece
antes de qualquer HTTP, então não precisa de credencial.)

- [ ] **Step 5: Smoke test contra o Jira real**

Exportar as três variáveis (no Windows o launcher já as tem; num shell, pegue do
`daily-tui.config.ps1` e do 1Password como `daily-tui-launch.ps1` faz), então:

```bash
for m in assignee reporter both; do
  n=$(uv run --script scripts/jira issues --filter $m | python -c "import json,sys;print(len(json.load(sys.stdin)))")
  echo "$m -> $n issues"
done
uv run --script scripts/jira mentions | python -c "
import json,sys
d=json.load(sys.stdin)
print(f'mentions -> {len(d)} issues')
i=d[0]
assert sorted(i)==['key','parent','project','status','summary','url'], sorted(i)
print('contrato ok; parent do primeiro:', i['parent'])
"
```

Esperado, com os números medidos em 2026-08-03 (podem ter mudado, o que importa é
a ordem de grandeza): `assignee -> 6`, `reporter -> 7`, `both -> ~13`,
`mentions -> 4`. As seis chaves exatas do contrato, e `parent` sendo `null` ou um
objeto com `key` e `summary`.

- [ ] **Step 6: Guardar a saída real para a Task 2**

```bash
uv run --script scripts/jira issues --filter both > /tmp/jira-real.json
```

É a fonte da fixture do teste da Task 2 — o teste tem de sair de saída real.

- [ ] **Step 7: Commit**

```bash
git add scripts/jira scripts/jira.cmd
git commit -m "feat(jira): add jira helper emitting structured JSON"
```

---

### Task 2: Painel estruturado, filtro e abrir no navegador

Deliverable: o painel lista issues agrupadas por projeto com cursor, `f` circula o filtro e `Enter` abre a issue no navegador.

**Files:**
- Modify: `src/data/jira.rs` (reescrita completa)
- Modify: `src/app.rs` (estado do painel, teclas)
- Modify: `src/ui.rs` (`render_jira`)
- Modify: `src/msg.rs`, `src/worker.rs`

**Interfaces:**
- Consumes: a CLI `jira` da Task 1 e a saída real em `/tmp/jira-real.json`.
- Produces:
  ```rust
  pub struct JiraItem { pub key: String, pub summary: String, pub status: String,
                        pub project: String, pub url: String, pub parent: Option<JiraParent> }
  pub struct JiraParent { pub key: String, pub summary: String }
  pub enum JiraFilter { Assignee, Reporter, Both }   // Display: minhas/relator/ambas
  pub enum JiraRow { Header(String), Issue(usize) }  // usize = índice em items
  pub fn parse_issues(raw: &str) -> Result<Vec<JiraItem>, String>
  pub fn rows_by_project(items: &[JiraItem]) -> Vec<JiraRow>
  pub fn row_of_item(rows: &[JiraRow], item: usize) -> usize
  pub fn fetch(filter: JiraFilter) -> Result<Vec<JiraItem>, String>
  pub fn open_url(url: &str) -> Result<(), String>
  ```

- [ ] **Step 1: Escrever os testes que falham — parse e agrupamento**

Em `src/data/jira.rs`, substituindo o `mod tests` atual (os testes de
`parse_jira` morrem com a função):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Saída real de `jira issues --filter both` (2026-08-03), reduzida a três
    // issues: uma com pai, duas sem — o suficiente para exercitar o agrupamento.
    const REAL: &str = r#"[
      {"key":"ENG-101","summary":"[Painel] - Melhorias no dashboard","status":"Em andamento",
       "project":"ENG","url":"https://x.atlassian.net/browse/ENG-101",
       "parent":{"key":"ENG-42","summary":"Engenharia de Plataforma"}},
      {"key":"OPS-55","summary":"Revisar rotação de chaves","status":"Em Andamento",
       "project":"OPS","url":"https://x.atlassian.net/browse/OPS-55","parent":null},
      {"key":"OPS-56","summary":"Outra da OPS","status":"Backlog",
       "project":"OPS","url":"https://x.atlassian.net/browse/OPS-56","parent":null}
    ]"#;

    #[test]
    fn parses_the_real_contract() {
        let items = parse_issues(REAL).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].key, "ENG-101");
        assert_eq!(items[0].status, "Em andamento");
        assert_eq!(items[0].url, "https://x.atlassian.net/browse/ENG-101");
        assert_eq!(items[0].parent.as_ref().unwrap().key, "ENG-42");
        assert!(items[1].parent.is_none());
    }

    #[test]
    fn null_parent_and_missing_fields_are_tolerated() {
        let items = parse_issues(r#"[{"key":"A-1","summary":"s","status":"","project":"A","url":"u","parent":null}]"#).unwrap();
        assert!(items[0].parent.is_none());
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_issues("nope").is_err());
    }

    #[test]
    fn groups_by_project_with_one_header_each() {
        let items = parse_issues(REAL).unwrap();
        let rows = rows_by_project(&items);
        // TT vem antes de SEA porque a ordem dos itens é preservada.
        assert!(matches!(&rows[0], JiraRow::Header(h) if h == "TT"));
        assert!(matches!(rows[1], JiraRow::Issue(0)));
        assert!(matches!(&rows[2], JiraRow::Header(h) if h == "SEA"));
        assert!(matches!(rows[3], JiraRow::Issue(1)));
        assert!(matches!(rows[4], JiraRow::Issue(2)));
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn row_of_item_finds_the_line_of_each_issue() {
        let items = parse_issues(REAL).unwrap();
        let rows = rows_by_project(&items);
        assert_eq!(row_of_item(&rows, 0), 1);
        assert_eq!(row_of_item(&rows, 2), 4);
    }

    #[test]
    fn empty_input_yields_no_rows() {
        assert!(rows_by_project(&[]).is_empty());
    }
}
```

- [ ] **Step 2: Rodar e confirmar que falha**

```bash
cargo test --lib data::jira
```

Esperado: FAIL na compilação — `parse_issues`, `rows_by_project`, `row_of_item`,
`JiraRow` e `JiraItem` não existem ainda.

- [ ] **Step 3: Reescrever `src/data/jira.rs`**

```rust
//! Issues do Jira via a CLI `jira`, que emite JSON estruturado.
//!
//! Diferente do painel antigo (que recebia texto colorido e só rolava), aqui os
//! itens são estruturados: o painel precisa saber qual issue está sob o cursor
//! para abri-la no navegador e para reagrupar as linhas por pai.

use serde::Deserialize;

/// Uma issue do Jira, já normalizada para exibição.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JiraItem {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub project: String,
    /// Link para o navegador, montado pelo helper.
    pub url: String,
    /// Épico ou iniciativa acima desta issue; `None` quando é solta.
    #[serde(default)]
    pub parent: Option<JiraParent>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JiraParent {
    pub key: String,
    pub summary: String,
}

/// Modo de filtro do painel; circulado pela tecla `f`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JiraFilter {
    #[default]
    Assignee,
    Reporter,
    Both,
}

impl JiraFilter {
    /// Valor passado no `--filter` do helper.
    pub const fn flag(self) -> &'static str {
        match self {
            JiraFilter::Assignee => "assignee",
            JiraFilter::Reporter => "reporter",
            JiraFilter::Both => "both",
        }
    }

    /// Rótulo exibido no cabeçalho do painel.
    pub const fn label(self) -> &'static str {
        match self {
            JiraFilter::Assignee => "minhas",
            JiraFilter::Reporter => "relator",
            JiraFilter::Both => "ambas",
        }
    }

    /// Próximo modo no ciclo da tecla `f`.
    pub const fn next(self) -> Self {
        match self {
            JiraFilter::Assignee => JiraFilter::Reporter,
            JiraFilter::Reporter => JiraFilter::Both,
            JiraFilter::Both => JiraFilter::Assignee,
        }
    }
}

/// Uma linha renderizada do painel.
///
/// O cursor do painel indexa **issues**, não linhas: os cabeçalhos de grupo não
/// são selecionáveis. A renderização usa `row_of_item` para traduzir o cursor na
/// linha correspondente antes de calcular a rolagem.
#[derive(Debug, Clone, PartialEq)]
pub enum JiraRow {
    Header(String),
    Issue(usize),
}

/// Faz o parse da saída de `jira issues` / `jira mentions`.
pub fn parse_issues(raw: &str) -> Result<Vec<JiraItem>, String> {
    serde_json::from_str(raw).map_err(|e| format!("JSON inválido do jira: {e}"))
}

/// Agrupa por projeto, preservando a ordem em que as issues vieram.
pub fn rows_by_project(items: &[JiraItem]) -> Vec<JiraRow> {
    let mut rows = Vec::new();
    let mut current: Option<&str> = None;
    for (i, item) in items.iter().enumerate() {
        if current != Some(item.project.as_str()) {
            rows.push(JiraRow::Header(item.project.clone()));
            current = Some(item.project.as_str());
        }
        rows.push(JiraRow::Issue(i));
    }
    rows
}

/// Índice da linha que mostra a issue `item`; 0 quando não estiver nas linhas.
pub fn row_of_item(rows: &[JiraRow], item: usize) -> usize {
    rows.iter()
        .position(|r| matches!(r, JiraRow::Issue(i) if *i == item))
        .unwrap_or(0)
}

/// Roda `jira issues --filter <modo>` e devolve as issues.
pub fn fetch(filter: JiraFilter) -> Result<Vec<JiraItem>, String> {
    parse_issues(&run(&["issues", "--filter", filter.flag()])?)
}

/// Roda `jira <args...>` e devolve o stdout (ou um erro com o stderr).
fn run(args: &[&str]) -> Result<String, String> {
    let mut cmd = super::helper_command("jira");
    // O helper serializa com `ensure_ascii=False`, então resumos acentuados
    // dependem da codificação do stdout (veja `force_utf8_stdout`).
    super::force_utf8_stdout(&mut cmd);
    let output = cmd
        .args(args)
        .output()
        .map_err(|e| format!("falha ao executar jira: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("jira falhou: {}", super::stderr_summary(&err)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Abre uma URL no navegador do sistema.
///
/// No Windows via `cmd /C start ""` — o primeiro argumento vazio é o título da
/// janela, sem ele o `start` interpreta a URL como título. No Unix, `xdg-open`.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.status()
        .map_err(|e| format!("falha ao abrir o navegador: {e}"))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(format!("navegador saiu com {s}"))
            }
        })
}
```

- [ ] **Step 4: Rodar os testes de `jira.rs`**

```bash
cargo test --lib data::jira
```

Esperado: os 6 testes passando. A compilação do resto ainda vai falhar, porque
`app.rs` espera `PanelData<String>` — é o próximo passo.

- [ ] **Step 5: Trocar o tipo do painel e guardar o filtro em `app.rs`**

Em `src/app.rs`, no `pub struct App`, trocar a linha do jira e acrescentar o
filtro logo depois:

```rust
    pub jira: PanelData<JiraItem>,
    /// Modo de filtro do painel de Jira, circulado pela tecla `f`.
    pub jira_filter: JiraFilter,
```

No construtor (onde os outros campos são inicializados), acrescentar
`jira_filter: JiraFilter::default(),`. Ajustar o `use` no topo do arquivo para
importar `JiraFilter` e `JiraItem` de `crate::data::jira`.

- [ ] **Step 6: Escrever o teste de teclas que falha**

No `mod tests` de `src/app.rs`, seguindo o estilo `app.update(key(...))` que já
existe lá:

```rust
    #[test]
    fn f_cycles_the_jira_filter_only_when_jira_is_focused() {
        let mut app = test_app();
        assert_eq!(app.jira_filter, JiraFilter::Assignee);

        // Sem foco no Jira, `f` não faz nada.
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Assignee);

        app.update(key(KeyCode::Tab)); // Email -> Jira
        assert_eq!(app.focus, Panel::Jira);
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Reporter);
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Both);
        app.update(key(KeyCode::Char('f')));
        assert_eq!(app.jira_filter, JiraFilter::Assignee, "o ciclo volta ao início");
    }
```

- [ ] **Step 7: Rodar e confirmar a falha**

```bash
cargo test --lib app::tests::f_cycles_the_jira_filter_only_when_jira_is_focused
```

Esperado: FAIL — a tecla `f` ainda não está tratada.

- [ ] **Step 8: Tratar `f` e `Enter` no painel de Jira**

Em `src/app.rs`, na função que trata as teclas normais (onde estão os
`KeyCode::Char(' ') if self.focus == Panel::Tasks` etc.), acrescentar:

```rust
            KeyCode::Char('f') if self.focus == Panel::Jira => {
                self.jira_filter = self.jira_filter.next();
                let _ = self.cmd_tx.send(WorkerCmd::FetchJira(self.jira_filter));
            }
```

E fazer o `Enter` decidir por painel — trocar `KeyCode::Enter => self.open_detail(),` por:

```rust
            KeyCode::Enter => match self.focus {
                Panel::Email => self.open_detail(),
                Panel::Jira => self.open_selected_issue(),
                _ => {}
            },
```

Com o método novo, ao lado de `open_detail`:

```rust
    /// Abre no navegador a issue sob o cursor. O erro vai para o painel, como
    /// qualquer outra falha de busca.
    fn open_selected_issue(&mut self) {
        if let Some(item) = self.jira.items.get(self.jira.cursor) {
            if let Err(e) = crate::data::jira::open_url(&item.url) {
                self.jira.error = Some(e);
            }
        }
    }
```

- [ ] **Step 9: Fazer o cursor funcionar no painel de Jira**

O painel hoje usa rolagem livre (`scroll`), não cursor. Na função que move o
cursor por painel (`focused_scroll`) e nas de início/fim (`focused_to_first`,
`focused_to_last`), incluir `Panel::Jira` junto de `Panel::Email` e
`Panel::Tasks`, que já usam cursor. O padrão exato está no código dessas
funções — siga-o em vez de inventar outro.

- [ ] **Step 10: Comando e mensagem do worker**

Em `src/msg.rs`, a variante de resultado do Jira passa a carregar itens:

```rust
    /// Resultado da busca de issues do Jira.
    Jira(Result<Vec<JiraItem>, String>),
```

Em `src/worker.rs`, o comando ganha o filtro:

```rust
    /// Busca as issues do Jira com o modo de filtro dado.
    FetchJira(JiraFilter),
```

E o tratamento chama `data::jira::fetch(filter)` em vez do antigo
`data::jira::fetch()`. Onde o worker faz o refresh periódico de todos os
painéis, passar o filtro atual — o worker não guarda estado, então o `App` envia
`WorkerCmd::FetchJira(self.jira_filter)` no `r` (refresh) e no arranque, como já
faz com os outros painéis.

- [ ] **Step 11: Render do painel**

Em `src/ui.rs`, substituir o corpo de `render_jira`:

```rust
fn render_jira(app: &App, frame: &mut Frame<'_>, area: Rect) {
    let theme = &app.theme;
    let p = &app.jira;
    let title = format!(" JIRA · {} ", app.jira_filter.label());
    let focused = app.focus == Panel::Jira;

    let rows = jira::rows_by_project(&p.items);
    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = rows
        .iter()
        .map(|row| match row {
            jira::JiraRow::Header(h) => Line::from(vec![theme.accent_bold(h.clone())]),
            jira::JiraRow::Issue(i) => {
                let item = &p.items[*i];
                highlight(issue_line(item, theme), theme, selected == Some(*i))
            }
        })
        .collect();

    let inner = panel_inner(frame, theme, area, title, focused);
    if render_empty_state(frame, app, inner, p) {
        return;
    }
    // A rolagem segue o cursor, mas em linhas — o cursor indexa issues.
    let height = inner.height as usize;
    let cursor_row = jira::row_of_item(&rows, p.cursor);
    let off = window(lines.len(), cursor_row, p.offset.get(), height);
    p.offset.set(off);
    render_lines(frame, theme, inner, lines, off);
}

/// Linha de uma issue: chave, status esmaecido e resumo.
fn issue_line(item: &JiraItem, theme: &BubbleTheme) -> Line<'static> {
    Line::from(vec![
        theme.span("  "),
        theme.accent(item.key.clone()),
        theme.muted(format!(" [{}] ", item.status)),
        theme.span(clip(&item.summary, 44)),
    ])
}
```

Se `theme.accent_bold` não existir no `BubbleTheme`, use a combinação que o
painel de PRs já usa para nome de repo (negrito + cor de destaque) em vez de
acrescentar um método novo ao tema.

- [ ] **Step 12: Ajustar o teste de render existente**

O teste `jira_panel_renders_title_and_ansi_colors` em `src/ui.rs` monta
`app.jira.items` com strings ANSI e morre com a mudança de tipo. Substituir por
um que exercite a forma nova, derivando os dados de `/tmp/jira-real.json`:

```rust
    #[test]
    fn jira_panel_renders_filter_label_and_groups_by_project() {
        let mut app = test_app();
        app.jira.items = crate::data::jira::parse_issues(
            r#"[{"key":"ENG-101","summary":"[Painel] - Melhorias","status":"Em andamento",
                 "project":"ENG","url":"u","parent":{"key":"ENG-42","summary":"Eng"}}]"#,
        )
        .unwrap();
        app.jira.loaded = true;
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("JIRA · minhas"), "cabeçalho com o filtro ativo");
        assert!(out.contains("ENG"), "cabeçalho de grupo do projeto");
        assert!(out.contains("ENG-101"), "a chave da issue");
    }
```

- [ ] **Step 13: Suíte inteira e lint**

```bash
cargo test
cargo clippy --all-targets 2>&1 | grep "^warning:" | grep -v "generated" | wc -l
```

Esperado: tudo passando (70 anteriores menos os 3 de `parse_jira` que sumiram,
mais os 6 novos de `jira.rs` e 1 de teclas e 1 de render), e a contagem de
warnings individuais igual a 3.

- [ ] **Step 14: Commit**

```bash
git add src/data/jira.rs src/app.rs src/ui.rs src/msg.rs src/worker.rs
git commit -m "feat(jira): structured issues with filter cycling and browser open"
```

---

### Task 3: Visões por pai e menções

Deliverable: `p` reagrupa as mesmas issues pelo pai, `n` busca e mostra as menções, `Esc` volta.

**Files:**
- Modify: `src/data/jira.rs` (`rows_by_parent`, `JiraView`)
- Modify: `src/app.rs` (visão ativa, teclas `p`/`n`/`Esc`)
- Modify: `src/ui.rs` (cabeçalho de três estados)
- Modify: `src/msg.rs`, `src/worker.rs` (buscar menções)

**Interfaces:**
- Consumes: `JiraItem`, `JiraRow`, `rows_by_project`, `row_of_item`, `fetch` da Task 2.
- Produces:
  ```rust
  pub enum JiraView { Issues, ByParent, Mentions }
  pub fn rows_by_parent(items: &[JiraItem]) -> Vec<JiraRow>
  pub fn fetch_mentions() -> Result<Vec<JiraItem>, String>
  ```
  `App` ganha `jira_view: JiraView` e `jira_mentions: PanelData<JiraItem>`.

- [ ] **Step 1: Teste que falha para o agrupamento por pai**

Em `src/data/jira.rs`, no `mod tests` (a const `REAL` já existe da Task 2):

```rust
    #[test]
    fn groups_by_parent_with_orphans_last() {
        let items = parse_issues(REAL).unwrap();
        let rows = rows_by_parent(&items);
        // O grupo do pai vem primeiro, com chave e resumo no cabeçalho.
        assert!(matches!(&rows[0], JiraRow::Header(h) if h == "ENG-42 Engenharia de Plataforma"));
        assert!(matches!(rows[1], JiraRow::Issue(0)));
        // As sem pai caem num grupo próprio, no fim.
        assert!(matches!(&rows[2], JiraRow::Header(h) if h == "sem pai"));
        assert!(matches!(rows[3], JiraRow::Issue(1)));
        assert!(matches!(rows[4], JiraRow::Issue(2)));
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn by_parent_keeps_every_issue_visible() {
        // Trocar de visão não pode esconder issue nenhuma.
        let items = parse_issues(REAL).unwrap();
        let count = |rows: &[JiraRow]| rows.iter().filter(|r| matches!(r, JiraRow::Issue(_))).count();
        assert_eq!(count(&rows_by_parent(&items)), count(&rows_by_project(&items)));
    }
```

- [ ] **Step 2: Rodar e confirmar a falha**

```bash
cargo test --lib data::jira::tests::groups_by_parent_with_orphans_last
```

Esperado: FAIL — `rows_by_parent` não existe.

- [ ] **Step 3: Implementar `rows_by_parent` e `JiraView`**

Em `src/data/jira.rs`:

```rust
/// Visão ativa do painel de Jira.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JiraView {
    #[default]
    Issues,
    ByParent,
    Mentions,
}

/// Agrupa pelo pai (épico ou iniciativa). As issues sem pai vão para um grupo
/// "sem pai" no fim, para não sumirem da visão.
pub fn rows_by_parent(items: &[JiraItem]) -> Vec<JiraRow> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut orphans: Vec<usize> = Vec::new();

    for (i, item) in items.iter().enumerate() {
        match &item.parent {
            Some(p) => {
                let header = format!("{} {}", p.key, p.summary);
                match groups.iter_mut().find(|(h, _)| *h == header) {
                    Some((_, list)) => list.push(i),
                    None => groups.push((header, vec![i])),
                }
            }
            None => orphans.push(i),
        }
    }
    if !orphans.is_empty() {
        groups.push(("sem pai".to_string(), orphans));
    }

    let mut rows = Vec::new();
    for (header, list) in groups {
        rows.push(JiraRow::Header(header));
        rows.extend(list.into_iter().map(JiraRow::Issue));
    }
    rows
}

/// Roda `jira mentions` e devolve as issues onde fui mencionado.
pub fn fetch_mentions() -> Result<Vec<JiraItem>, String> {
    parse_issues(&run(&["mentions"])?)
}
```

- [ ] **Step 4: Confirmar que passa**

```bash
cargo test --lib data::jira
```

Esperado: os 8 testes de `jira.rs` passando.

- [ ] **Step 5: Teste que falha para as teclas de visão**

No `mod tests` de `src/app.rs`:

```rust
    #[test]
    fn p_and_n_switch_jira_views_and_esc_returns() {
        let mut app = test_app();
        app.update(key(KeyCode::Tab)); // Email -> Jira
        assert_eq!(app.jira_view, JiraView::Issues);

        app.update(key(KeyCode::Char('p')));
        assert_eq!(app.jira_view, JiraView::ByParent);
        app.update(key(KeyCode::Esc));
        assert_eq!(app.jira_view, JiraView::Issues);

        app.update(key(KeyCode::Char('n')));
        assert_eq!(app.jira_view, JiraView::Mentions);
        app.update(key(KeyCode::Esc));
        assert_eq!(app.jira_view, JiraView::Issues, "Esc sempre volta para issues");
    }
```

- [ ] **Step 6: Rodar e confirmar a falha**

```bash
cargo test --lib app::tests::p_and_n_switch_jira_views_and_esc_returns
```

Esperado: FAIL — as teclas não existem.

- [ ] **Step 7: Tratar `p`, `n` e `Esc`**

Em `src/app.rs`, acrescentar `pub jira_view: JiraView,` e
`pub jira_mentions: PanelData<JiraItem>,` ao `struct App` (inicializados com
`JiraView::default()` e `PanelData::default()`), e as teclas:

```rust
            KeyCode::Char('p') if self.focus == Panel::Jira => {
                self.jira_view = JiraView::ByParent;
                self.jira.cursor = 0;
            }
            KeyCode::Char('n') if self.focus == Panel::Jira => {
                self.jira_view = JiraView::Mentions;
                self.jira_mentions.cursor = 0;
                // Menções têm dados próprios; busca na primeira vez e no refresh.
                if !self.jira_mentions.loaded {
                    let _ = self.cmd_tx.send(WorkerCmd::FetchJiraMentions);
                }
            }
            KeyCode::Esc if self.focus == Panel::Jira => {
                self.jira_view = JiraView::Issues;
            }
```

O cursor volta a zero ao trocar de visão porque o conjunto exibido muda de
ordem (por pai) ou de conteúdo (menções) — manter o índice apontaria para outra
issue, o que faria `Enter` abrir a errada.

- [ ] **Step 8: Comando de menções no worker**

Em `src/worker.rs`, acrescentar a variante `FetchJiraMentions` ao `WorkerCmd` e
tratá-la chamando `data::jira::fetch_mentions()`; em `src/msg.rs`, a variante
`JiraMentions(Result<Vec<JiraItem>, String>)`. O `App` aplica o resultado em
`self.jira_mentions` do mesmo jeito que aplica os outros painéis. No refresh
(`r`), re-buscar menções apenas se `jira_mentions.loaded` já for verdadeiro —
não vale pagar a consulta para quem nunca abriu a visão.

- [ ] **Step 9: Render das três visões**

Em `src/ui.rs`, em `render_jira`, escolher a fonte e o agrupamento pela visão, e
montar o cabeçalho com a visão ativa entre colchetes:

```rust
    let (p, rows) = match app.jira_view {
        JiraView::Issues => (&app.jira, jira::rows_by_project(&app.jira.items)),
        JiraView::ByParent => (&app.jira, jira::rows_by_parent(&app.jira.items)),
        JiraView::Mentions => (
            &app.jira_mentions,
            jira::rows_by_project(&app.jira_mentions.items),
        ),
    };
    let title = format!(
        " JIRA · {} · {} ",
        app.jira_filter.label(),
        match app.jira_view {
            JiraView::Issues => "[issues] por-pai menções",
            JiraView::ByParent => "issues [por-pai] menções",
            JiraView::Mentions => "issues por-pai [menções]",
        }
    );
```

O resto da função (linhas, `window`, `render_lines`) continua igual, operando
sobre `p` e `rows`.

- [ ] **Step 10: Teste de render das visões**

```rust
    #[test]
    fn jira_header_marks_the_active_view() {
        let mut app = test_app();
        app.jira.loaded = true;
        app.jira_view = crate::data::jira::JiraView::ByParent;
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("[por-pai]"), "a visão ativa aparece entre colchetes");
    }
```

- [ ] **Step 11: Suíte e lint**

```bash
cargo test
cargo clippy --all-targets 2>&1 | grep "^warning:" | grep -v "generated" | wc -l
```

Esperado: tudo passando; warnings = 3.

- [ ] **Step 12: Smoke test ao vivo das menções**

```bash
uv run --script scripts/jira mentions | python -c "import json,sys;print(len(json.load(sys.stdin)),'menções')"
```

Esperado: um número (4 em 2026-08-03). Confirma que a visão tem dado real por trás.

- [ ] **Step 13: Commit**

```bash
git add src/data/jira.rs src/app.rs src/ui.rs src/msg.rs src/worker.rs
git commit -m "feat(jira): add by-parent and mentions views"
```

---

### Task 4: Remover o `jirapending` e atualizar docs e setup

Deliverable: nenhuma referência ao `jirapending` fora do histórico; setup e README ensinam o helper novo.

**Files:**
- Delete: `scripts/jirapending`, `scripts/jirapending.ps1`, `scripts/jirapending.cmd`
- Modify: `scripts/install.sh`, `scripts/setup-auth.sh`, `README.md`
- Modify: `src/data/mod.rs` (comentário de `helper_command` e de `force_utf8_stdout`)

**Interfaces:**
- Consumes: o helper `jira` da Task 1.
- Produces: nada consumido por outra task.

- [ ] **Step 1: Apagar os três arquivos**

```bash
git rm scripts/jirapending scripts/jirapending.ps1 scripts/jirapending.cmd
```

- [ ] **Step 2: Instalar o helper novo no `install.sh`**

Na função `install_helpers`, trocar as duas linhas do `jirapending` por uma do
`jira`, mantendo o `mstodo` como está:

```bash
install_helpers() {
  step "Instalando helpers (jira, mstodo)"
  mkdir -p "$BIN_DIR"
  install -m 0755 "$SCRIPT_DIR/jira"   "$BIN_DIR/jira"
  install -m 0755 "$SCRIPT_DIR/mstodo" "$BIN_DIR/mstodo"
  info "copiados para $BIN_DIR"
}
```

Atualizar também o comentário do cabeçalho do arquivo, que lista os helpers copiados.

- [ ] **Step 3: `setup-auth.sh`**

Trocar `jirapending` por `jira` na lista de CLIs do preflight (`doctor`), e o
probe do painel:

```bash
  probe "jira (jira)" "jira issues" \
    "defina JIRA_EMAIL / JIRA_CLOUD / JIRA_TOKEN"
```

- [ ] **Step 4: Comentários em `src/data/mod.rs`**

`helper_command` e `force_utf8_stdout` citam `jirapending` e `mstodo`. O helper
novo é Python, então os dois comentários passam a citar `jira` e `mstodo` —
e o de `helper_command` deixa de precisar explicar a porta PowerShell, porque não
há mais nenhuma.

- [ ] **Step 5: README**

Substituir as menções ao `jirapending` (tabela de helpers, lista de features,
seção de setup e a tabela de troubleshooting) pelas do `jira`, documentando:
os três subcomandos, as teclas novas do painel (`Enter`, `f`, `p`, `n`, `Esc`),
a precedência do `JIRA_JQL` sobre o `--filter`, e que o modo relator filtra por
`In Progress` — com o motivo em uma linha, porque um leitor vai estranhar a
assimetria entre os modos.

Acrescentar na nota 🪟 do Windows o `jira` junto do `mstodo` no comando de
instalação em `%USERPROFILE%\.local\bin`.

- [ ] **Step 6: Gate final**

```bash
grep -rn "jirapending" --exclude-dir=target --exclude-dir=.git . | grep -v "docs/superpowers/"
bash -n scripts/install.sh && bash -n scripts/setup-auth.sh && echo "sintaxe ok"
cargo test
```

Esperado: o `grep` sem saída (as menções em `docs/superpowers/` são registro
histórico e ficam), `sintaxe ok`, e a suíte passando.

- [ ] **Step 7: Instalar o helper no PATH e verificar o painel de ponta a ponta**

No Windows, o daily-tui invoca `jira` pelo nome via `cmd /C`, então o helper
precisa estar em `%USERPROFILE%\.local\bin` — o `install.sh` só cobre Linux:

```bash
cp scripts/jira scripts/jira.cmd "$USERPROFILE/.local/bin/"
cmd //C "where jira"
```

Esperado: os dois caminhos. Sem isso o painel mostra `jira falhou` mesmo com o
código correto — foi exatamente o que aconteceu na migração do `mstodo`.

- [ ] **Step 8: Commit**

```bash
git add -u && git add README.md
git commit -m "chore(jira): drop jirapending and document the new helper"
```

---

## Ordem e dependências

1. **Task 1** (helper) — independente; precisa das três variáveis `JIRA_*` no ambiente.
2. **Task 2** (painel estruturado) — depende da Task 1, inclusive da saída real capturada no Step 6 dela.
3. **Task 3** (visões) — depende da Task 2.
4. **Task 4** (remoção e docs) — por último, porque o gate é o grep vazio.

---

### Task 5: Atalhos no footer e papel na visão "ambas"

Acrescentada em 2026-08-04, a pedido do João. Duas coisas que só aparecem com o
painel em uso: as teclas novas não são descobríveis, e no filtro `ambas` não há
como saber por que uma issue está ali.

**Files:**
- Modify: `scripts/jira` (campos `assignee`/`reporter`, `accountId`, campo `role`)
- Modify: `src/data/jira.rs` (`JiraRole`, campo em `JiraItem`)
- Modify: `src/ui.rs` (marcador na linha da issue; footer por painel)

**Interfaces:**
- Consumes: o helper e o painel das Tasks 1-3.
- Produces: `role` no contrato JSON (`"assignee" | "reporter" | "both"`); `JiraItem.role: JiraRole`; footer sensível ao painel focado.

- [ ] **Step 1: O helper passa a dizer o papel**

`FIELDS` ganha os dois campos e o `to_item` compara com o `accountId`:

```python
FIELDS = ["summary", "status", "project", "parent", "assignee", "reporter"]
```

```python
@cache
def account_id():
    """accountId do dono do token; usado para o papel e para as menções."""
    me = api("GET", "/rest/api/3/myself").json().get("accountId")
    if not me:
        die("Jira não devolveu o accountId em /myself")
    return me


def role_of(fields, me):
    """Por que a issue entrou no resultado: sou responsável, relator, ou os dois."""
    is_assignee = ((fields.get("assignee") or {}).get("accountId")) == me
    is_reporter = ((fields.get("reporter") or {}).get("accountId")) == me
    if is_assignee and is_reporter:
        return "both"
    return "reporter" if is_reporter else "assignee"
```

`to_item` recebe o `me` e acrescenta `"role": role_of(fields, me)`. O `emit`
resolve o `accountId` uma vez e repassa. O default é `assignee` quando nenhum dos
dois casa — acontece com `JIRA_JQL` customizada, e assumir "atuando" é menos
enganoso que inventar um quarto estado.

- [ ] **Step 2: Verificar contra o Jira real**

```bash
uv run --script scripts/jira issues --filter both | python -c "
import json,sys,collections
d=json.load(sys.stdin)
print(collections.Counter(i['role'] for i in d))
assert all(i['role'] in ('assignee','reporter','both') for i in d)
"
```

Esperado: um `Counter` com os três papéis somando o total, e ao menos um `both`
(medido em 2026-08-04: 7 assignee + 7 reporter com 1 sobreposição = 13).

- [ ] **Step 3: Teste que falha no lado Rust**

Em `src/data/jira.rs`, no `mod tests`:

```rust
    #[test]
    fn parses_the_role_of_each_issue() {
        let raw = r#"[{"key":"ENG-1","summary":"s","status":"Em andamento","project":"ENG",
                       "url":"https://example.atlassian.net/browse/ENG-1","parent":null,"role":"both"},
                      {"key":"OPS-2","summary":"s","status":"Backlog","project":"OPS",
                       "url":"https://example.atlassian.net/browse/OPS-2","parent":null,"role":"reporter"}]"#;
        let items = parse_issues(raw).unwrap();
        assert_eq!(items[0].role, JiraRole::Both);
        assert_eq!(items[1].role, JiraRole::Reporter);
        assert_eq!(items[0].role.marker(), "[AR]");
        assert_eq!(items[1].role.marker(), "[R]");
    }

    #[test]
    fn missing_role_defaults_to_assignee() {
        let items = parse_issues(r#"[{"key":"A-1","summary":"s","status":"","project":"A","url":"u","parent":null}]"#).unwrap();
        assert_eq!(items[0].role, JiraRole::Assignee);
        assert_eq!(items[0].role.marker(), "[A]");
    }
```

- [ ] **Step 4: Rodar e confirmar a falha**

```bash
cargo test --lib data::jira::tests::parses_the_role_of_each_issue
```

Esperado: FAIL — `JiraRole` não existe.

- [ ] **Step 5: Implementar `JiraRole`**

```rust
/// Por que a issue está no resultado. Só é exibido no filtro `ambas`, onde a
/// pergunta "sou responsável ou só relator disso?" tem resposta ambígua.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum JiraRole {
    #[default]
    Assignee,
    Reporter,
    Both,
}

impl JiraRole {
    /// Marcador curto, no idioma dos `[W]`/`[P]` do e-mail e da agenda.
    pub const fn marker(self) -> &'static str {
        match self {
            JiraRole::Assignee => "[A]",
            JiraRole::Reporter => "[R]",
            JiraRole::Both => "[AR]",
        }
    }
}
```

Em `JiraItem`, `#[serde(default)] pub role: JiraRole,`.

- [ ] **Step 6: Mostrar o marcador só no filtro `ambas`**

`issue_line` recebe se deve mostrar o papel. No `render_jira`, o marcador entra
quando `app.jira_filter == JiraFilter::Both` **e** a visão não é `Mentions` — em
menções o papel não explica nada, porque a issue está ali por citação.
O marcador vai depois do status, esmaecido, antes do resumo, e o `clip` do resumo
encurta em 5 para o marcador não empurrar texto fora do painel.

- [ ] **Step 7: Footer com as teclas do painel focado**

O footer hoje é uma linha fixa. Passa a acrescentar as teclas do painel em foco,
mantendo as globais:

```rust
/// Teclas específicas do painel em foco, para as ações serem descobríveis.
fn panel_hints(focus: Panel) -> &'static str {
    match focus {
        Panel::Jira => "enter abre · f filtro · p por-pai · n menções · esc volta",
        Panel::Tasks => "espaço alterna · a nova · e edita · d apaga",
        _ => "",
    }
}
```

Compor com o texto global existente, separando por `·`, e omitir quando vazio.
Não inventar teclas: só as que existem hoje no `handle_panel_key`.

- [ ] **Step 8: Teste do footer**

```rust
    #[test]
    fn footer_shows_the_keys_of_the_focused_panel() {
        let mut app = test_app();
        app.update(key(KeyCode::Tab)); // Email -> Jira
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("f filtro"), "o footer anuncia as teclas do Jira");
        assert!(out.contains("n menções"));
    }
```

- [ ] **Step 9: Suíte, lint e README**

```bash
cargo test
cargo clippy --all-targets 2>&1 | grep -c "^warning: [a-z]"
```

Esperado: tudo passando; warnings = 3. No README, documentar o `role` no contrato
JSON e os três marcadores.

- [ ] **Step 10: Commit e reinstalar o helper**

```bash
git add scripts/jira src/data/jira.rs src/ui.rs README.md
git commit -m "feat(jira): show role markers in the both filter and panel key hints"
cp scripts/jira scripts/jira.cmd "$USERPROFILE/.local/bin/"
```
