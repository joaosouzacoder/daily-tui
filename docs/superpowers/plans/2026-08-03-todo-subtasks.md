# To Do Subtasks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ver e marcar as subtarefas de uma tarefa do Microsoft To Do direto no painel, sem abrir o app.

**Architecture:** O helper `mstodo` passa a pedir `$expand=checklistItems` na mesma chamada do `list` (custo zero por tarefa) e ganha `check`/`uncheck`. No Rust, `TaskItem` ganha `subtasks`, e o cursor do painel passa a indexar **linhas** — tarefa ou subtarefa — em vez de tarefas.

**Tech Stack:** Rust (ratatui, serde_json), Python 3.10+ via `uv`, Microsoft Graph v1.0 (`checklistItems`).

**Spec:** `docs/superpowers/specs/2026-08-03-panel-interactions-design.md`, seção "To Do com subtarefas".

## Global Constraints

- Contrato JSON do `mstodo list` passa a incluir `subtasks`:
  `[{"id","title","completed","due","notes","subtasks":[{"id","title","completed"}]}]`
- `subtasks` é `[]` quando a tarefa não tem nenhuma.
- Endpoints: `PATCH /me/todo/lists/{lid}/tasks/{tid}/checklistItems/{cid}` com `{"isChecked": true|false}`.
- Mapeamento do `checklistItem`: `id` ← `id`, `title` ← `displayName`, `completed` ← `isChecked`.
- Erros do helper: uma linha no stderr, sem traceback (o `mstodo` já tem o `try/except` que garante isso no `main()` — não removê-lo).
- Comentários e mensagens em português; commits em inglês.
- **Fixtures: forma do real, conteúdo inventado.** O repo é público. Derive a
  estrutura da saída real (nomes de campo, formato de id, presença/ausência de
  cada campo, um título acentuado para exercitar UTF-8), mas **não** copie
  títulos, ids ou domínios reais para dentro de arquivo versionado. A regra de
  "derivar do real" existe para o contrato não derivar do que a ferramenta
  emite — não para publicar dado pessoal ou da empresa.
- Teclas do painel de Tarefas: `Enter` expande/recolhe as subtarefas da tarefa sob o cursor; `Space` age na linha sob o cursor (tarefa ou subtarefa). `Enter` numa tarefa sem subtarefas não faz nada.
- `cargo test` e `cargo clippy --all-targets` sem regressão; os 3 warnings pré-existentes (`src/app.rs:115`, `src/app.rs:119`, `src/data/email.rs:90`) ficam como estão.
- Não rodar `cargo build --release` nem o TUI.
- `mstodo` é a lista pessoal real do João: nos smoke tests, desfaça o que marcar.

## File Structure

| Arquivo | Responsabilidade |
| --- | --- |
| `scripts/mstodo` (modificar) | `$expand` no `list`, mapeamento das subtarefas, subcomandos `check`/`uncheck`. |
| `src/data/tasks.rs` (modificar) | `SubTask`, `TaskItem.subtasks`, `check`/`uncheck`, e a função pura que achata tarefas em linhas. |
| `src/app.rs` (modificar) | Conjunto de tarefas expandidas, cursor sobre linhas, teclas `Enter` e `Space`. |
| `src/ui.rs` (modificar) | Render das subtarefas indentadas. |
| `src/msg.rs`, `src/worker.rs` (modificar) | Comando de marcar subtarefa. |

**A decisão estrutural:** hoje o cursor do painel indexa tarefas e cada tarefa é uma linha. Com subtarefas expandidas, linhas ≠ tarefas. A solução espelha a do painel de Jira: uma função pura em `tasks.rs` achata em `Vec<TaskRow>`, e o cursor indexa linhas diretamente — diferente do Jira, aqui **toda** linha é selecionável, então não há tradução de índice, mas expandir/recolher muda a contagem e exige reancorar o cursor.

---

### Task 1: `mstodo` com subtarefas

Deliverable: `mstodo list` traz `subtasks` e `mstodo check` alterna um item na conta real.

