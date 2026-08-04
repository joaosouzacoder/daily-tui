# Email Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Agir sobre o e-mail selecionado sem sair do painel: marcar lido/não lido, mover para uma pasta e excluir.

**Architecture:** Três funções novas em `src/data/email.rs` chamando o `himalaya` já instalado, expostas por comandos do worker que re-buscam a lista depois — o mesmo padrão que o painel de Tarefas usa. Na interface, uma tecla direta para o toggle de lido, um seletor de pasta e uma confirmação para excluir.

**Tech Stack:** Rust (ratatui), himalaya 1.2.0 CLI.

**Spec:** `docs/superpowers/specs/2026-08-03-panel-interactions-design.md`, seção "E-mail com ações".

## Global Constraints

- Comandos do himalaya, verificados na versão 1.2.0 instalada:
  - marcar lido: `himalaya flag add <id> seen -a <conta>`
  - marcar não lido: `himalaya flag remove <id> seen -a <conta>`
  - mover: `himalaya message move <pasta> <id> -a <conta>`
  - excluir: `himalaya message move trash <id> -a <conta>`
- **Excluir é mover para `trash`**, não `message delete`: o efeito fica explícito e recuperável. `trash` é o alias que a config de cada conta já define.
- Pastas oferecidas no seletor, exatamente as que a config declara: `inbox`, `sent`, `drafts`, `trash`, `spam`, `all`.
- O `Space` **alterna**: escolhe `flag add` ou `flag remove` pelo campo `unread` do envelope sob o cursor.
- Toda escrita re-busca a lista depois; nada de atualização otimista. Erro aparece no painel e a lista fica como estava.
- Teclas do painel de E-mail: `Space` alterna lido, `m` abre o seletor de pasta, `d` pede confirmação e move para a Lixeira. `Enter` continua abrindo o detalhe.
- Comentários e mensagens em português; commits em inglês.
- **Fixtures: forma do real, conteúdo inventado.** O repo é público. Nenhum
  assunto, remetente, endereço ou id de e-mail real vai para arquivo versionado —
  derive só a estrutura.
- `cargo test` e `cargo clippy --all-targets` sem regressão; os 3 warnings pré-existentes (`src/app.rs:115`, `src/app.rs:119`, `src/data/email.rs:90`) ficam.
- Não rodar `cargo build --release` nem o TUI.
- **É a caixa de e-mail real do João.** Nos smoke tests, use um e-mail que você mesmo mova de volta, e prefira a conta `personal`.

## File Structure

| Arquivo | Responsabilidade |
| --- | --- |
| `src/data/email.rs` (modificar) | As três ações, cada uma um `himalaya` só. Nada de estado. |
| `src/app.rs` (modificar) | Teclas do painel, o prompt de seleção de pasta e a confirmação. |
| `src/ui.rs` (modificar) | Render do seletor de pasta no overlay de prompt que já existe. |
| `src/msg.rs`, `src/worker.rs` (modificar) | Comandos de escrita e re-busca. |

---

### Task 1: As três ações no `email.rs` e no worker

Deliverable: as funções existem, são chamáveis pelo worker e verificadas contra a caixa real.

**Files:**
- Modify: `src/data/email.rs`
- Modify: `src/msg.rs`, `src/worker.rs`

**Interfaces:**
- Consumes: `Account` e o `run` de helper que `email.rs` já usa para o `himalaya`.
- Produces:
  ```rust
  pub fn set_seen(account: Account, id: &str, seen: bool) -> Result<(), String>
  pub fn move_to(account: Account, id: &str, folder: &str) -> Result<(), String>
  pub fn delete(account: Account, id: &str) -> Result<(), String>   // move para `trash`
  pub const FOLDERS: [&str; 6] = ["inbox", "sent", "drafts", "trash", "spam", "all"];
  ```
  `WorkerCmd` ganha `EmailSetSeen { account, id, seen }`, `EmailMove { account, id, folder }`, `EmailDelete { account, id }`.

- [ ] **Step 1: Escrever o teste que falha**

`email.rs` hoje não testa execução de processo, e este plano não introduz essa
infra. O que **é** testável sem rede é a escolha do comando — então extraia-a
para uma função pura e teste-a. No `mod tests` de `src/data/email.rs`:

