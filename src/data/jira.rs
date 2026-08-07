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
    /// Por que a issue está no resultado (assignee/reporter/both).
    #[serde(default)]
    pub role: JiraRole,
    /// Nome do tipo como o Jira o chama (`História`, `Epic`, `Iniciativa`…).
    #[serde(default, rename = "type")]
    pub kind: String,
    /// `true` quando o Jira classifica o tipo como subtarefa. Vem de um campo
    /// próprio da API, e não do nome — cada instância batiza o tipo como quer.
    #[serde(default)]
    pub subtask: bool,
}

impl JiraItem {
    /// Marcador de tipo desta issue.
    ///
    /// Subtarefa tem o seu (`[s]`, minúsculo ao lado do `[S]` de história):
    /// o nome do tipo varia por instância, mas a API diz quem é subtarefa.
    pub fn type_marker(&self) -> &'static str {
        if self.subtask {
            "[s]"
        } else {
            type_marker(&self.kind)
        }
    }
}

/// Marcador de uma letra para o tipo da issue.
///
/// O nome vem no idioma da instância — a do autor responde `História` e
/// `Iniciativa`, não `Story`/`Initiative` —, então os dois idiomas entram no
/// mapa. Tipo fora do mapa vira `[?]`, e não uma letra tirada da inicial:
/// `[System] Service request` começa com `[`, e chutar `S` por causa de
/// "Service" o deixaria idêntico a história.
pub fn type_marker(kind: &str) -> &'static str {
    let k = kind.trim().to_lowercase();
    match k.as_str() {
        "história" | "historia" | "story" => "[S]",
        "epic" | "épico" | "epico" => "[E]",
        "iniciativa" | "initiative" => "[I]",
        "objetivo" | "objective" => "[O]",
        // Requisição: no Jira do autor é o tipo do projeto de Pedido de Serviço
        // (PdS), que a API devolve com o nome do template do Service Management.
        "[system] service request"
        | "service request"
        | "solicitação de serviço"
        | "solicitacao de servico"
        | "pedido de serviço"
        | "pedido de servico"
        | "requisição"
        | "requisicao" => "[R]",
        _ => "[?]",
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct JiraParent {
    pub key: String,
    pub summary: String,
}

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

/// Modo de filtro do painel; circulado pela tecla `f`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JiraFilter {
    Assignee,
    Reporter,
    #[default]
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
    for i in project_order(items) {
        let item = &items[i];
        if current != Some(item.project.as_str()) {
            rows.push(JiraRow::Header(item.project.clone()));
            current = Some(item.project.as_str());
        }
        rows.push(JiraRow::Issue(i));
    }
    rows
}

/// Ordem das issues no agrupamento por projeto: a do helper, com cada subtarefa
/// puxada para logo depois do pai.
///
/// Indentar uma subtarefa longe do pai é pior do que não indentar: a linha fica
/// deslocada sob quem não é dela. Subtarefa cujo pai não está na lista fica onde
/// estava, sem indentação (ver `is_nested`).
fn project_order(items: &[JiraItem]) -> Vec<usize> {
    /// Filhas diretas de `parent`, na ordem em que o helper as devolveu.
    fn children_of(items: &[JiraItem], parent: &str) -> Vec<usize> {
        items
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                i.subtask && i.parent.as_ref().map(|p| p.key.as_str()) == Some(parent)
            })
            .map(|(k, _)| k)
            .collect()
    }

    let mut out = Vec::with_capacity(items.len());
    let mut done = vec![false; items.len()];
    for (i, item) in items.iter().enumerate() {
        // Subtarefa com o pai na lista não entra aqui: ela é emitida logo depois
        // dele, quando chegar a vez do pai.
        if done[i] || is_nested(items, i) {
            continue;
        }
        out.push(i);
        done[i] = true;
        for k in children_of(items, &item.key) {
            if !done[k] {
                out.push(k);
                done[k] = true;
            }
        }
    }
    // Rede de segurança: subtarefa cujo pai existe na lista mas não foi emitido
    // (subtarefa de subtarefa, dado circular) não pode desaparecer da tela.
    out.extend(
        done.iter()
            .enumerate()
            .filter(|(_, emitted)| !**emitted)
            .map(|(i, _)| i),
    );
    out
}