**Files:**
- Modify: `scripts/mstodo`

**Interfaces:**
- Consumes: nada.
- Produces: `list` com o campo `subtasks`; `check <task-id> <item-id>` e `uncheck <task-id> <item-id>`, silenciosos em caso de sucesso.

- [ ] **Step 1: Pedir as subtarefas no `list`**

Em `do_list`, a chamada passa a expandir os checklistItems. Trocar a linha do
`in_list("GET", "?$top=100")` por:

```python
    resp = in_list("GET", "?$top=100&$expand=checklistItems")
```

E o `to_item` ganha o campo, mapeando os três campos que o painel usa:

```python
def to_item(task):
    """Converte um todoTask do Graph no contrato que o daily-tui espera."""
    due = ((task.get("dueDateTime") or {}).get("dateTime") or "")[:10]
    return {
        "id": task["id"],
        "title": (task.get("title") or "").strip(),
        "completed": task.get("status") == "completed",
        "due": due,
        "notes": ((task.get("body") or {}).get("content") or "").strip(),
        "subtasks": [
            {
                "id": item["id"],
                "title": (item.get("displayName") or "").strip(),
                "completed": bool(item.get("isChecked")),
            }
            for item in (task.get("checklistItems") or [])
        ],
    }
```

- [ ] **Step 2: Subcomandos `check` e `uncheck`**

Ao lado de `do_complete`/`do_reopen`:

```python
def do_check(task_id, item_id, checked):
    in_list("PATCH", f"/{task_id}/checklistItems/{item_id}", json={"isChecked": checked})
```

E no dispatch do `main`, junto dos outros:

```python
    elif cmd == "check":
        do_check(rest[0], rest[1], True)
    elif cmd == "uncheck":
        do_check(rest[0], rest[1], False)
```

Atualizar o docstring do módulo, que lista os subcomandos, incluindo as duas
linhas novas:

```
  mstodo check <tarefa> <item>
  mstodo uncheck <tarefa> <item>
```

- [ ] **Step 3: Verificar o `list`**

```bash
export DAILY_TUI_TODO_CLIENT_ID="14d82eec-204b-4c2f-b7e8-296a70dab67e" PYTHONIOENCODING=utf-8
uv run --script scripts/mstodo list | python -c "
import json,sys
d=json.load(sys.stdin)
assert all('subtasks' in t for t in d), 'toda tarefa precisa do campo'
com=[t for t in d if t['subtasks']]
print(f'{len(d)} tarefas, {len(com)} com subtarefas')
t=com[0]; s=t['subtasks'][0]
assert sorted(s)==['completed','id','title'], sorted(s)
print('contrato da subtarefa ok:', s['title'][:30], '| completed =', s['completed'])
"
```

Esperado: em 2026-08-03 eram 8 tarefas e 3 com subtarefas; as três chaves exatas.

- [ ] **Step 4: Smoke test de `check`/`uncheck` — e desfazer**

```bash
IDS=$(uv run --script scripts/mstodo list | python -c "
import json,sys
d=json.load(sys.stdin)
t=[x for x in d if x['subtasks'] and not x['subtasks'][0]['completed']][0]
print(t['id'], t['subtasks'][0]['id'], t['subtasks'][0]['title'])
")
set -- $IDS
uv run --script scripts/mstodo check "$1" "$2"
uv run --script scripts/mstodo list | python -c "
import json,sys; d=json.load(sys.stdin)
print('marcada:', [s['completed'] for t in d if t['id']=='$1' for s in t['subtasks'] if s['id']=='$2'])"
uv run --script scripts/mstodo uncheck "$1" "$2"
uv run --script scripts/mstodo list | python -c "
import json,sys; d=json.load(sys.stdin)
print('desmarcada:', [s['completed'] for t in d if t['id']=='$1' for s in t['subtasks'] if s['id']=='$2'])"
```

Esperado: `marcada: [True]` e depois `desmarcada: [False]`. A lista tem de
terminar como começou — é a lista pessoal do João.

- [ ] **Step 5: Guardar a saída real para a Task 2**

