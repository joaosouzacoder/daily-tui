# Microsoft To Do Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Trocar o backend do painel de tarefas do daily-tui do Google Tasks para o Microsoft To Do (conta pessoal), sem mudar nada do comportamento visível na TUI.

**Architecture:** Um helper CLI externo novo (`scripts/mstodo`, Python via uv) fala com a Microsoft Graph API e imprime o **mesmo contrato JSON** que o `gtasks` imprimia; `src/data/tasks.rs` só troca o nome do programa que invoca. O `gtasks` é removido do repo, dos scripts de setup e da documentação.

**Tech Stack:** Rust (ratatui, serde_json), Python 3.10+ via `uv` (PEP 723 inline script), `msal` (device code + refresh), `requests`, Microsoft Graph v1.0.

**Spec:** `docs/superpowers/specs/2026-07-31-microsoft-todo-design.md`

## Global Constraints

- Contrato JSON inalterado: `[{"id","title","completed","due","notes"}]` — é o que permite manter `parse_tasks` e seus testes.
- Graph base `https://graph.microsoft.com/v1.0`; authority `https://login.microsoftonline.com/consumers`; escopo `Tasks.ReadWrite`.
- Ordenação: pendentes primeiro, depois vencimento crescente com "sem data" no fim, depois `createdDateTime` crescente.
- `list` devolve **também as concluídas** (sem `$filter`), como hoje.
- Todo erro é **uma linha no stderr**, sem traceback. Mensagens exatas: `sem credenciais — rode: mstodo auth`, `defina DAILY_TUI_TODO_CLIENT_ID`, `lista '<nome>' não encontrada`, `Graph <status>: <mensagem>`.
- Caminhos com `Path.home()` nos dois sistemas: token em `<home>/.local/share/daily-tui/mstodo-personal.json` (`chmod 0600` no Unix), sidecar da lista em `mstodo-list.json` na mesma pasta.
- Variáveis: `DAILY_TUI_TODO_CLIENT_ID` (obrigatória), `DAILY_TUI_TODO_LIST` (opcional, nome da lista; vazio = `wellknownListName: defaultList`), `MSTODO_TOKEN` (opcional, caminho do cache).
- Comentários e mensagens do código em português, como o resto do repo.
- Ao final: `cargo test` e `cargo clippy --all-targets` sem regressão (a base tem 3 warnings pré-existentes em `app.rs:115`, `app.rs:119` e `email.rs:90` — não corrigir aqui).

## Pré-requisito manual — RESOLVIDO de outra forma (2026-08-03)

O plano pedia um app registration próprio no portal Entra. O login no portal
falhou com `AADSTS500121` (MFA de tenant recusando a conta pessoal), então o
client em uso é o first-party público da Microsoft
`14d82eec-204b-4c2f-b7e8-296a70dab67e` ("Microsoft Graph Command Line Tools"),
autorizado por device code. Ver a seção "Autenticação" do spec para o trade-off
e para o roteiro do portal como plano B.

Consequência para a execução: `mstodo auth` **já foi rodado com sucesso** e o
cache existe em `<home>/.local/share/daily-tui/mstodo-personal.json`. Toda
invocação precisa exportar aquele client id — o cache de token está atrelado a
ele.

Consequência para os artefatos já entregues: `daily-tui.env.example`,
`daily-tui.config.example.ps1` e o roteiro impresso por `setup_mstodo` no
`setup-auth.sh` ainda mandam criar um app registration. A **Task 6** alinha
esses três com a realidade.

## File Structure

| Arquivo | Responsabilidade |
| --- | --- |
| `scripts/mstodo` (criar) | Toda a conversa com o Graph: auth, leitura, escritas, resolução da lista. Único lugar que conhece a Graph API. |
| `scripts/mstodo.cmd` (criar) | Shim Windows: `uv run --script` no `mstodo`. |
| `src/data/tasks.rs` (modificar) | Continua dono do `TaskItem` e do parse; só muda o programa invocado e as mensagens. |
| `src/data/mod.rs` (modificar) | Comentários de `helper_command` e `force_utf8_stdout` citam `gtasks`. |
| `src/worker.rs`, `src/msg.rs` (modificar) | Comentários que dizem "Google Tasks". |
| `scripts/daily-tui.env.example` (modificar) | Variáveis novas (Linux). |
| `scripts/daily-tui.config.example.ps1`, `scripts/daily-tui-launch.ps1` (modificar) | Variáveis novas (Windows). |
| `scripts/install.sh` (modificar) | Instala `mstodo` no lugar do `gtasks`. |
| `scripts/setup-auth.sh` (modificar) | Alvo `google` fica só com o gcalcli; alvo novo para o `mstodo`; preflight e probes. |
| `scripts/google-auth.ps1` (modificar) | Sai o trecho de tarefas; entra o nome novo do client secret. |
| `README.md` (modificar) | Feature list, tabela de helpers, setup, troubleshooting. |
| `scripts/gtasks`, `scripts/gtasks.cmd` (apagar) | — |

