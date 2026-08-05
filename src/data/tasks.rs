//! Tarefas do Microsoft To Do (conta pessoal) via a CLI `mstodo`.
//!
//! Leitura: `mstodo list` devolve JSON; escrita: `add`/`complete`/`reopen`/
//! `edit`/`delete`. O painel é interativo, então diferente de PRs/Jira aqui os
//! itens são estruturados (precisamos do `id` para agir na tarefa selecionada).

use std::collections::HashSet;

use chrono::NaiveDate;
use serde::{Deserialize, Deserializer};

/// Uma subtarefa: `checklistItem` no Graph, "etapa" na interface do To Do.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct SubTask {
    pub id: String,
    pub title: String,
    pub completed: bool,
}

/// Nível de prioridade — o `importance` do Graph.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl Priority {
    /// Valor que o `mstodo edit --priority` espera.
    pub const fn flag(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Normal => "normal",
            Priority::High => "high",
        }
    }

    /// Nome no formulário de edição.
    ///
    /// O Graph só tem três níveis (`low`/`normal`/`high`); "média" é o nome do
    /// do meio aqui, para casar com a escala de `!` da lista.
    pub const fn label(self) -> &'static str {
        match self {
            Priority::Low => "baixa",
            Priority::Normal => "média",
            Priority::High => "alta",
        }
    }

    /// Marcador na lista, em escala: `!!!` alta, `!` média, nada para baixa.
    pub const fn marker(self) -> &'static str {
        match self {
            Priority::Low => "",
            Priority::Normal => "!",
            Priority::High => "!!!",
        }
    }

    /// Próximo valor no ciclo do formulário.
    pub const fn next(self) -> Self {
        match self {
            Priority::Low => Priority::Normal,
            Priority::Normal => Priority::High,
            Priority::High => Priority::Low,
        }
    }
}

/// Repetição da tarefa.
///
/// O Graph aceita padrões que este painel não oferece (`relativeMonthly`,
/// `yearly`…) e uma tarefa criada no app do To Do pode usá-los — por isso existe
/// `Other`: ler o que não sabemos escrever é melhor do que falhar o parse da
/// lista inteira.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Recur {
    #[default]
    None,
    Daily,
    Weekly,
    Monthly,
    Other,
}

impl Recur {
    /// Converte o `recurrence.pattern.type` do Graph.
    pub fn from_graph(raw: &str) -> Self {
        match raw {
            "" => Recur::None,
            "daily" => Recur::Daily,
            "weekly" => Recur::Weekly,
            "absoluteMonthly" => Recur::Monthly,
            _ => Recur::Other,
        }
    }

    /// Valor que o `mstodo edit --recur` espera.
    ///
    /// `Other` não é escrito: o ciclo do formulário sai dele para `None`, e é
    /// isso que "tirar a repetição que eu não sei editar" quer dizer.
    pub const fn flag(self) -> &'static str {
        match self {
            Recur::None | Recur::Other => "none",
            Recur::Daily => "daily",
            Recur::Weekly => "weekly",
            Recur::Monthly => "monthly",
        }
    }

    /// Nome no formulário de edição.
    pub const fn label(self) -> &'static str {
        match self {
            Recur::None => "nenhuma",
            Recur::Daily => "diária",
            Recur::Weekly => "semanal",
            Recur::Monthly => "mensal",
            Recur::Other => "outra (do app)",
        }
    }

    /// Marcador na lista; nada quando não repete.
    pub const fn marker(self) -> &'static str {
        match self {
            Recur::None => "",
            _ => " ↻",
        }
    }

    /// Próximo valor no ciclo do formulário.
    pub const fn next(self) -> Self {
        match self {
            Recur::None => Recur::Daily,
            Recur::Daily => Recur::Weekly,
            Recur::Weekly => Recur::Monthly,
            Recur::Monthly | Recur::Other => Recur::None,
        }
    }
}

impl<'de> Deserialize<'de> for Recur {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        Ok(Recur::from_graph(&String::deserialize(de)?))
    }
}

/// Faixa de prazo em que a tarefa cai — é o que dá a ordem de prioridade da
/// lista: o que passou da data primeiro, o sem data por último.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskGroup {
    Overdue,
    Today,
    Week,
    Month,
    Later,
    NoDate,
}

/// Faixas na ordem em que aparecem no painel.
pub const GROUPS: [TaskGroup; 6] = [
    TaskGroup::Overdue,
    TaskGroup::Today,
    TaskGroup::Week,
    TaskGroup::Month,
    TaskGroup::Later,
    TaskGroup::NoDate,
];