```bash
uv run --script scripts/mstodo list > /tmp/mstodo-subtasks.json
```

- [ ] **Step 6: Commit**

```bash
git add scripts/mstodo
git commit -m "feat(mstodo): expose and toggle checklist items"
```

---

### Task 2: Painel com subtarefas expansíveis

Deliverable: `Enter` expande as subtarefas indentadas e `Space` marca a linha sob o cursor, seja tarefa ou subtarefa.

**Files:**
- Modify: `src/data/tasks.rs`
- Modify: `src/app.rs`
- Modify: `src/ui.rs`
- Modify: `src/msg.rs`, `src/worker.rs`

**Interfaces:**
- Consumes: a CLI `mstodo` da Task 1 e a saída real em `/tmp/mstodo-subtasks.json`.
- Produces:
  ```rust
  pub struct SubTask { pub id: String, pub title: String, pub completed: bool }
  // TaskItem ganha: pub subtasks: Vec<SubTask>
  pub enum TaskRow { Task(usize), Sub { task: usize, sub: usize } }
  pub fn rows(items: &[TaskItem], expanded: &HashSet<String>) -> Vec<TaskRow>
  pub fn check(task_id: &str, item_id: &str) -> Result<(), String>
  pub fn uncheck(task_id: &str, item_id: &str) -> Result<(), String>
  ```
  `App` ganha `tasks_expanded: HashSet<String>` (ids de tarefas expandidas).

- [ ] **Step 1: Testes que falham — contrato e achatamento**

No `mod tests` de `src/data/tasks.rs`, com um item derivado de
`/tmp/mstodo-subtasks.json` (troque id e títulos pelos reais, escolhendo uma
tarefa inócua):

```rust
    // Saída real de `mstodo list` com subtarefas (2026-08-03).
    const REAL_SUB: &str = r#"[
      {"id":"T1","title":"Trocar a instalação elétrica","completed":false,"due":"","notes":"",
       "subtasks":[{"id":"S1","title":"Medir a fiação","completed":true},
                   {"id":"S2","title":"Comprar disjuntor","completed":false}]},
      {"id":"T2","title":"Sem filhos","completed":false,"due":"","notes":"","subtasks":[]}
    ]"#;

    #[test]
    fn parses_subtasks_from_real_output() {
        let tasks = parse_tasks(REAL_SUB).unwrap();
        assert_eq!(tasks[0].subtasks.len(), 2);
        assert_eq!(tasks[0].subtasks[0].id, "S1");
        assert!(tasks[0].subtasks[0].completed);
        assert_eq!(tasks[0].subtasks[1].title, "Comprar disjuntor");
        assert!(tasks[1].subtasks.is_empty());
    }

    #[test]
    fn missing_subtasks_field_defaults_to_empty() {
        // Compatibilidade: um `mstodo` antigo não emitia o campo.
        let tasks = parse_tasks(r#"[{"id":"a","title":"t","completed":false}]"#).unwrap();
        assert!(tasks[0].subtasks.is_empty());
    }

    #[test]
    fn rows_are_tasks_only_when_nothing_is_expanded() {
        let tasks = parse_tasks(REAL_SUB).unwrap();
        let rows = rows(&tasks, &HashSet::new());
        assert_eq!(rows, vec![TaskRow::Task(0), TaskRow::Task(1)]);
    }

    #[test]
    fn expanding_inserts_the_subtasks_right_below_the_task() {
        let tasks = parse_tasks(REAL_SUB).unwrap();
        let expanded = HashSet::from(["T1".to_string()]);
        let rows = rows(&tasks, &expanded);
        assert_eq!(
            rows,
            vec![
                TaskRow::Task(0),
                TaskRow::Sub { task: 0, sub: 0 },
                TaskRow::Sub { task: 0, sub: 1 },
                TaskRow::Task(1),
            ]
        );
    }

    #[test]
    fn expanding_a_task_without_subtasks_adds_no_rows() {
        let tasks = parse_tasks(REAL_SUB).unwrap();
        let expanded = HashSet::from(["T2".to_string()]);
        assert_eq!(rows(&tasks, &expanded).len(), 2);
    }
```