**Acoplamento que o spec não previu:** `setup-auth.sh:144` e `google-auth.ps1:27` usam o arquivo `~/.config/daily-tui/gtasks-client-secret.json` como **client OAuth do gcalcli** (a agenda), não só do gtasks. Ele não pode ser apagado junto. A Task 3 renomeia para `google-client-secret.json` mantendo fallback para o nome antigo, para não quebrar a instalação existente do João.

---

### Task 1: Helper `mstodo`

Deliverable independente: `mstodo list` devolve o JSON das tarefas reais da conta, e as cinco escritas têm efeito verificável no `list`.

**Files:**
- Create: `scripts/mstodo`
- Create: `scripts/mstodo.cmd`

**Interfaces:**
- Consumes: nada (primeira task).
- Produces: a CLI `mstodo` com os subcomandos `auth`, `list`, `add "<título>"`, `complete <id>`, `reopen <id>`, `edit <id> "<título>"`, `delete <id>`. `list` escreve no stdout um array JSON de objetos `{id: str, title: str, completed: bool, due: str, notes: str}`; as escritas não escrevem nada em caso de sucesso. Qualquer falha: uma linha no stderr e exit 1.

- [ ] **Step 1: Criar `scripts/mstodo`**

```python
#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "msal",
#   "requests",
# ]
# ///
"""mstodo — CLI mínima para o Microsoft To Do (conta pessoal), usada pelo daily-tui.

Subcomandos:
  mstodo auth                  # consent único (device code), salva o token
  mstodo list                  # JSON das tarefas da lista configurada
  mstodo add "<título>"
  mstodo complete <id>
  mstodo reopen <id>
  mstodo edit <id> "<título>"
  mstodo delete <id>

Configuração (veja scripts/daily-tui.env.example):
  DAILY_TUI_TODO_CLIENT_ID — Application (client) ID do app registration (obrigatório)
  DAILY_TUI_TODO_LIST      — nome da lista; vazio = lista padrão do To Do
  MSTODO_TOKEN             — sobrescreve o caminho do cache de token

Todo erro sai como UMA linha no stderr: o painel do daily-tui mostra só o
resumo (ver `stderr_summary` em src/data/mod.rs), então traceback não ajuda.
"""
import json
import os
import sys
from functools import cache
from pathlib import Path

import msal
import requests

GRAPH = "https://graph.microsoft.com/v1.0"
AUTHORITY = "https://login.microsoftonline.com/consumers"
SCOPES = ["Tasks.ReadWrite"]
TIMEOUT = 30

TOKEN = Path(os.environ.get(
    "MSTODO_TOKEN",
    str(Path.home() / ".local/share/daily-tui/mstodo-personal.json")))
# Sidecar com o id da lista resolvido — não vai dentro do cache do MSAL, que
# tem formato próprio.
LIST_CACHE = TOKEN.with_name("mstodo-list.json")


def die(msg):
    """Sai com código 1 e a mensagem no stderr."""
    sys.exit(str(msg))


def client_id():
    cid = os.environ.get("DAILY_TUI_TODO_CLIENT_ID", "").strip()
    if not cid:
        die("defina DAILY_TUI_TODO_CLIENT_ID")
    return cid


def build_app():
    """Devolve (app MSAL, cache) com o cache lido do disco, se existir."""
    token_cache = msal.SerializableTokenCache()
    if TOKEN.exists():
        token_cache.deserialize(TOKEN.read_text(encoding="utf-8"))
    app = msal.PublicClientApplication(
        client_id(), authority=AUTHORITY, token_cache=token_cache)
    return app, token_cache


def save_cache(token_cache):
    if not token_cache.has_state_changed:
        return
    TOKEN.parent.mkdir(parents=True, exist_ok=True)
    TOKEN.write_text(token_cache.serialize(), encoding="utf-8")
    if os.name != "nt":
        TOKEN.chmod(0o600)


@cache
def bearer():
    """Access token válido, renovado pelo refresh token quando preciso."""
    app, token_cache = build_app()
    accounts = app.get_accounts()
    result = app.acquire_token_silent(SCOPES, account=accounts[0]) if accounts else None
    save_cache(token_cache)
    if not result or "access_token" not in result:
        die("sem credenciais — rode: mstodo auth")
    return result["access_token"]


def do_auth():
    app, token_cache = build_app()
    flow = app.initiate_device_flow(scopes=SCOPES)
    if "user_code" not in flow:
        die(f"device flow falhou: {flow.get('error_description') or flow}")
    print(flow["message"], flush=True)
    result = app.acquire_token_by_device_flow(flow)
    save_cache(token_cache)
    if "access_token" not in result:
        die(f"autorização falhou: {result.get('error_description') or result}")
    print(f"ok — token salvo em {TOKEN}")


def error_message(resp):
    try:
        return resp.json()["error"]["message"]
    except (ValueError, KeyError, TypeError):
        return resp.text[:200].replace("\n", " ")


def request(method, url, **kw):
    resp = requests.request(
        method, url,
        headers={"Authorization": f"Bearer {bearer()}"}, timeout=TIMEOUT, **kw)
    if not resp.ok:
        die(f"Graph {resp.status_code}: {error_message(resp)}")
    return resp


@cache
def list_id():
    """Id da lista configurada, com cache em disco para poupar uma chamada."""
    want = os.environ.get("DAILY_TUI_TODO_LIST", "").strip()
    if LIST_CACHE.exists():
        try:
            cached = json.loads(LIST_CACHE.read_text(encoding="utf-8"))
            if cached.get("name", "") == want and cached.get("id"):
                return cached["id"]
        except (OSError, ValueError):
            pass

    lists = request("GET", f"{GRAPH}/me/todo/lists").json().get("value", [])
    if want:
        found = next((l for l in lists
                      if (l.get("displayName") or "").casefold() == want.casefold()), None)
        if not found:
            die(f"lista '{want}' não encontrada")
    else:
        found = next((l for l in lists
                      if l.get("wellknownListName") == "defaultList"), None)
        if not found:
            die("lista padrão do To Do não encontrada")

    LIST_CACHE.parent.mkdir(parents=True, exist_ok=True)
    LIST_CACHE.write_text(
        json.dumps({"name": want, "id": found["id"]}), encoding="utf-8")
    return found["id"]


def in_list(method, suffix="", **kw):
    """Chama o Graph dentro da lista configurada.

    Um 404 na primeira tentativa pode ser id de lista velho (lista apagada ou
    recriada), então invalida o cache e tenta de novo. Se o 404 for do id da
    tarefa, a segunda tentativa falha com a mensagem do Graph.
    """
    for attempt in (1, 2):
        url = f"{GRAPH}/me/todo/lists/{list_id()}/tasks{suffix}"
        resp = requests.request(
            method, url,
            headers={"Authorization": f"Bearer {bearer()}"}, timeout=TIMEOUT, **kw)
        if resp.status_code == 404 and attempt == 1:
            LIST_CACHE.unlink(missing_ok=True)
            list_id.cache_clear()
            continue
        if not resp.ok:
            die(f"Graph {resp.status_code}: {error_message(resp)}")
        return resp
    return None  # inalcançável: a segunda volta retorna ou chama die()


def to_item(task):
    """Converte um todoTask do Graph no contrato que o daily-tui espera."""
    due = ((task.get("dueDateTime") or {}).get("dateTime") or "")[:10]
    return {
        "id": task["id"],
        "title": (task.get("title") or "").strip(),
        "completed": task.get("status") == "completed",
        "due": due,
        "notes": ((task.get("body") or {}).get("content") or "").strip(),
    }


def do_list():
    raw = []
    resp = in_list("GET", "?$top=100")
    while True:
        page = resp.json()
        raw.extend(page.get("value", []))
        nxt = page.get("@odata.nextLink")
        if not nxt:
            break
        resp = request("GET", nxt)

    created = {t["id"]: t.get("createdDateTime") or "" for t in raw}
    items = [to_item(t) for t in raw]
    # Pendentes primeiro; depois vencimento (sem data no fim); depois criação.
    items.sort(key=lambda i: (i["completed"], i["due"] == "", i["due"], created[i["id"]]))
    print(json.dumps(items, ensure_ascii=False))


def do_add(title):
    in_list("POST", "", json={"title": title})


def do_complete(tid):
    in_list("PATCH", f"/{tid}", json={"status": "completed"})


def do_reopen(tid):
    in_list("PATCH", f"/{tid}", json={"status": "notStarted"})


def do_edit(tid, title):
    in_list("PATCH", f"/{tid}", json={"title": title})


def do_delete(tid):
    in_list("DELETE", f"/{tid}")


def main():
    args = sys.argv[1:]
    if not args:
        die("uso: mstodo {auth|list|add|complete|reopen|edit|delete}")
    cmd, rest = args[0], args[1:]
    if cmd == "auth":
        do_auth()
    elif cmd == "list":
        do_list()
    elif cmd == "add":
        do_add(rest[0])
    elif cmd == "complete":
        do_complete(rest[0])
    elif cmd == "reopen":
        do_reopen(rest[0])
    elif cmd == "edit":
        do_edit(rest[0], rest[1])
    elif cmd == "delete":
        do_delete(rest[0])
    else:
        die(f"comando desconhecido: {cmd}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Criar `scripts/mstodo.cmd`**

```bat
@echo off
rem Shim Windows: o daily-tui chama `mstodo`; roda o script Python via uv (PEP 723).
uv run --script "%~dp0mstodo" %*
```

- [ ] **Step 3: Verificar que sem client id o erro é a linha certa**

```bash
DAILY_TUI_TODO_CLIENT_ID= uv run --script scripts/mstodo list; echo "exit=$?"
```

Esperado: stderr com exatamente `defina DAILY_TUI_TODO_CLIENT_ID` e `exit=1`.

- [ ] **Step 4: Autorizar (device code)**

```bash
export DAILY_TUI_TODO_CLIENT_ID="<client id do app registration>"
uv run --script scripts/mstodo auth
```

Esperado: imprime a instrução com o código; após digitar em `https://www.microsoft.com/link` (a URL que o MSAL devolve), termina com `ok — token salvo em ...`. No Windows: `scripts\mstodo.cmd auth`.