impl TaskGroup {
    /// Cabeçalho exibido no painel.
    pub const fn label(self) -> &'static str {
        match self {
            TaskGroup::Overdue => "ATRASADAS",
            TaskGroup::Today => "HOJE",
            TaskGroup::Week => "ESTA SEMANA",
            TaskGroup::Month => "ESTE MÊS",
            TaskGroup::Later => "DEPOIS",
            TaskGroup::NoDate => "SEM DATA",
        }
    }
}

/// Em que faixa a data `due` (`AAAA-MM-DD`, vazio = sem data) cai hoje.
///
/// As janelas são móveis — 7 e 30 dias a partir de hoje — e não o calendário:
/// numa sexta-feira, "esta semana" no sentido de calendário mostraria dois dias
/// e jogaria o resto para "este mês", que é o contrário de ajudar a priorizar.
pub fn group_of(due: &str, today: NaiveDate) -> TaskGroup {
    let Ok(date) = NaiveDate::parse_from_str(due, "%Y-%m-%d") else {
        return TaskGroup::NoDate;
    };
    match (date - today).num_days() {
        d if d < 0 => TaskGroup::Overdue,
        0 => TaskGroup::Today,
        1..=7 => TaskGroup::Week,
        8..=30 => TaskGroup::Month,
        _ => TaskGroup::Later,
    }
}

/// Interpreta o que foi digitado no campo de vencimento.
///
/// Aceita `AAAA-MM-DD`, `hoje`, `amanhã` e `+Nd`; vazio limpa a data. Digitar a
/// data inteira é o caso raro — o comum é "hoje" ou "daqui a três dias".
pub fn parse_due(input: &str, today: NaiveDate) -> Result<Option<NaiveDate>, String> {
    let raw = input.trim().to_lowercase();
    if raw.is_empty() {
        return Ok(None);
    }
    if raw == "hoje" {
        return Ok(Some(today));
    }
    if raw == "amanhã" || raw == "amanha" {
        return Ok(Some(today + chrono::Duration::days(1)));
    }
    if let Some(n) = raw.strip_prefix('+').and_then(|r| r.strip_suffix('d')) {
        let days: i64 = n
            .parse()
            .map_err(|_| format!("não entendi “{input}” — use +3d"))?;
        return Ok(Some(today + chrono::Duration::days(days)));
    }
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| format!("data inválida: “{input}” — use AAAA-MM-DD, hoje, amanhã ou +3d"))
}

/// Uma linha renderizada do painel: cabeçalho de faixa, tarefa ou subtarefa.
///
/// O cursor indexa linhas, então expandir uma tarefa muda quantas existem — quem
/// expande reancora o cursor na tarefa, não no índice. `Header` é a única linha
/// em que o cursor não para; quem o move pula por cima dela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRow {
    Header(TaskGroup),
    Task(usize),
    Sub { task: usize, sub: usize },
}

impl TaskRow {
    /// `true` para as linhas em que o cursor pode parar.
    pub const fn selectable(&self) -> bool {
        !matches!(self, TaskRow::Header(_))
    }
}

/// Achata as tarefas em linhas: cabeçalho da faixa de prazo, as tarefas dela, e
/// as subtarefas das que estão expandidas.
///
/// Dentro de cada faixa vale a ordem que o `mstodo list` já devolve (pendentes
/// primeiro, depois por vencimento). Faixa sem tarefa não gera cabeçalho.
pub fn rows(items: &[TaskItem], expanded: &HashSet<String>, today: NaiveDate) -> Vec<TaskRow> {
    let mut out = Vec::new();
    for group in GROUPS {
        let mut header_done = false;
        for (t, item) in items.iter().enumerate() {
            if group_of(&item.due, today) != group {
                continue;
            }
            if !header_done {
                out.push(TaskRow::Header(group));
                header_done = true;
            }
            out.push(TaskRow::Task(t));
            if expanded.contains(&item.id) {
                out.extend((0..item.subtasks.len()).map(|sub| TaskRow::Sub { task: t, sub }));
            }
        }
    }
    out
}

/// Uma tarefa do Microsoft To Do.
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
    /// Etapas da tarefa; `[]` quando não há. Vêm embutidas no `mstodo list`.
    #[serde(default)]
    pub subtasks: Vec<SubTask>,
    /// Prioridade (`importance` do Graph).
    #[serde(default)]
    pub priority: Priority,
    /// Repetição, quando a tarefa tem.
    #[serde(default)]
    pub recur: Recur,
}

/// O que mudar numa tarefa. Campo em `None` fica como está.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TaskEdit {
    pub title: Option<String>,
    /// Já no formato do helper: `AAAA-MM-DD`, ou `none` para limpar.
    pub due: Option<String>,
    pub recur: Option<Recur>,
    pub priority: Option<Priority>,
}

