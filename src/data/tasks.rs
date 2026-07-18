//! Tarefas do Google Tasks (conta pessoal) via a CLI `gtasks`.
//!
//! Leitura: `gtasks list` devolve JSON; escrita: `add`/`complete`/`reopen`/
//! `edit`/`delete`. O painel é interativo, então diferente de PRs/Jira aqui os
//! itens são estruturados (precisamos do `id` para agir na tarefa selecionada).

use serde::Deserialize;

/// Uma tarefa do Google Tasks.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub completed: bool,
    /// Data de vencimento `YYYY-MM-DD` (vazio quando não há).
    #[serde(default)]
    pub due: String,
    #[serde(default)]
    pub notes: String,
}

/// Parseia o JSON do `gtasks list` numa lista de tarefas.
pub fn parse_tasks(raw: &str) -> Result<Vec<TaskItem>, String> {
    serde_json::from_str(raw).map_err(|e| format!("JSON inválido do gtasks: {e}"))
}

/// Roda `gtasks list` e devolve as tarefas.
pub fn fetch() -> Result<Vec<TaskItem>, String> {
    parse_tasks(&run(&["list"])?)
}

/// Marca a tarefa como concluída.
pub fn complete(id: &str) -> Result<(), String> {
    run(&["complete", id]).map(|_| ())
}

/// Reabre a tarefa (volta a pendente).
pub fn reopen(id: &str) -> Result<(), String> {
    run(&["reopen", id]).map(|_| ())
}

/// Cria uma nova tarefa com o título dado.
pub fn add(title: &str) -> Result<(), String> {
    run(&["add", title]).map(|_| ())
}

/// Edita o título de uma tarefa.
pub fn edit(id: &str, title: &str) -> Result<(), String> {
    run(&["edit", id, title]).map(|_| ())
}

/// Apaga a tarefa.
pub fn delete(id: &str) -> Result<(), String> {
    run(&["delete", id]).map(|_| ())
}

/// Roda `gtasks <args...>` e devolve o stdout (ou um erro com o stderr).
fn run(args: &[&str]) -> Result<String, String> {
    let output = super::helper_command("gtasks")
        .args(args)
        .output()
        .map_err(|e| format!("falha ao executar gtasks: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gtasks falhou: {}", err.lines().last().unwrap_or("")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_into_tasks() {
        let raw = r#"[
            {"id":"a1","title":"Comprar café","completed":false,"due":"2026-06-10","notes":""},
            {"id":"b2","title":"Feito","completed":true,"due":"","notes":"obs"}
        ]"#;
        let tasks = parse_tasks(raw).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, "a1");
        assert_eq!(tasks[0].title, "Comprar café");
        assert!(!tasks[0].completed);
        assert_eq!(tasks[0].due, "2026-06-10");
        assert!(tasks[1].completed);
        assert_eq!(tasks[1].notes, "obs");
    }

    #[test]
    fn empty_array_yields_no_tasks() {
        assert_eq!(parse_tasks("[]").unwrap().len(), 0);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_tasks("nope").is_err());
    }
}