- [ ] **Step 5: Smoke test dos sete subcomandos**

Rodar na ordem, conferindo o efeito de cada um na saída do `list`:

```bash
uv run --script scripts/mstodo list                      # baseline: array JSON
uv run --script scripts/mstodo add "teste daily-tui ção"  # acento de propósito
uv run --script scripts/mstodo list                      # a nova tarefa aparece, completed=false
ID=$(uv run --script scripts/mstodo list | python -c "import json,sys;print([t['id'] for t in json.load(sys.stdin) if t['title'].startswith('teste daily-tui')][0])")
uv run --script scripts/mstodo complete "$ID"
uv run --script scripts/mstodo list                      # completed=true e no fim da lista
uv run --script scripts/mstodo reopen "$ID"
uv run --script scripts/mstodo edit "$ID" "teste editado"
uv run --script scripts/mstodo list                      # title novo, completed=false
uv run --script scripts/mstodo delete "$ID"
uv run --script scripts/mstodo list                      # sumiu
```

Esperado em todas as leituras: JSON válido, acentos corretos (`ção`, não `ção` nem `Ã§`), e as chaves exatamente `id`, `title`, `completed`, `due`, `notes`.

- [ ] **Step 6: Guardar a saída real para a Task 2**

```bash
uv run --script scripts/mstodo list > /tmp/mstodo-real.json
```

