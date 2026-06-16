//! Busca dos tickets do Jira atribuídos a mim e abertos, via `jirapending`
//! (saída colorida em ANSI, agrupada por projeto).

use std::process::Command;

/// Quebra a saída crua do `jirapending` em linhas, **preservando** os escapes
/// ANSI (as cores são reaplicadas na renderização via `crate::ansi`).
/// Descarta apenas as linhas em branco nas pontas.
pub fn parse_jira(raw: &str) -> Vec<String> {
    let lines: Vec<String> = raw.lines().map(|l| l.trim_end().to_string()).collect();

    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].to_vec(),
        _ => Vec::new(),
    }
}

/// Roda o `jirapending` e devolve as linhas (com ANSI).
pub fn fetch() -> Result<Vec<String>, String> {
    let output = Command::new("jirapending")
        .output()
        .map_err(|e| format!("falha ao executar jirapending: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("jirapending falhou: {}", err.lines().last().unwrap_or("")));
    }

    Ok(parse_jira(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_ansi_and_inner_lines() {
        let raw = "\n\x1b[36;1mGOV (1)\x1b[0m\n  GOV-1 fix\n\n";
        let lines = parse_jira(raw);
        assert_eq!(lines, vec!["\x1b[36;1mGOV (1)\x1b[0m".to_string(), "  GOV-1 fix".to_string()]);
    }

    #[test]
    fn trims_only_blank_edges() {
        let raw = "\n\na\n\nb\n\n";
        assert_eq!(parse_jira(raw), vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn empty_output_yields_no_lines() {
        assert_eq!(parse_jira("").len(), 0);
        assert_eq!(parse_jira("\n\n").len(), 0);
    }
}