- [ ] **Step 2: Rodar e confirmar a falha**

```bash
cargo test --lib data::tasks
```

Esperado: FAIL na compilação — `SubTask`, `subtasks`, `TaskRow` e `rows` não existem.

- [ ] **Step 3: Implementar em `src/data/tasks.rs`**

```rust
use std::collections::HashSet;

/// Uma subtarefa (checklistItem no Graph; "etapa" no app To Do).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SubTask {
    pub id: String,
    pub title: String,
    pub completed: bool,
}
```

No `TaskItem`, acrescentar o campo com default, para tolerar saída de um helper
antigo:

```rust
    #[serde(default)]
    pub subtasks: Vec<SubTask>,
```

E as funções novas:

```rust
/// Uma linha renderizada do painel: uma tarefa ou uma subtarefa dela.
///
/// Diferente do painel de Jira, aqui **toda** linha é selecionável — o cursor
/// indexa linhas direto, sem tradução. Expandir muda quantas linhas existem, e
/// é por isso que quem expande precisa reancorar o cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRow {
    Task(usize),
    Sub { task: usize, sub: usize },
}

/// Achata as tarefas em linhas, intercalando as subtarefas das expandidas.
pub fn rows(items: &[TaskItem], expanded: &HashSet<String>) -> Vec<TaskRow> {
    let mut out = Vec::new();
    for (t, item) in items.iter().enumerate() {
        out.push(TaskRow::Task(t));
        if expanded.contains(&item.id) {
            out.extend((0..item.subtasks.len()).map(|sub| TaskRow::Sub { task: t, sub }));
        }
    }
    out
}

/// Marca uma subtarefa como concluída.
pub fn check(task_id: &str, item_id: &str) -> Result<(), String> {
    run(&["check", task_id, item_id]).map(|_| ())
}

/// Desmarca uma subtarefa.
pub fn uncheck(task_id: &str, item_id: &str) -> Result<(), String> {
    run(&["uncheck", task_id, item_id]).map(|_| ())
}
```

- [ ] **Step 4: Confirmar que passa**

```bash
cargo test --lib data::tasks
```

Esperado: os 5 testes novos e os 4 existentes passando.

- [ ] **Step 5: Teste que falha para as teclas**

No `mod tests` de `src/app.rs`:

```rust
    #[test]
    fn enter_expands_only_tasks_that_have_subtasks() {
        let mut app = test_app();
        app.tasks.items = crate::data::tasks::parse_tasks(
            r#"[{"id":"T1","title":"com filhos","completed":false,"due":"","notes":"",
                 "subtasks":[{"id":"S1","title":"um","completed":false}]},
                {"id":"T2","title":"sem filhos","completed":false,"due":"","notes":"","subtasks":[]}]"#,
        )
        .unwrap();
        app.tasks.loaded = true;
        // Tab até o painel de Tarefas.
        for _ in 0..4 {
            app.update(key(KeyCode::Tab));
        }
        assert_eq!(app.focus, Panel::Tasks);

        app.update(key(KeyCode::Enter));
        assert!(app.tasks_expanded.contains("T1"), "expande a que tem filhos");
        app.update(key(KeyCode::Enter));
        assert!(!app.tasks_expanded.contains("T1"), "Enter de novo recolhe");

        app.update(key(KeyCode::Char('j'))); // cursor -> T2
        app.update(key(KeyCode::Enter));
        assert!(app.tasks_expanded.is_empty(), "tarefa sem filhos não expande");
    }
```

- [ ] **Step 6: Rodar e confirmar a falha**

```bash
cargo test --lib app::tests::enter_expands_only_tasks_that_have_subtasks
```

Esperado: FAIL — `tasks_expanded` não existe.

- [ ] **Step 7: Estado e teclas em `src/app.rs`**

No `struct App`, acrescentar `pub tasks_expanded: std::collections::HashSet<String>,`
(inicializado com `HashSet::new()`).

