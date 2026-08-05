//! Central de notificações: coisas que pedem sua atenção, de várias fontes.
//!
//! Hoje só o Jira alimenta a lista (menções a você). O tipo foi desenhado para
//! crescer sem mexer no overlay: cada fonte só precisa saber converter os seus
//! itens em `Notification`. Candidatos já mapeados: convites de agenda para
//! aceitar e menções no GitHub.

use super::jira::JiraItem;

/// De onde a notificação veio. O marcador aparece em cada linha do overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Jira,
}

impl Source {
    /// Marcador curto, no idioma dos `[W]`/`[P]` dos outros painéis.
    pub const fn marker(self) -> &'static str {
        match self {
            Source::Jira => "[JIRA]",
        }
    }
}

/// Uma linha da central de notificações.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// Identidade estável, usada para guardar no banco que você já leu esta.
    /// Prefixada pela fonte para duas fontes nunca colidirem.
    pub id: String,
    pub source: Source,
    /// O que aconteceu, em uma linha.
    pub title: String,
    /// Contexto secundário (chave, status, remetente…), esmaecido na tela.
    pub context: String,
    /// Aberto no navegador com `Enter`; vazio quando não há para onde ir.
    pub url: String,
}

/// Converte menções do Jira em notificações.
pub fn from_jira_mentions(items: &[JiraItem]) -> Vec<Notification> {
    items
        .iter()
        .map(|i| Notification {
            // A issue é a identidade: marcar como lida dispensa esta menção
            // para sempre, que é o que se espera de uma central que se limpa.
            id: format!("jira:{}", i.key),
            source: Source::Jira,
            title: i.summary.clone(),
            context: format!("{} · {}", i.key, i.status),
            url: i.url.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::jira::parse_issues;

    // Forma da saída real de `jira mentions`, com conteúdo inventado.
    const MENTIONS: &str = r#"[
      {"key":"ENG-101","summary":"Revisar o plano de capacidade","status":"Em andamento",
       "project":"ENG","url":"https://example.atlassian.net/browse/ENG-101","parent":null,"role":"reporter"}
    ]"#;

    #[test]
    fn a_jira_mention_becomes_a_notification_with_key_and_status() {
        let items = parse_issues(MENTIONS).unwrap();
        let notes = from_jira_mentions(&items);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, "jira:ENG-101");
        assert_eq!(notes[0].source, Source::Jira);
        assert_eq!(notes[0].title, "Revisar o plano de capacidade");
        assert_eq!(notes[0].context, "ENG-101 · Em andamento");
        assert_eq!(notes[0].url, "https://example.atlassian.net/browse/ENG-101");
        assert_eq!(notes[0].source.marker(), "[JIRA]");
    }

    #[test]
    fn no_mentions_yields_no_notifications() {
        assert!(from_jira_mentions(&[]).is_empty());
    }
}
