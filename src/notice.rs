//! Canal de notificação do painel. Genérico de propósito: um `Notice` tem
//! título e corpo, e nada aqui sabe o que é um pomodoro — é o que permite que
//! qualquer painel avise depois (e-mail novo, PR aprovado, menção no Jira).

use std::process::Command;

/// O que se manda. Título e corpo — os dois campos que todo canal entende.
///
/// Prioridade, ícone e tags existem no ntfy, mas nenhum chamador precisa deles
/// hoje; quem precisar acrescenta o campo com o caso de uso na mão.
#[derive(Debug, Clone)]
pub struct Notice {
    pub title: String,
    pub body: String,
}

impl Notice {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
        }
    }
}

/// Manda pelo primeiro canal que funcionar: notificação do sistema e, se ela
/// falhar e houver tópico no config, ntfy.sh.
///
/// Erro só quando nenhum canal funciona — e ele nomeia os dois, porque "não
/// avisou" sem motivo não dá para agir.
pub fn send(n: &Notice) -> Result<(), String> {
    let native = match os(n) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let topic = &crate::config::get().notify.ntfy_topic;
    if topic.is_empty() {
        return Err(native);
    }
    ntfy(n, topic).map_err(|e| format!("{native}; ntfy: {e}"))
}

/// Notificação nativa. A mesma chamada serve Windows, macOS e Linux — quem
/// resolve a diferença é o `notify-rust`.
fn os(n: &Notice) -> Result<(), String> {
    notify_rust::Notification::new()
        .summary(&n.title)
        .body(&n.body)
        .show()
        .map(|_| ())
        .map_err(|e| format!("sistema: {e}"))
}

/// Argumentos do `curl` para o ntfy. Separado do envio para ser testável — o
/// envio em si depende de rede e não entra em teste.
pub fn ntfy_args(n: &Notice, topic: &str) -> Vec<String> {
    vec![
        // `-f`: HTTP 4xx/5xx vira código de saída != 0. `-sS`: sem barra de
        // progresso, mas com a mensagem de erro no stderr.
        "-f".into(),
        "-sS".into(),
        "-H".into(),
        format!("Title: {}", n.title),
        "-d".into(),
        n.body.clone(),
        format!("https://ntfy.sh/{topic}"),
    ]
}

/// Manda pelo `curl`. Sem shell no meio: o corpo e o título viajam como
/// argumentos, então acento, aspas e quebra de linha não precisam de escape.
fn ntfy(n: &Notice, topic: &str) -> Result<(), String> {
    let out = Command::new("curl")
        .args(ntfy_args(n, topic))
        .output()
        .map_err(|e| format!("curl não rodou: {e}"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() {
            format!("curl saiu com {}", out.status)
        } else {
            why
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curl_arguments_carry_the_title_the_body_and_the_topic() {
        let n = Notice::new("Pomodoro: hora da pausa", "25 min fechados.");
        let args = ntfy_args(&n, "meutopico");

        // `-f` é o que transforma um 404 do ntfy em código de saída != 0: sem
        // ele, o curl sai zero e um tópico errado passaria por envio bem-feito.
        assert!(args.iter().any(|a| a == "-f"), "{args:?}");
        assert!(
            args.contains(&"Title: Pomodoro: hora da pausa".to_string()),
            "{args:?}"
        );
        assert!(args.contains(&"25 min fechados.".to_string()), "{args:?}");
        assert_eq!(args.last().unwrap(), "https://ntfy.sh/meutopico");
    }

    #[test]
    fn accents_travel_untouched_because_the_body_is_an_argument_not_a_shell_line() {
        let n = Notice::new("Pausa", "Descanso de 5 min começou. Até já!");
        let args = ntfy_args(&n, "t");
        assert!(
            args.contains(&"Descanso de 5 min começou. Até já!".to_string()),
            "{args:?}"
        );
    }
}