```rust
    #[test]
    fn seen_flag_args_switch_between_add_and_remove() {
        assert_eq!(flag_verb(true), "add");
        assert_eq!(flag_verb(false), "remove");
    }

    #[test]
    fn delete_moves_to_the_trash_alias() {
        assert_eq!(DELETE_FOLDER, "trash");
    }

    #[test]
    fn folders_are_the_ones_the_config_declares() {
        assert_eq!(FOLDERS, ["inbox", "sent", "drafts", "trash", "spam", "all"]);
    }
```

- [ ] **Step 2: Rodar e confirmar a falha**

```bash
cargo test --lib data::email
```

Esperado: FAIL na compilação — `flag_verb`, `DELETE_FOLDER` e `FOLDERS` não existem.

- [ ] **Step 3: Implementar em `src/data/email.rs`**

```rust
/// Pastas oferecidas no seletor de "mover", exatamente as que a config do
/// himalaya declara como alias em cada conta.
pub const FOLDERS: [&str; 6] = ["inbox", "sent", "drafts", "trash", "spam", "all"];

/// Excluir é mover para a Lixeira: recuperável, e é o que o Gmail espera.
pub const DELETE_FOLDER: &str = "trash";

/// Subcomando de `himalaya flag` para ligar ou desligar uma flag.
const fn flag_verb(seen: bool) -> &'static str {
    if seen { "add" } else { "remove" }
}

/// Marca (ou desmarca) o e-mail como lido.
pub fn set_seen(account: Account, id: &str, seen: bool) -> Result<(), String> {
    run(&["flag", flag_verb(seen), id, "seen", "-a", account.himalaya_name()]).map(|_| ())
}

/// Move o e-mail para a pasta dada (nome ou alias conhecido do himalaya).
pub fn move_to(account: Account, id: &str, folder: &str) -> Result<(), String> {
    run(&["message", "move", folder, id, "-a", account.himalaya_name()]).map(|_| ())
}

/// Exclui movendo para a Lixeira.
pub fn delete(account: Account, id: &str) -> Result<(), String> {
    move_to(account, id, DELETE_FOLDER)
}
```

Se `email.rs` ainda não tiver um `run(&[&str])` genérico (hoje ele monta o
`Command` dentro de `fetch` e de `fetch_body`), extraia um — espelhando o `run`
de `src/data/tasks.rs`, com `force_utf8_stdout` e `stderr_summary`, e faça
`fetch`/`fetch_body` passarem a usá-lo. É refatoração dentro do arquivo que este
plano já modifica, não um refactor alheio.

- [ ] **Step 4: Confirmar que passa**

```bash
cargo test --lib data::email
```

Esperado: os 3 testes novos e os 6 existentes passando.

- [ ] **Step 5: Comandos no worker**

Em `src/worker.rs`, as três variantes, e o tratamento seguindo exatamente a forma
das escritas de tarefa (executa, propaga erro, re-busca a lista da conta):

```rust
    /// Marca/desmarca lido; re-busca a lista depois.
    EmailSetSeen { account: Account, id: String, seen: bool },
    /// Move para uma pasta; re-busca a lista depois.
    EmailMove { account: Account, id: String, folder: String },
    /// Move para a Lixeira; re-busca a lista depois.
    EmailDelete { account: Account, id: String },
```

- [ ] **Step 6: Smoke test contra a caixa real**

Use a conta **personal** e um e-mail seu, e desfaça no fim. Pegue um id da lista:

```bash
ID=$(himalaya envelope list -a personal --page-size 1 -o json | python -c "import json,sys;print(json.load(sys.stdin)[0]['id'])")
himalaya flag add "$ID" seen -a personal && echo "marcado como lido"
himalaya envelope list -a personal --page-size 1 -o json | python -c "import json,sys;print('flags:',json.load(sys.stdin)[0]['flags'])"
himalaya flag remove "$ID" seen -a personal && echo "desmarcado"
```

Esperado: as flags mostram `Seen` depois do `add` e não mostram depois do
`remove`. **Não** teste `message move` num e-mail que você não queira mover —
se testar, mova de volta com `himalaya message move inbox "$ID" -a personal`.

- [ ] **Step 7: Commit**

```bash
git add src/data/email.rs src/msg.rs src/worker.rs
git commit -m "feat(email): add seen toggle, move and delete actions"
```