Esse arquivo é a fonte do caso de teste da Task 2 — o teste tem de sair de saída real, não de JSON escrito à mão.

- [ ] **Step 7: Commit**

```bash
git add scripts/mstodo scripts/mstodo.cmd
git commit -m "feat(tasks): add mstodo helper for Microsoft To Do"
```

---

### Task 2: `tasks.rs` passa a chamar o `mstodo`

**Files:**
- Modify: `src/data/tasks.rs` (linhas 1-9, 22-24, 27, 57-70)
- Modify: `src/data/mod.rs:15` e `mod.rs:32` (comentários)
- Modify: `src/worker.rs:22`, `src/worker.rs:75`, `src/msg.rs:22` (comentários)

**Interfaces:**
- Consumes: a CLI `mstodo` da Task 1 e o arquivo `/tmp/mstodo-real.json`.
- Produces: `tasks::fetch/add/complete/reopen/edit/delete` inalterados em assinatura; `TaskItem` inalterado.

**Nota sobre TDD:** esta task não tem fase vermelha, e isso é consequência do
design, não descuido: o contrato JSON é idêntico, então `parse_tasks` já passa
com a saída do `mstodo`. O teste novo existe para **travar** esse contrato com
dados reais; o gate de verdade é `cargo test` mais o painel rodando.

- [ ] **Step 1: Adicionar o teste de contrato com a saída real**

Copiar de `/tmp/mstodo-real.json` um item real (id longo do Graph, `due` e
`notes` vazios) para dentro de `src/data/tasks.rs`, no `mod tests`:

```rust
    // Item real de `mstodo list` (Microsoft Graph): id longo, due/notes vazios.
    #[test]
    fn parses_real_mstodo_output() {
        let raw = r#"[{"id":"AAMkADAwATM3ZmYAZS0zNGZmLTM0ZmYALTAwAi0wMAoARgAAA=","title":"Comprar café","completed":false,"due":"","notes":""}]"#;
        let tasks = parse_tasks(raw).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Comprar café");
        assert!(!tasks[0].completed);
        assert_eq!(tasks[0].due, "");
        assert_eq!(tasks[0].notes, "");
    }
```

Substituir o `id` e o `title` pelos valores que saíram de verdade no arquivo.

- [ ] **Step 2: Rodar o teste**

```bash
cargo test parses_real_mstodo_output
```

Esperado: PASS (o contrato não mudou — ver a nota acima).

- [ ] **Step 3: Trocar o programa e as mensagens em `tasks.rs`**

Cabeçalho do módulo (linhas 1-5):

```rust
//! Tarefas do Microsoft To Do (conta pessoal) via a CLI `mstodo`.
//!
//! Leitura: `mstodo list` devolve JSON; escrita: `add`/`complete`/`reopen`/
//! `edit`/`delete`. O painel é interativo, então diferente de PRs/Jira aqui os
//! itens são estruturados (precisamos do `id` para agir na tarefa selecionada).
```

Doc do tipo (linha 9): `/// Uma tarefa do Microsoft To Do.`
Doc do parse (linha 22): `/// Parseia o JSON do `mstodo list` numa lista de tarefas.`
Mensagem do parse (linha 24): `format!("JSON inválido do mstodo: {e}")`
Doc do fetch (linha 27): `/// Roda `mstodo list` e devolve as tarefas.`