O `Enter` já é um `match self.focus` (introduzido no plano do Jira; se este plano
rodar antes, transforme o `KeyCode::Enter => self.open_detail(),` no match).
Acrescentar o braço de Tarefas:

```rust
                Panel::Tasks => self.toggle_expand(),
```

E os métodos:

```rust
    /// Expande ou recolhe as subtarefas da linha sob o cursor.
    ///
    /// Reancora o cursor na própria tarefa depois da operação: recolher encurta
    /// a lista de linhas, e manter o índice antigo apontaria para outra coisa.
    fn toggle_expand(&mut self) {
        let rows = tasks::rows(&self.tasks.items, &self.tasks_expanded);
        let Some(row) = rows.get(self.tasks.cursor) else { return };
        let t = match row {
            tasks::TaskRow::Task(t) => *t,
            tasks::TaskRow::Sub { task, .. } => *task,
        };
        let Some(item) = self.tasks.items.get(t) else { return };
        if item.subtasks.is_empty() {
            return;
        }
        if !self.tasks_expanded.remove(&item.id) {
            self.tasks_expanded.insert(item.id.clone());
        }
        // Reancora: o cursor volta para a linha da tarefa.
        let rows = tasks::rows(&self.tasks.items, &self.tasks_expanded);
        if let Some(pos) = rows.iter().position(|r| *r == tasks::TaskRow::Task(t)) {
            self.tasks.cursor = pos;
        }
    }
```

- [ ] **Step 8: Fazer o `Space` decidir entre tarefa e subtarefa**

O `toggle_task` atual assume que o cursor indexa tarefas. Passa a resolver a
linha primeiro:

```rust
    /// Alterna o estado da linha sob o cursor: tarefa ou subtarefa.
    fn toggle_task(&mut self) {
        let rows = tasks::rows(&self.tasks.items, &self.tasks_expanded);
        let Some(row) = rows.get(self.tasks.cursor) else { return };
        let cmd = match row {
            tasks::TaskRow::Task(t) => {
                let item = &self.tasks.items[*t];
                if item.completed {
                    WorkerCmd::TaskReopen(item.id.clone())
                } else {
                    WorkerCmd::TaskComplete(item.id.clone())
                }
            }
            tasks::TaskRow::Sub { task, sub } => {
                let item = &self.tasks.items[*task];
                let s = &item.subtasks[*sub];
                WorkerCmd::SubTaskToggle {
                    task_id: item.id.clone(),
                    item_id: s.id.clone(),
                    check: !s.completed,
                }
            }
        };
        let _ = self.cmd_tx.send(cmd);
    }
```

Os nomes exatos das variantes de `WorkerCmd` para tarefa (`TaskComplete`,
`TaskReopen`) estão em `src/worker.rs` — use os que existem lá, não estes se
divergirem.

Também ajustar as ações que hoje usam `self.tasks.cursor` como índice de tarefa
(`open_edit_task`, `open_delete_task`): elas passam a resolver a linha e a ignorar
o caso `Sub`, porque editar e apagar valem só para tarefas.

- [ ] **Step 9: Comando no worker**

Em `src/worker.rs`, a variante nova e o tratamento (que re-busca a lista depois,
como as outras escritas):

```rust
    /// Marca ou desmarca uma subtarefa; re-busca a lista depois.
    SubTaskToggle { task_id: String, item_id: String, check: bool },
```

O `worker.rs` já tem uma função que aplica uma escrita e re-busca a lista — no
código atual é ela que serve `TaskComplete`, `TaskReopen`, `TaskAdd`, `TaskEdit`
e `TaskDelete`, todos passando um closure que devolve `Result<(), String>`. A
variante nova entra no mesmo caminho, sem inventar outro:

```rust
        WorkerCmd::SubTaskToggle { task_id, item_id, check } => {
            apply_task_write(&tx, || {
                if check {
                    data::tasks::check(&task_id, &item_id)
                } else {
                    data::tasks::uncheck(&task_id, &item_id)
                }
            });
        }
```

