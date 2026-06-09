//! Busca de PRs/issues pendentes via `ghpending` (que só emite texto colorido).

use std::process::Command;

/// Remove sequências de escape ANSI (cores) de uma string.
///
/// Trata o padrão CSI usado pelo `ghpending`: `ESC [ ... <letra final>`.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Pula um '[' opcional e tudo até a letra final do CSI.
            if chars.peek() == Some(&'[') {
                chars.next();
                for inner in chars.by_ref() {
                    if inner.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }

    out
}

/// Converte a saída crua do `ghpending` em linhas limpas (sem ANSI),
/// preservando a estrutura do digest e descartando linhas vazias nas pontas.
pub fn parse_pulls(raw: &str) -> Vec<String> {
    let stripped = strip_ansi(raw);
    let lines: Vec<String> = stripped
        .lines()
        .map(|l| l.trim_end().to_string())
        .collect();

    // Remove linhas em branco no começo e no fim, mantendo as do meio.
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].to_vec(),
        _ => Vec::new(),
    }
}

/// Roda o `ghpending` e devolve o digest já limpo, linha a linha.
pub fn fetch() -> Result<Vec<String>, String> {
    let output = Command::new("ghpending")
        .output()
        .map_err(|e| format!("falha ao executar ghpending: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ghpending falhou: {}", err.lines().last().unwrap_or("")));
    }

    Ok(parse_pulls(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_color_codes() {
        let colored = "\x1b[36m\x1b[1mrepo/name\x1b[39m\x1b[0m";
        assert_eq!(strip_ansi(colored), "repo/name");
    }

    #[test]
    fn keeps_plain_text_intact() {
        assert_eq!(strip_ansi("nada para mudar"), "nada para mudar");
    }

    #[test]
    fn parse_trims_blank_edges_but_keeps_inner_lines() {
        let raw = "\n\x1b[36mrepo\x1b[39m\n  PR #12 fix\n\n";
        let lines = parse_pulls(raw);
        assert_eq!(lines, vec!["repo".to_string(), "  PR #12 fix".to_string()]);
    }

    #[test]
    fn empty_output_yields_no_lines() {
        assert_eq!(parse_pulls("").len(), 0);
        assert_eq!(parse_pulls("\n\n").len(), 0);
    }
}