E o corpo do `run` (linhas 57-70):

```rust
/// Roda `mstodo <args...>` e devolve o stdout (ou um erro com o stderr).
fn run(args: &[&str]) -> Result<String, String> {
    let mut cmd = super::helper_command("mstodo");
    // O `mstodo` serializa com `ensure_ascii=False`, então títulos acentuados
    // dependem da codificação do stdout (veja `force_utf8_stdout`).
    super::force_utf8_stdout(&mut cmd);
    let output = cmd
        .args(args)
        .output()
        .map_err(|e| format!("falha ao executar mstodo: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("mstodo falhou: {}", super::stderr_summary(&err)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
```

- [ ] **Step 4: Atualizar os comentários que citam Google Tasks / gtasks**

- `src/data/mod.rs:15`: trocar ``No Windows, `jirapending` e `gtasks` são shims `.cmd``` por ``No Windows, `jirapending` e `mstodo` são shims `.cmd``.
- `src/data/mod.rs:32`: trocar ``(`gcalcli`, `gtasks`)`` por ``(`gcalcli`, `mstodo`)``.
- `src/worker.rs:22` e `src/worker.rs:75`: trocar "Google Tasks" por "Microsoft To Do".
- `src/msg.rs:22`: trocar "lista do gtasks" por "lista do mstodo".

- [ ] **Step 5: Rodar a suíte inteira**

```bash
cargo test
cargo clippy --all-targets 2>&1 | grep -c "^warning: "
```

Esperado: todos os testes passando (67 + 1 novo = 68) e a contagem de warnings igual à da base (os 3 pré-existentes).

- [ ] **Step 6: Commit**

```bash
git add src/data/tasks.rs src/data/mod.rs src/worker.rs src/msg.rs
git commit -m "feat(tasks): point tasks panel at mstodo instead of gtasks"
```

---

### Task 3: Setup e configuração no Linux

**Files:**
- Modify: `scripts/daily-tui.env.example:25-30`
- Modify: `scripts/install.sh:136-140`
- Modify: `scripts/setup-auth.sh` (linhas 10, 60, 78-79, 132, 139, 144, 167-173, 179)

**Interfaces:**
- Consumes: a CLI `mstodo` da Task 1.
- Produces: `setup-auth.sh mstodo` como alvo novo; `install.sh` instalando `mstodo` em `~/.local/bin`.

- [ ] **Step 1: Trocar a seção do `.env.example`**

Substituir as linhas 25-30 por:

```bash
# --- Microsoft To Do (mstodo) ---------------------------------------------
# App registration no portal Entra: "Personal Microsoft accounts only",
# "Allow public client flows: Yes", permissão delegada Tasks.ReadWrite.
export DAILY_TUI_TODO_CLIENT_ID="00000000-0000-0000-0000-000000000000"
# Nome da lista do To Do; vazio ou ausente = lista padrão ("Tarefas").
# export DAILY_TUI_TODO_LIST="Trabalho"
# Cache de token (default: ~/.local/share/daily-tui/mstodo-personal.json)
# export MSTODO_TOKEN="$HOME/.local/share/daily-tui/mstodo-personal.json"
```

- [ ] **Step 2: Trocar o helper instalado no `install.sh`**

```bash
install_helpers() {
  step "Instalando helpers (jirapending, mstodo)"
  mkdir -p "$BIN_DIR"
  install -m 0755 "$SCRIPT_DIR/jirapending" "$BIN_DIR/jirapending"
  install -m 0755 "$SCRIPT_DIR/mstodo"      "$BIN_DIR/mstodo"
  info "copiados para $BIN_DIR"
}
```

Também atualizar o comentário do cabeçalho (`install.sh:10`): `#   6. copia os helpers jirapending e mstodo para ~/.local/bin.`

- [ ] **Step 3: Renomear o client secret compartilhado no `setup-auth.sh`**

O JSON hoje chamado `gtasks-client-secret.json` é o client OAuth **do gcalcli**
também, então ele fica — com nome honesto e fallback para o antigo. Na
`setup_google()`, trocar o bloco das linhas 143-150 por:

```bash
  # Client OAuth compartilhado da agenda (gcalcli). O nome antigo
  # (gtasks-client-secret.json) é aceito para não quebrar instalações existentes.
  local secret_dir="$HOME/.config/daily-tui"
  local default_secret="$secret_dir/google-client-secret.json"
  [[ -f "$default_secret" ]] || [[ ! -f "$secret_dir/gtasks-client-secret.json" ]] \
    || default_secret="$secret_dir/gtasks-client-secret.json"
  local secret
  if [[ -f "$default_secret" ]]; then
    info "Já existe um client salvo. Enter aceita; ou informe outro caminho."
    secret="$(ask 'Caminho do JSON do OAuth client (Desktop app)' "$default_secret")"
  else
    secret="$(ask 'Caminho do JSON do OAuth client (Desktop app)')"
  fi
```