---

### Task 2: Teclas, seletor de pasta e confirmação

Deliverable: no painel de E-mail, `Space` alterna lido, `m` escolhe pasta e `d` exclui após confirmar.

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui.rs`

**Interfaces:**
- Consumes: `set_seen`, `move_to`, `delete`, `FOLDERS` da Task 1 e os `WorkerCmd` correspondentes.
- Produces: duas variantes novas de `Prompt`:
  ```rust
  PickFolder { account: Account, id: String, cursor: usize },
  ConfirmEmailDelete { account: Account, id: String, subject: String },
  ```

- [ ] **Step 1: Teste que falha para o toggle de lido**

No `mod tests` de `src/app.rs`:

```rust
    #[test]
    fn space_on_email_does_not_touch_tasks() {
        let mut app = test_app();
        app.emails.items = vec![EmailItem {
            id: "1".into(), account: Account::Personal, from: "a".into(),
            subject: "s".into(), unread: true, date: "2026-08-03 10:00+00:00".into(),
        }];
        app.emails.loaded = true;
        assert_eq!(app.focus, Panel::Email);
        // Não deve entrar em prompt nem mexer em tarefas; o efeito é um comando
        // enviado ao worker, e o que este teste garante é que nada quebra e o
        // painel segue em Email.
        app.update(key(KeyCode::Char(' ')));
        assert!(app.prompt.is_none());
        assert_eq!(app.focus, Panel::Email);
    }

    #[test]
    fn m_opens_the_folder_picker_and_esc_closes_it() {
        let mut app = test_app();
        app.emails.items = vec![EmailItem {
            id: "42".into(), account: Account::Work, from: "a".into(),
            subject: "s".into(), unread: false, date: "2026-08-03 10:00+00:00".into(),
        }];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('m')));
        match &app.prompt {
            Some(Prompt::PickFolder { id, cursor, .. }) => {
                assert_eq!(id, "42");
                assert_eq!(*cursor, 0);
            }
            other => panic!("esperava PickFolder, veio {other:?}"),
        }
        app.update(key(KeyCode::Char('j')));
        assert!(matches!(&app.prompt, Some(Prompt::PickFolder { cursor: 1, .. })));
        app.update(key(KeyCode::Esc));
        assert!(app.prompt.is_none());
    }

    #[test]
    fn d_on_email_asks_for_confirmation_before_deleting() {
        let mut app = test_app();
        app.emails.items = vec![EmailItem {
            id: "9".into(), account: Account::Personal, from: "a".into(),
            subject: "assunto".into(), unread: false, date: "2026-08-03 10:00+00:00".into(),
        }];
        app.emails.loaded = true;

        app.update(key(KeyCode::Char('d')));
        assert!(matches!(&app.prompt, Some(Prompt::ConfirmEmailDelete { subject, .. }) if subject == "assunto"));
        app.update(key(KeyCode::Char('n')));
        assert!(app.prompt.is_none(), "recusar fecha sem excluir");
    }
```

- [ ] **Step 2: Rodar e confirmar as falhas**

```bash
cargo test --lib app::tests::m_opens_the_folder_picker_and_esc_closes_it
```

Esperado: FAIL — `Prompt::PickFolder` não existe.

- [ ] **Step 3: Variantes de prompt**

Em `src/app.rs`, no `enum Prompt`:

```rust
    /// Seleção de pasta para mover o e-mail sob o cursor.
    PickFolder { account: Account, id: String, cursor: usize },
    /// Confirmação de exclusão do e-mail (move para a Lixeira).
    ConfirmEmailDelete { account: Account, id: String, subject: String },
```

- [ ] **Step 4: Teclas do painel**

Ao lado das teclas de Tarefas em `handle_panel_key`:

```rust
            KeyCode::Char(' ') if self.focus == Panel::Email => self.toggle_email_seen(),
            KeyCode::Char('m') if self.focus == Panel::Email => self.open_move_email(),
            KeyCode::Char('d') if self.focus == Panel::Email => self.open_delete_email(),