impl TaskEdit {
    /// `true` quando não há nada para gravar.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.due.is_none()
            && self.recur.is_none()
            && self.priority.is_none()
    }
}

/// Grava as mudanças de uma tarefa numa chamada só.
///
/// Uma só porque o Graph exige a data no mesmo pedido em que a recorrência muda
/// — e porque é uma re-busca em vez de quatro.
pub fn update(id: &str, edit: &TaskEdit) -> Result<(), String> {
    if edit.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = vec!["edit", id];
    if let Some(title) = &edit.title {
        args.extend(["--title", title]);
    }
    if let Some(due) = &edit.due {
        args.extend(["--due", due]);
    }
    if let Some(recur) = edit.recur {
        args.extend(["--recur", recur.flag()]);
    }
    if let Some(priority) = edit.priority {
        args.extend(["--priority", priority.flag()]);
    }
    run(&args).map(|_| ())
}

/// Parseia o JSON do `mstodo list` numa lista de tarefas.
pub fn parse_tasks(raw: &str) -> Result<Vec<TaskItem>, String> {
    serde_json::from_str(raw).map_err(|e| format!("JSON inválido do mstodo: {e}"))
}

/// Roda `mstodo list` e devolve as tarefas.
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

/// Cria uma subtarefa na tarefa dada.
pub fn add_subtask(task_id: &str, title: &str) -> Result<(), String> {
    run(&["subtask", task_id, title]).map(|_| ())
}

/// Renomeia uma subtarefa.
pub fn edit_subtask(task_id: &str, item_id: &str, title: &str) -> Result<(), String> {
    run(&["subtask-edit", task_id, item_id, title]).map(|_| ())
}

/// Apaga uma subtarefa.
pub fn delete_subtask(task_id: &str, item_id: &str) -> Result<(), String> {
    run(&["subtask-delete", task_id, item_id]).map(|_| ())
}

/// Marca uma subtarefa como concluída.
pub fn check(task_id: &str, item_id: &str) -> Result<(), String> {
    run(&["check", task_id, item_id]).map(|_| ())
}

/// Desmarca uma subtarefa.
pub fn uncheck(task_id: &str, item_id: &str) -> Result<(), String> {
    run(&["uncheck", task_id, item_id]).map(|_| ())
}