E substituir o bloco do `gtasks` (linhas 167-173) por uma cópia do client para
o nome novo, sem auth de tarefas:

```bash
  # Guarda o client no caminho padrão, para o google-auth.ps1 e re-runs.
  mkdir -p "$secret_dir"
  cp "$secret" "$default_secret"
  info "client OAuth salvo em $default_secret"
```

- [ ] **Step 4: Ajustar título, roteiro e probes do `setup-auth.sh`**

- linha 10: `#   google         — configura gcalcli (agenda) via OAuth do GCP`
- linha 132: `step "Agenda (gcalcli) via OAuth do Google Cloud"`
- linha 139: `      2. Habilite a API: "Google Calendar API"` (sai a Tasks API)
- linha 60: no preflight, trocar `gtasks` por `mstodo` na lista de CLIs
- linhas 78-79: trocar o probe por:

```bash
  probe "tasks (mstodo)" "mstodo list" \
    "rode: scripts/setup-auth.sh mstodo"
```

- linha 179: remover o `probe "tasks (gtasks)" ...` de dentro da `setup_google()`.

- [ ] **Step 5: Adicionar o alvo `mstodo`**

Nova função, no estilo das existentes, e entrada no dispatch do `main`:

```bash
# ---------------------------------------------------------------- mstodo ----
setup_mstodo() {
  step "Tarefas (mstodo) — Microsoft To Do, conta pessoal"
  have mstodo || die "mstodo ausente — rode scripts/install.sh"
  [[ -n "${DAILY_TUI_TODO_CLIENT_ID:-}" ]] \
    || die "defina DAILY_TUI_TODO_CLIENT_ID (veja scripts/daily-tui.env.example)"

  cat <<'EOF'
    Pré-requisito MANUAL (uma vez, no portal Entra):
      1. App registrations > New registration
      2. Supported account types: "Personal Microsoft accounts only"
      3. Authentication > Allow public client flows: Yes
      4. API permissions > Microsoft Graph > Delegated > Tasks.ReadWrite
      5. Copie o Application (client) ID para DAILY_TUI_TODO_CLIENT_ID
EOF
  mstodo auth || warn "mstodo auth falhou — repita depois"

  step "Validando tarefas"
  probe "tasks (mstodo)" "mstodo list" "refaça: mstodo auth"
}
```

No `main`, acrescentar o caso `mstodo) setup_mstodo ;;` junto dos existentes
(`email`, `google`, `doctor`) e citar `mstodo` no texto de uso do script.

- [ ] **Step 6: Verificar**

```bash
bash -n scripts/setup-auth.sh && bash -n scripts/install.sh && echo "sintaxe ok"
grep -rn "gtasks" scripts/setup-auth.sh scripts/install.sh scripts/daily-tui.env.example
```

Esperado: `sintaxe ok`, e do `grep` só a linha de fallback do nome antigo em
`setup-auth.sh` (a que existe de propósito).

- [ ] **Step 7: Commit**

```bash
git add scripts/install.sh scripts/setup-auth.sh scripts/daily-tui.env.example
git commit -m "chore(setup): install and authorize mstodo instead of gtasks on Linux"
```

---

### Task 4: Setup e configuração no Windows

**Files:**
- Modify: `scripts/daily-tui.config.example.ps1`
- Modify: `scripts/daily-tui.config.ps1` (config local do João, não versionada)
- Modify: `scripts/daily-tui-launch.ps1:72-78`
- Modify: `scripts/google-auth.ps1:11, 27, 53-63`

**Interfaces:**
- Consumes: a CLI `mstodo` da Task 1.
- Produces: `DAILY_TUI_TODO_CLIENT_ID` e `DAILY_TUI_TODO_LIST` no ambiente do daily-tui no Windows.

- [ ] **Step 1: Acrescentar a seção no `daily-tui.config.example.ps1`**

Depois do bloco do Jira:

```powershell
# --- Microsoft To Do (tarefas) -----------------------------------------------
# Application (client) ID do app registration (portal Entra, conta pessoal).
$TodoClientId = '00000000-0000-0000-0000-000000000000'
# Nome da lista do To Do; vazio = lista padrao ("Tarefas").
$TodoList     = ''
```

Repetir os mesmos dois `$Todo*` no `scripts/daily-tui.config.ps1` local, com o
client id de verdade.

- [ ] **Step 2: Exportar as variáveis no launcher**

Em `daily-tui-launch.ps1`, no bloco de variáveis de ambiente (linha 72-78),
acrescentar depois do `JIRA_CLOUD`:

```powershell
$env:DAILY_TUI_TODO_CLIENT_ID = $TodoClientId
$env:DAILY_TUI_TODO_LIST      = $TodoList
```

- [ ] **Step 3: Tirar as tarefas do `google-auth.ps1` e usar o nome novo do client**

- linha 11: `# Uso:  google-auth.ps1              # work + personal (agenda)`
- linhas 27-31: passar a procurar o nome novo com fallback para o antigo:

```powershell
$secretDir = "$env:USERPROFILE\.config\daily-tui"
$secret = "$secretDir\google-client-secret.json"
if (-not (Test-Path $secret)) { $secret = "$secretDir\gtasks-client-secret.json" }
if (-not (Test-Path $secret)) {
    Write-Error "client secret nao encontrado em $secretDir (google-client-secret.json)"
    exit 1
}
```

- linhas 53-63: apagar o bloco inteiro de tarefas (`$gtasksToken` até o
  `& gtasks auth`). A autorização do To Do é `mstodo auth`, que não usa OAuth do
  Google e por isso não pertence a este script.

- [ ] **Step 4: Verificar sintaxe dos scripts PowerShell**

```powershell
foreach ($f in 'daily-tui-launch.ps1','google-auth.ps1','daily-tui.config.example.ps1') {
  $tokens = $null; $errs = $null
  [void][System.Management.Automation.Language.Parser]::ParseFile(
    (Resolve-Path "scripts\$f"), [ref]$tokens, [ref]$errs)
  "$f -> $($errs.Count) erro(s)"
}
```

As duas variáveis precisam existir antes: `[ref]` de variável inexistente não
liga ao parâmetro.

Esperado: `0 erro(s)` nos três.

- [ ] **Step 5: Rodar o painel de verdade**

Fechar o daily-tui se estiver aberto (ele trava o binário), então:

```powershell
cargo build --release
scripts\daily-tui-launch.ps1
```

Esperado: o painel de tarefas lista as tarefas do To Do, com acentos corretos, e
as teclas de criar/concluir/editar/apagar funcionam.

- [ ] **Step 6: Commit**

```bash
git add scripts/daily-tui.config.example.ps1 scripts/daily-tui-launch.ps1 scripts/google-auth.ps1
git commit -m "chore(setup): wire mstodo config into the Windows launcher"
```

---

### Task 5: Remover o `gtasks` e atualizar o README

**Files:**
- Delete: `scripts/gtasks`, `scripts/gtasks.cmd`
- Modify: `README.md` (linhas 10, 58, 94, 101, 182, 197-198, 206, 209, 264, 333)

**Interfaces:**
- Consumes: tudo das tasks anteriores.
- Produces: repo sem nenhuma referência ao Google Tasks fora do histórico.

- [ ] **Step 1: Apagar os arquivos**

```bash
git rm scripts/gtasks scripts/gtasks.cmd
```

- [ ] **Step 2: Atualizar o README**

- linha 10: `- ✅ **Tarefas** do Microsoft To Do, com criar/concluir/editar/apagar pela TUI (via \`mstodo\`).`
- linha 58: `6. copia os helpers \`jirapending\` e \`mstodo\` para \`~/.local/bin\`.`
- linha 94: na tabela de helpers, trocar a linha do `gtasks` por
  `| \`mstodo\` | Tarefas | CLI (Python/uv) para o Microsoft To Do com CRUD | \`scripts/mstodo\` (repo) |`
- linha 101: `| \`uv\` | \`gcalcli\`, \`mstodo\` | runner Python self-contained (não precisa de venv manual) |`
- linha 182: `2. habilite a **Google Calendar API**;` (sai a Tasks API)
- linhas 197-198: trocar o parágrafo do `gtasks auth` por: o client OAuth do
  Google vai para `~/.config/daily-tui/google-client-secret.json` (só a agenda);
  as tarefas se autorizam com `scripts/setup-auth.sh mstodo`, que roda
  `mstodo auth` (device code) e guarda o token em
  `~/.local/share/daily-tui/mstodo-personal.json`.
- linha 206: trocar `gtasks list` por `mstodo list` na lista de comandos que
  devem sair sem erro.
- linha 209: ajustar a menção ao `gtasks-client-secret.json` para
  `google-client-secret.json`.
- linha 264: `Os helpers \`jirapending\` e \`mstodo\` já são configuráveis por variáveis de ambiente`
- linha 333: na tabela de troubleshooting, trocar a linha do gtasks por
  `| \`mstodo: sem credenciais — rode: mstodo auth\` | falta autorizar; rode \`mstodo auth\` (ou \`setup-auth.sh mstodo\`). |`

Acrescentar na seção de setup o roteiro do app registration (os cinco passos do
pré-requisito manual deste plano).

- [ ] **Step 3: Gate final — nenhuma referência sobrando**

```bash
grep -rn "gtasks\|Google Tasks\|GTASKS" --exclude-dir=target --exclude-dir=.git . \
  | grep -v "docs/superpowers/"
```

Esperado: só as duas linhas de fallback deliberadas (`setup-auth.sh` e
`google-auth.ps1`). Qualquer outra saída é referência esquecida. Os arquivos em
`docs/superpowers/` (spec e plano) citam o `gtasks` de propósito, como registro
histórico.

- [ ] **Step 4: Suíte e lint**

```bash
cargo test
cargo clippy --all-targets 2>&1 | tail -3
```