```

E os três métodos:

```rust
    /// Alterna lido/não lido do e-mail sob o cursor.
    fn toggle_email_seen(&mut self) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            let _ = self.cmd_tx.send(WorkerCmd::EmailSetSeen {
                account: item.account,
                id: item.id.clone(),
                // `unread` verdadeiro significa que falta a flag Seen: marcar.
                seen: item.unread,
            });
        }
    }

    /// Abre o seletor de pasta para o e-mail sob o cursor.
    fn open_move_email(&mut self) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.prompt = Some(Prompt::PickFolder {
                account: item.account,
                id: item.id.clone(),
                cursor: 0,
            });
        }
    }

    /// Pede confirmação antes de mover o e-mail para a Lixeira.
    fn open_delete_email(&mut self) {
        if let Some(item) = self.emails.items.get(self.emails.cursor) {
            self.prompt = Some(Prompt::ConfirmEmailDelete {
                account: item.account,
                id: item.id.clone(),
                subject: item.subject.clone(),
            });
        }
    }
```

- [ ] **Step 5: Navegação e submissão do prompt**

Em `handle_prompt_key`, o `PickFolder` precisa de `j`/`k` (e setas) movendo o
cursor dentro de `FOLDERS`, `Enter` submetendo e `Esc` fechando. O
`ConfirmEmailDelete` entra no mesmo braço de confirmação que o
`ConfirmDelete` de tarefa já usa (`y`/`Enter` confirma, `n`/`Esc` recusa).

Em `submit_prompt`, os dois casos novos:

```rust
            Some(Prompt::PickFolder { account, id, cursor }) => WorkerCmd::EmailMove {
                account,
                id,
                folder: email::FOLDERS[cursor.min(email::FOLDERS.len() - 1)].to_string(),
            },
            Some(Prompt::ConfirmEmailDelete { account, id, .. }) => {
                WorkerCmd::EmailDelete { account, id }
            }
```

- [ ] **Step 6: Confirmar que os testes passam**

```bash
cargo test --lib app::tests
```

Esperado: os 3 testes novos e os existentes passando.

- [ ] **Step 7: Render do seletor e da confirmação**

Em `src/ui.rs`, `render_prompt` já desenha os dois casos atuais. Acrescentar:

```rust
        Prompt::PickFolder { cursor, .. } => {
            let mut lines = vec![Line::from(theme.span("Mover para:"))];
            lines.extend(email::FOLDERS.iter().enumerate().map(|(i, f)| {
                highlight(Line::from(vec![theme.span(format!("  {f}"))]), theme, i == *cursor)
            }));
            lines.push(Line::from(theme.muted("j/k escolhe · Enter move · Esc cancela")));
            lines
        }
        Prompt::ConfirmEmailDelete { subject, .. } => vec![
            Line::from(theme.span(format!("Mover para a Lixeira: {}", clip(subject, 40)))),
            Line::from(theme.muted("y confirma · n cancela")),
        ],
```

Ajuste a altura do overlay se ele for fixo: o seletor tem 8 linhas (título, seis
pastas, ajuda). O código atual calcula a área do prompt — siga o que ele faz.

- [ ] **Step 8: Teste de render do seletor**

```rust
    #[test]
    fn prompt_overlay_renders_the_folder_picker() {
        let mut app = test_app();
        app.prompt = Some(Prompt::PickFolder {
            account: Account::Personal,
            id: "1".into(),
            cursor: 0,
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| render(&app, f)).unwrap();
        let buf = terminal.backend().buffer();
        let out: String = (0..30)
            .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(out.contains("Mover para:"));
        assert!(out.contains("trash"));
    }
```

- [ ] **Step 9: Suíte e lint**

```bash
cargo test
cargo clippy --all-targets 2>&1 | grep "^warning:" | grep -v "generated" | wc -l
```

Esperado: tudo passando; warnings = 3.

- [ ] **Step 10: Atualizar o README**

A lista de teclas do README ganha as três do painel de E-mail, e a lista de
features passa a dizer que o painel de e-mail age, não só lê. Registre também que
excluir move para a Lixeira — não apaga.

- [ ] **Step 11: Commit**

```bash
git add src/app.rs src/ui.rs README.md
git commit -m "feat(email): keys for seen toggle, move and delete"
```

---

## Ordem e dependências

1. **Task 1** (ações e worker) — independente.
2. **Task 2** (teclas e prompts) — depende da Task 1.

Independente dos outros dois planos. A única sobreposição possível é em
`handle_panel_key` e `submit_prompt`, que os três planos tocam em pontos
diferentes.
