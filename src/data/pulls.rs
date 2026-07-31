//! Busca de PRs/issues pendentes via `ghpending` (saída colorida em ANSI).

use std::process::Command;

/// Quebra a saída crua do `ghpending` em linhas, **preservando** os escapes
/// ANSI (as cores são reaplicadas na renderização via `crate::ansi`).
/// Descarta apenas as linhas em branco nas pontas.
pub fn parse_pulls(raw: &str) -> Vec<String> {
    let lines: Vec<String> = raw.lines().map(|l| l.trim_end().to_string()).collect();

    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].to_vec(),
        _ => Vec::new(),
    }
}

/// Roda o `ghpending` e devolve o digest linha a linha (com ANSI).
pub fn fetch() -> Result<Vec<String>, String> {
    let output = Command::new("ghpending")
        .output()
        .map_err(|e| format!("falha ao executar ghpending: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ghpending falhou: {}", super::stderr_summary(&err)));
    }

    Ok(parse_pulls(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_ansi_and_inner_lines() {
        let raw = "\n\x1b[36mrepo\x1b[39m\n  PR #12 fix\n\n";
        let lines = parse_pulls(raw);
        assert_eq!(lines, vec!["\x1b[36mrepo\x1b[39m".to_string(), "  PR #12 fix".to_string()]);
    }

    #[test]
    fn trims_only_blank_edges() {
        let raw = "\n\na\n\nb\n\n";
        assert_eq!(parse_pulls(raw), vec!["a".to_string(), "".to_string(), "b".to_string()]);
    }

    #[test]
    fn empty_output_yields_no_lines() {
        assert_eq!(parse_pulls("").len(), 0);
        assert_eq!(parse_pulls("\n\n").len(), 0);
    }
}