Esperado: 68 testes passando; warnings iguais à base.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore(tasks): drop gtasks and document the Microsoft To Do setup"
```

---

---

### Task 6: Alinhar a documentação de auth com o client em uso

Acrescentada em 2026-08-03. As Tasks 3 e 4 documentaram o caminho do app
registration, que não foi o adotado. Deliverable: nenhum arquivo instrui o
usuário a criar um app quando o padrão é o client público.

**Files:**
- Modify: `scripts/daily-tui.env.example` (seção do To Do)
- Modify: `scripts/daily-tui.config.example.ps1` (seção do To Do)
- Modify: `scripts/setup-auth.sh` (o heredoc de `setup_mstodo`)

**Interfaces:**
- Consumes: a decisão de auth registrada no spec.
- Produces: nada que outra task consuma.

- [ ] **Step 1: `daily-tui.env.example`**

Trocar a seção do To Do por: o client público como valor padrão e comentado o
motivo, com o roteiro do portal reduzido a uma linha de "plano B".

```bash
# --- Microsoft To Do (mstodo) ---------------------------------------------
# Client publico first-party da Microsoft ("Microsoft Graph Command Line
# Tools"), autorizado por device code — nao exige app registration proprio.
export DAILY_TUI_TODO_CLIENT_ID="14d82eec-204b-4c2f-b7e8-296a70dab67e"
# Plano B, se a Microsoft restringir Tasks.ReadWrite nesse client: registre um
# app proprio no portal Entra (Personal Microsoft accounts only, public client
# flows habilitado, permissao delegada Tasks.ReadWrite) e ponha o client id aqui.
# Nome da lista do To Do; vazio ou ausente = lista padrão ("Tarefas").
# export DAILY_TUI_TODO_LIST="Trabalho"
# Cache de token (default: ~/.local/share/daily-tui/mstodo-personal.json)
# export MSTODO_TOKEN="$HOME/.local/share/daily-tui/mstodo-personal.json"
```

- [ ] **Step 2: `daily-tui.config.example.ps1`**

Mesmo tratamento, em ASCII puro (o arquivo é lido como ANSI pelo PowerShell 5.1):

```powershell
# --- Microsoft To Do (tarefas) -----------------------------------------------
# Client publico first-party da Microsoft ("Microsoft Graph Command Line Tools"),
# autorizado por device code — nao exige app registration proprio.
$TodoClientId = '14d82eec-204b-4c2f-b7e8-296a70dab67e'
# Nome da lista do To Do; vazio = lista padrao ("Tarefas").
$TodoList     = ''
```

- [ ] **Step 3: o heredoc de `setup_mstodo` no `setup-auth.sh`**

Hoje imprime os cinco passos do portal como pré-requisito obrigatório. Passa a
dizer que o padrão do `daily-tui.env.example` já serve e que o `auth` é
device code; o portal vira nota de plano B. Manter o `die` que exige
`DAILY_TUI_TODO_CLIENT_ID`, porque a variável continua obrigatória.

```bash
  cat <<'EOF'
    O client padrão do daily-tui.env.example é o client público first-party da
    Microsoft e não exige nenhum cadastro. O passo a seguir abre o device code:
    você digita o código exibido em https://www.microsoft.com/link.

    Plano B (só se o escopo Tasks.ReadWrite for restringido nesse client):
      registre um app no portal Entra — Personal Microsoft accounts only,
      "Allow public client flows: Yes", permissão delegada Tasks.ReadWrite —
      e troque DAILY_TUI_TODO_CLIENT_ID pelo Application (client) ID dele.
EOF
```

- [ ] **Step 4: Verificar**

```bash
bash -n scripts/setup-auth.sh && echo "sintaxe ok"
grep -rn "App registrations\|app registration" scripts/
python -c "
for f in ['scripts/daily-tui.config.example.ps1']:
    b=open(f,'rb').read()
    bad=[i for i,c in enumerate(b) if c>127]
    print(f, 'nao-ASCII:', len(bad))
"
```

Esperado: `sintaxe ok`; o grep só encontra menções na forma de plano B; zero
bytes não-ASCII no `.ps1`.

- [ ] **Step 5: Commit**

```bash
git add scripts/daily-tui.env.example scripts/daily-tui.config.example.ps1 scripts/setup-auth.sh
git commit -m "docs(setup): default to the public Graph client for To Do auth"
```

---

## Ordem e dependências

1. **Task 1** (helper) — independente; o `auth` foi resolvido com o client público (ver acima).
2. **Task 2** (Rust) — depende da Task 1 (usa a saída real no teste).
3. **Task 3** (Linux) e **Task 4** (Windows) — independentes entre si, ambas depois da Task 1.
4. **Task 5** (remoção + README) — depois da Task 2, porque o gate é o grep vazio e a última referência a `gtasks` sai em `tasks.rs`.
5. **Task 6** (doc de auth) — por último; corrige o que as Tasks 3 e 4 escreveram antes da decisão mudar.