O nome real dessa função e a assinatura exata estão em `src/worker.rs` (procure
por onde `TaskComplete` é tratado). Se ela não existir como função separada e o
padrão for inline, replique o inline — o requisito é que o erro vá para o painel
e a lista seja re-buscada, igual às outras escritas.

- [ ] **Step 10: Render das subtarefas indentadas**

Em `src/ui.rs`, `render_tasks` passa a montar as linhas a partir de `rows`:

```rust
    let rows = tasks::rows(&p.items, &app.tasks_expanded);
    let selected = focused.then_some(p.cursor);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| {
            let line = match row {
                tasks::TaskRow::Task(t) => task_line(&p.items[*t], theme),
                tasks::TaskRow::Sub { task, sub } => {
                    subtask_line(&p.items[*task].subtasks[*sub], theme)
                }
            };
            highlight(line, theme, selected == Some(row_idx))
        })
        .collect();
```

E a linha da subtarefa, indentada e com o mesmo checkbox:

```rust
/// Linha de uma subtarefa: indentada sob a tarefa, checkbox e título.
fn subtask_line(s: &SubTask, theme: &BubbleTheme) -> Line<'static> {
    if s.completed {
        Line::from(vec![theme.muted("      [x] "), theme.muted(clip(&s.title, 36))])
    } else {
        Line::from(vec![theme.span("      [ ] "), theme.span(clip(&s.title, 36))])
    }
}
```

O `window(lines.len(), p.cursor, ...)` continua correto sem mudança: aqui o
cursor já indexa linhas.

Marcar a tarefa expandida no `task_line` com um indicador — um `▾` antes do
checkbox quando expandida e `▸` quando tem subtarefas e está recolhida — para o
usuário saber que há algo escondido. Tarefas sem subtarefas não recebem
indicador.

- [ ] **Step 11: Teste de render**

```rust
    #[test]
    fn tasks_panel_renders_expanded_subtasks_indented() {
        let mut app = test_app();
        app.tasks.items = crate::data::tasks::parse_tasks(
            r#"[{"id":"T1","title":"pai","completed":false,"due":"","notes":"",
                 "subtasks":[{"id":"S1","title":"filha","completed":false}]}]"#,
        )
        .unwrap();
        app.tasks.loaded = true;
        app.tasks_expanded.insert("T1".to_string());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("pai"));
        assert!(out.contains("      [ ] filha"), "subtarefa indentada");
    }
```

- [ ] **Step 12: Suíte e lint**

```bash
cargo test
cargo clippy --all-targets 2>&1 | grep "^warning:" | grep -v "generated" | wc -l
```

Esperado: tudo passando; warnings = 3.

- [ ] **Step 13: Reinstalar o helper no PATH**

O painel invoca `mstodo` pelo nome, e o que está em `~/.local/bin` é uma **cópia**:

```bash
cp scripts/mstodo scripts/mstodo.cmd "$USERPROFILE/.local/bin/"
```

Sem isso o painel continua rodando a versão sem subtarefas.

- [ ] **Step 14: Atualizar o README**

A lista de teclas ganha as duas do painel de Tarefas (`Enter` expande/recolhe,
`Space` agindo em tarefa **ou** subtarefa), e a linha de feature das tarefas passa
a mencionar as subtarefas. Registre também que subtarefa no To Do é o que o app
chama de "etapa" — quem procurar "subtarefa" na interface da Microsoft não acha.

- [ ] **Step 15: Commit**

```bash
git add src/data/tasks.rs src/app.rs src/ui.rs src/msg.rs src/worker.rs README.md
git commit -m "feat(tasks): expand and toggle To Do subtasks in the panel"
```

---

## Ordem e dependências

1. **Task 1** (helper) — independente.
2. **Task 2** (painel) — depende da Task 1, inclusive da saída real capturada no Step 5 dela.

Este plano é independente do plano do Jira; só há sobreposição no `KeyCode::Enter`,
que os dois transformam em `match self.focus`. Quem rodar primeiro faz a
transformação; o segundo só acrescenta seu braço.