/// Apaga a tarefa.
pub fn delete(id: &str) -> Result<(), String> {
    run(&["delete", id]).map(|_| ())
}

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

    // Forma da saída real de `mstodo list` com subtarefas; conteúdo inventado,
    // porque o repo é público e a lista é pessoal.
    const WITH_SUBS: &str = r#"[
      {"id":"T1","title":"Trocar a instalação elétrica","completed":false,"due":"","notes":"",
       "subtasks":[{"id":"S1","title":"Medir a fiação","completed":true},
                   {"id":"S2","title":"Comprar disjuntor","completed":false}]},
      {"id":"T2","title":"Sem etapas","completed":false,"due":"","notes":"","subtasks":[]}
    ]"#;

    #[test]
    fn parses_subtasks_keeping_their_state() {
        let tasks = parse_tasks(WITH_SUBS).unwrap();
        assert_eq!(tasks[0].subtasks.len(), 2);
        assert_eq!(tasks[0].subtasks[0].id, "S1");
        assert!(tasks[0].subtasks[0].completed);
        assert_eq!(tasks[0].subtasks[1].title, "Comprar disjuntor");
        assert!(tasks[1].subtasks.is_empty());
    }

    #[test]
    fn missing_subtasks_field_defaults_to_empty() {
        // Um `mstodo` anterior não emitia o campo; o painel não deve quebrar.
        let tasks = parse_tasks(r#"[{"id":"a","title":"t","completed":false}]"#).unwrap();
        assert!(tasks[0].subtasks.is_empty());
    }

    /// Data fixa para os testes de faixa: nada aqui depende do dia real.
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 4).unwrap()
    }

    #[test]
    fn rows_are_one_per_task_under_the_group_header() {
        let tasks = parse_tasks(WITH_SUBS).unwrap();
        assert_eq!(
            rows(&tasks, &HashSet::new(), today()),
            vec![
                TaskRow::Header(TaskGroup::NoDate),
                TaskRow::Task(0),
                TaskRow::Task(1)
            ],
            "as duas não têm data, então uma faixa só"
        );
    }

    #[test]
    fn each_deadline_window_gets_its_own_header_in_order() {
        let raw = r#"[
          {"id":"a","title":"atrasada","completed":false,"due":"2026-08-01"},
          {"id":"b","title":"hoje","completed":false,"due":"2026-08-04"},
          {"id":"c","title":"semana","completed":false,"due":"2026-08-09"},
          {"id":"d","title":"mes","completed":false,"due":"2026-08-25"},
          {"id":"e","title":"depois","completed":false,"due":"2026-11-15"},
          {"id":"f","title":"sem data","completed":false,"due":""}
        ]"#;
        let tasks = parse_tasks(raw).unwrap();
        let headers: Vec<TaskGroup> = rows(&tasks, &HashSet::new(), today())
            .into_iter()
            .filter_map(|r| match r {
                TaskRow::Header(g) => Some(g),
                _ => None,
            })
            .collect();
        assert_eq!(headers, GROUPS.to_vec(), "uma faixa por prazo, nessa ordem");
    }

    #[test]
    fn a_window_without_tasks_has_no_header() {
        let raw = r#"[{"id":"a","title":"hoje","completed":false,"due":"2026-08-04"}]"#;
        let tasks = parse_tasks(raw).unwrap();
        assert_eq!(
            rows(&tasks, &HashSet::new(), today()),
            vec![TaskRow::Header(TaskGroup::Today), TaskRow::Task(0)]
        );
    }

    #[test]
    fn the_group_of_a_date_follows_rolling_windows() {
        let t = today();
        assert_eq!(group_of("2026-08-03", t), TaskGroup::Overdue);
        assert_eq!(group_of("2026-08-04", t), TaskGroup::Today);
        assert_eq!(group_of("2026-08-11", t), TaskGroup::Week, "sétimo dia ainda é semana");
        assert_eq!(group_of("2026-08-12", t), TaskGroup::Month);
        assert_eq!(group_of("2026-09-03", t), TaskGroup::Month, "trigésimo dia");
        assert_eq!(group_of("2026-09-04", t), TaskGroup::Later);
        assert_eq!(group_of("", t), TaskGroup::NoDate);
        assert_eq!(group_of("nem data", t), TaskGroup::NoDate);
    }

    #[test]
    fn only_headers_are_unselectable() {
        assert!(!TaskRow::Header(TaskGroup::Today).selectable());
        assert!(TaskRow::Task(0).selectable());
        assert!(TaskRow::Sub { task: 0, sub: 0 }.selectable());
    }

    #[test]
    fn expanding_inserts_the_subtasks_right_below_their_task() {
        let tasks = parse_tasks(WITH_SUBS).unwrap();
        let expanded = HashSet::from(["T1".to_string()]);
        assert_eq!(
            rows(&tasks, &expanded, today()),
            vec![
                TaskRow::Header(TaskGroup::NoDate),
                TaskRow::Task(0),
                TaskRow::Sub { task: 0, sub: 0 },
                TaskRow::Sub { task: 0, sub: 1 },
                TaskRow::Task(1),
            ]
        );
    }

    #[test]
    fn expanding_a_task_without_subtasks_adds_no_rows() {
        let tasks = parse_tasks(WITH_SUBS).unwrap();
        let expanded = HashSet::from(["T2".to_string()]);
        assert_eq!(
            rows(&tasks, &expanded, today()).len(),
            3,
            "cabeçalho da faixa mais as duas tarefas"
        );
    }

    #[test]
    fn priority_and_recurrence_come_from_the_helper() {
        let raw = r#"[{"id":"a","title":"t","completed":false,"due":"2026-08-07",
                       "priority":"high","recur":"absoluteMonthly"}]"#;
        let t = &parse_tasks(raw).unwrap()[0];
        assert_eq!(t.priority, Priority::High);
        assert_eq!(t.recur, Recur::Monthly);
        assert_eq!(t.priority.marker(), "!!!");
    }

    #[test]
    fn a_recurrence_this_panel_cannot_write_still_parses() {
        // Uma tarefa criada no app do To Do pode repetir de formas que o
        // formulário não oferece; falhar o parse tiraria a lista inteira do ar.
        let raw = r#"[{"id":"a","title":"t","completed":false,"recur":"relativeMonthly"}]"#;
        let t = &parse_tasks(raw).unwrap()[0];
        assert_eq!(t.recur, Recur::Other);
        assert_eq!(t.recur.label(), "outra (do app)");
        assert_eq!(t.recur.next(), Recur::None, "o ciclo sai dela para nenhuma");
    }

    #[test]
    fn missing_priority_and_recurrence_default_to_the_quiet_values() {
        let t = &parse_tasks(r#"[{"id":"a","title":"t","completed":false}]"#).unwrap()[0];
        assert_eq!(t.priority, Priority::Normal);
        assert_eq!(t.recur, Recur::None);
        assert_eq!(t.recur.marker(), "", "sem repetição, sem marcador");
    }

    #[test]
    fn the_priority_marker_is_a_scale_of_exclamation_marks() {
        assert_eq!(Priority::High.marker(), "!!!");
        assert_eq!(Priority::Normal.marker(), "!");
        assert_eq!(Priority::Low.marker(), "", "baixa não pede atenção");
    }

    #[test]
    fn the_due_field_accepts_shortcuts_and_iso() {
        let t = today();
        assert_eq!(parse_due("", t), Ok(None), "vazio limpa a data");
        assert_eq!(parse_due("hoje", t), Ok(Some(t)));
        assert_eq!(
            parse_due("amanhã", t),
            Ok(Some(NaiveDate::from_ymd_opt(2026, 8, 5).unwrap()))
        );
        assert_eq!(
            parse_due("amanha", t),
            Ok(Some(NaiveDate::from_ymd_opt(2026, 8, 5).unwrap())),
            "sem acento também"
        );
        assert_eq!(
            parse_due("+3d", t),
            Ok(Some(NaiveDate::from_ymd_opt(2026, 8, 7).unwrap()))
        );
        assert_eq!(
            parse_due("2026-12-31", t),
            Ok(Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()))
        );
    }

    #[test]
    fn a_due_field_that_makes_no_sense_says_what_to_type() {
        let err = parse_due("semana que vem", today()).unwrap_err();
        assert!(err.contains("AAAA-MM-DD"), "a mensagem ensina o formato: {err}");
        assert!(parse_due("+xd", today()).is_err());
    }

    #[test]
    fn an_edit_with_nothing_to_change_writes_nothing() {
        // Sem isto, confirmar o formulário sem mexer em nada chamaria o helper
        // só para receber "nada para mudar".
        assert!(TaskEdit::default().is_empty());
        assert_eq!(update("qualquer-id", &TaskEdit::default()), Ok(()));
    }

    #[test]
    fn empty_array_yields_no_tasks() {
        assert_eq!(parse_tasks("[]").unwrap().len(), 0);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_tasks("nope").is_err());
    }

    /// O `mstodo` promete UMA linha no stderr para qualquer falha — o painel só
    /// mostra o resumo (`stderr_summary`), e traceback não ajuda. Aqui o helper
    /// roda de verdade com um diretório no lugar do cache de token: o
    /// `read_text` estoura antes de qualquer chamada de rede (`PermissionError`
    /// no Windows, `IsADirectoryError` no Unix), então o teste é offline.
    ///
    /// Precisa do `uv` no PATH (é como o helper é executado); sem ele, o teste
    /// não tem o que exercitar e passa.
    #[test]
    fn mstodo_reports_an_unexpected_failure_as_a_single_line() {
        use std::process::Command;
        if Command::new("uv").arg("--version").output().is_err() {
            eprintln!("uv ausente — contrato de uma linha do mstodo não exercitado");
            return;
        }
        let root = env!("CARGO_MANIFEST_DIR");
        let out = Command::new("uv")
            .args(["run", "--script", "scripts/mstodo", "list"])
            .current_dir(root)
            .env("MSTODO_TOKEN", std::env::temp_dir())
            .env("DAILY_TUI_TODO_CLIENT_ID", "test-client-id")
            .output()
            .expect("uv run --script scripts/mstodo");

        assert!(!out.status.success(), "o helper deveria falhar");
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(err.lines().count(), 1, "stderr não é uma linha só:\n{err}");
        assert!(!err.contains("Traceback"), "stderr traz traceback:\n{err}");
        // E o painel mostra a exceção, não o cabeçalho do traceback.
        assert!(
            super::super::stderr_summary(&err).contains("Error: "),
            "resumo sem a exceção:\n{err}"
        );
    }

    // Item real de `mstodo list` (Microsoft Graph): id longo, due/notes vazios.
    #[test]
    fn parses_real_mstodo_output() {
        let raw = r#"[{"id":"AQMkADAwATNiZmYAZC1jYTc2LTZlYmMtMDACLTAwCgBGAAADHELHu7jbAUiyqKO5om2DIgcASi137h_gPUWbK6clfD9RhAAAAgESAAAASi137h_gPUWbK6clfD9RhAAJOL8XaQAAAA==","title":"Criar DOC automatizado com PR","completed":false,"due":"","notes":""}]"#;
        let tasks = parse_tasks(raw).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Criar DOC automatizado com PR");
        assert!(!tasks[0].completed);
        assert_eq!(tasks[0].due, "");
        assert_eq!(tasks[0].notes, "");
    }
}