/// `true` quando a linha desta issue deve ser indentada: é subtarefa **e** o pai
/// está na lista, logo acima dela.
pub fn is_nested(items: &[JiraItem], index: usize) -> bool {
    let item = &items[index];
    if !item.subtask {
        return false;
    }
    let Some(parent) = item.parent.as_ref() else {
        return false;
    };
    items.iter().any(|i| i.key == parent.key)
}

/// Índice da linha que mostra a issue `item`; 0 quando não estiver nas linhas.
pub fn row_of_item(rows: &[JiraRow], item: usize) -> usize {
    rows.iter()
        .position(|r| matches!(r, JiraRow::Issue(i) if *i == item))
        .unwrap_or(0)
}

/// Visão ativa do painel de Jira.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JiraView {
    #[default]
    Issues,
    ByParent,
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

/// Roda `jira issues --filter <modo>` e devolve as issues.
pub fn fetch(filter: JiraFilter) -> Result<Vec<JiraItem>, String> {
    parse_issues(&run(&["issues", "--filter", filter.flag()])?)
}

/// Roda `jira mentions` e devolve as issues onde fui mencionado.
pub fn fetch_mentions() -> Result<Vec<JiraItem>, String> {
    parse_issues(&run(&["mentions"])?)
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


#[cfg(test)]
mod tests {
    use super::*;

    // Mesma forma da saída real de `jira issues --filter both`, com conteúdo
    // inventado: chaves, domínio e resumos aqui não são reais — o repositório
    // é público, e a saída real carrega tickets internos da empresa (alguns
    // sobre segurança). Preservei, porém, o que faz o contrato valer a pena
    // testar: uma issue com pai e duas sem no mesmo projeto (para exercitar
    // `rows_by_project`/`row_of_item`), a inconsistência real de capitalização
    // do status ("Em andamento" vs. "Em Andamento") e um resumo acentuado
    // (para exercitar UTF-8).
    const REAL: &str = r#"[
      {"key":"ENG-101","summary":"[Painel] - Melhorias no dashboard de métricas","status":"Em andamento",
       "project":"ENG","url":"https://example.atlassian.net/browse/ENG-101","type":"História",
       "parent":{"key":"ENG-1","summary":"Iniciativa de Engenharia"}},
      {"key":"OPS-55","summary":"Revisão de configuração de acesso","status":"Em Andamento",
       "project":"OPS","url":"https://example.atlassian.net/browse/OPS-55","parent":null},
      {"key":"OPS-56","summary":"Atualização de rotina de backup","status":"Em Andamento",
       "project":"OPS","url":"https://example.atlassian.net/browse/OPS-56","parent":null}
    ]"#;

    #[test]
    fn parses_the_real_contract() {
        let items = parse_issues(REAL).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].key, "ENG-101");
        assert_eq!(items[0].status, "Em andamento");
        assert_eq!(items[0].url, "https://example.atlassian.net/browse/ENG-101");
        assert_eq!(items[0].parent.as_ref().unwrap().key, "ENG-1");
        assert!(items[1].parent.is_none());
    }

    #[test]
    fn null_parent_and_missing_fields_are_tolerated() {
        let items = parse_issues(r#"[{"key":"A-1","summary":"s","status":"","project":"A","url":"u","parent":null}]"#).unwrap();
        assert!(items[0].parent.is_none());
    }

    #[test]
    fn the_type_becomes_a_one_letter_marker_in_either_language() {
        // A instância do autor responde em português; o mapa cobre os dois.
        assert_eq!(type_marker("História"), "[S]");
        assert_eq!(type_marker("Story"), "[S]");
        assert_eq!(type_marker("Epic"), "[E]");
        assert_eq!(type_marker("Épico"), "[E]");
        assert_eq!(type_marker("Iniciativa"), "[I]");
        assert_eq!(type_marker("Initiative"), "[I]");
        assert_eq!(type_marker("Objetivo"), "[O]");
        assert_eq!(type_marker("Objective"), "[O]");
    }

    #[test]
    fn a_service_request_is_marked_as_a_request() {
        // O Jira do autor devolve o nome do template do Service Management para
        // as issues do projeto de Pedido de Serviço (PdS).
        assert_eq!(type_marker("[System] Service request"), "[R]");
        assert_eq!(type_marker("Service request"), "[R]");
        assert_eq!(type_marker("Solicitação de serviço"), "[R]");
        assert_eq!(type_marker("Requisição"), "[R]");
    }

    #[test]
    fn a_type_without_a_letter_says_so_instead_of_guessing() {
        // Chutar pela inicial daria `[B]` para bug e `[T]` para tarefa, mas
        // também `[S]` para subtarefa — igual a história. Melhor admitir.
        assert_eq!(type_marker("Subtarefa"), "[?]");
        assert_eq!(type_marker(""), "[?]");
    }

    #[test]
    fn the_type_is_read_from_the_helper_output() {
        let items = parse_issues(REAL).unwrap();
        assert_eq!(items[0].kind, "História");
        assert_eq!(items[1].kind, "", "ausente no JSON não é erro");
    }


    // Forma do que o helper devolve numa hierarquia de verdade — iniciativa,
    // épico, história e a subtarefa dela — com conteúdo inventado.
    const TREE: &str = r#"[
      {"key":"ENG-1","summary":"Plataforma","status":"Em andamento","project":"ENG",
       "url":"u","type":"Iniciativa","parent":null,"subtask":false,"role":"assignee"},
      {"key":"ENG-9","summary":"Ajustar o import","status":"Em andamento","project":"ENG",
       "url":"u","type":"Subtarefa","subtask":true,"role":"both",
       "parent":{"key":"ENG-7","summary":"Importar planilha"}},
      {"key":"ENG-7","summary":"Importar planilha","status":"Em andamento","project":"ENG",
       "url":"u","type":"História","subtask":false,"role":"reporter",
       "parent":{"key":"ENG-1","summary":"Plataforma"}}
    ]"#;

    #[test]
    fn a_subtask_gets_its_own_marker_instead_of_the_unknown_one() {
        // "Subtarefa" não está no mapa de nomes, e cairia em `[?]`; quem diz que
        // é subtarefa é o campo próprio da API.
        let items = parse_issues(TREE).unwrap();
        assert_eq!(items[1].kind, "Subtarefa");
        assert_eq!(type_marker(&items[1].kind), "[?]", "pelo nome, seria desconhecida");
        assert_eq!(items[1].type_marker(), "[s]", "mas o campo `subtask` resolve");
        assert_eq!(items[2].type_marker(), "[S]", "história segue com o dela");
    }

    #[test]
    fn a_subtask_is_pulled_under_its_parent() {
        // O helper devolve por data de atualização, então a subtarefa vinha
        // antes do pai. Indentar longe do pai é pior do que não indentar.
        let items = parse_issues(TREE).unwrap();
        let keys: Vec<&str> = rows_by_project(&items)
            .into_iter()
            .filter_map(|r| match r {
                JiraRow::Issue(i) => Some(items[i].key.as_str()),
                JiraRow::Header(_) => None,
            })
            .collect();
        assert_eq!(keys, vec!["ENG-1", "ENG-7", "ENG-9"], "a subtarefa segue ENG-7");
        assert_eq!(keys.len(), items.len(), "nada duplicado nem perdido");
    }

    #[test]
    fn a_subtask_of_a_subtask_still_shows_up() {
        // Reordenar não pode custar uma linha: se a cadeia de pais não fecha, a
        // issue entra no fim em vez de desaparecer.
        let raw = r#"[
          {"key":"A-1","summary":"neta","status":"x","project":"A","url":"u",
           "type":"Subtarefa","subtask":true,"parent":{"key":"A-2","summary":"filha"}},
          {"key":"A-2","summary":"filha","status":"x","project":"A","url":"u",
           "type":"Subtarefa","subtask":true,"parent":{"key":"A-3","summary":"pai"}}
        ]"#;
        let items = parse_issues(raw).unwrap();
        let shown: Vec<&str> = rows_by_project(&items)
            .into_iter()
            .filter_map(|r| match r {
                JiraRow::Issue(i) => Some(items[i].key.as_str()),
                JiraRow::Header(_) => None,
            })
            .collect();
        assert_eq!(shown.len(), 2, "as duas aparecem: {shown:?}");
    }

    #[test]
    fn only_a_subtask_whose_parent_is_listed_gets_indented() {
        let items = parse_issues(TREE).unwrap();
        assert!(!is_nested(&items, 0), "iniciativa não indenta");
        assert!(!is_nested(&items, 2), "história com pai na lista também não");
        assert!(is_nested(&items, 1), "subtarefa com o pai à vista, sim");

        // Sozinha, sem o pai na lista, ela não é deslocada sob quem não é dela.
        let orphan = parse_issues(
            r#"[{"key":"ENG-9","summary":"s","status":"x","project":"ENG","url":"u",
                 "type":"Subtarefa","subtask":true,
                 "parent":{"key":"ENG-7","summary":"Importar planilha"}}]"#,
        )
        .unwrap();
        assert!(!is_nested(&orphan, 0));
    }

    #[test]
    fn an_issue_without_the_subtask_field_is_not_one() {
        // JSON de um helper anterior não tem o campo.
        let items = parse_issues(REAL).unwrap();
        assert!(items.iter().all(|i| !i.subtask));
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_issues("nope").is_err());
    }

    #[test]
    fn groups_by_project_with_one_header_each() {
        let items = parse_issues(REAL).unwrap();
        let rows = rows_by_project(&items);
        // ENG vem antes de OPS porque a ordem dos itens é preservada.
        assert!(matches!(&rows[0], JiraRow::Header(h) if h == "ENG"));
        assert!(matches!(rows[1], JiraRow::Issue(0)));
        assert!(matches!(&rows[2], JiraRow::Header(h) if h == "OPS"));
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

    #[test]
    fn parses_the_role_of_each_issue() {
        let raw = r#"[{"key":"ENG-1","summary":"s","status":"Em andamento","project":"ENG",
                       "url":"https://example.atlassian.net/browse/ENG-1","parent":null,"role":"both"},
                      {"key":"OPS-2","summary":"s","status":"Backlog","project":"OPS",
                       "url":"https://example.atlassian.net/browse/OPS-2","parent":null,"role":"reporter"}]"#;
        let items = parse_issues(raw).unwrap();
        assert_eq!(items[0].role, JiraRole::Both);
        assert_eq!(items[1].role, JiraRole::Reporter);
    }

    #[test]
    fn missing_role_defaults_to_assignee() {
        let items = parse_issues(r#"[{"key":"A-1","summary":"s","status":"","project":"A","url":"u","parent":null}]"#).unwrap();
        assert_eq!(items[0].role, JiraRole::Assignee);
    }

    #[test]
    fn groups_by_parent_with_orphans_last() {
        let items = parse_issues(REAL).unwrap();
        let rows = rows_by_parent(&items);
        // O grupo do pai vem primeiro, com chave e resumo no cabeçalho.
        assert!(matches!(&rows[0], JiraRow::Header(h) if h == "ENG-1 Iniciativa de Engenharia"));
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
}
