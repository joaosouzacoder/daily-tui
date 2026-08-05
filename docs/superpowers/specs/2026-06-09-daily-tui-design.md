# daily-tui — Design

Data: 2026-06-09

## Objetivo

Um painel TUI para ficar sempre rodando num monitor, mostrando as
informações do dia a dia do João: e-mails, agendas, PRs/issues pendentes
e um relógio em tempo real.

## Decisões de escopo

- **Interação:** painel passivo com navegação leve (read-only). Rola listas
  e abre detalhe do e-mail; sem escrita/ações.
- **Contas:** agregadas (a do trabalho + a pessoal) com marcador de
  origem `[W]` / `[P]`.
- **Refresh dos dados externos:** a cada 5 minutos (+ refresh manual `r`).
- **Relógio:** atualiza a cada 1s (`HH:MM:SS` + data por extenso em pt-BR).

## Stack

- Rust, `ratatui` 0.30.x
- `ratatui-tea`, `ratatui-bubbletea-theme`, `ratatui-bubbletea-components`
  (crates.io 0.2.0) — estilo Elm/TEA (Model/Msg/Cmd) + tema Charm.
- `chrono` para tempo.
- `crossterm` para terminal/eventos.

## Arquitetura

A doc do `ratatui-tea` deixa claro que o executor embutido é MVP: o
`Cmd::tick` bloqueia (sleep síncrono) e ainda não há execução assíncrona
de processos externos. Para um painel 24/7 com relógio de 1s + CLIs lentas
(`himalaya`/`gcalcli`/`ghpending`), dirigimos **nosso próprio event loop**
(caminho explicitamente abençoado pela doc), reaproveitando o estilo
Model/Msg/Cmd e os crates de tema/componentes para o visual.

- **Main loop** (`main.rs`): setup do terminal; poll de teclado a ~250ms;
  a cada 1s envia `ClockTick`; drena o canal de resultados do worker e
  converte em `Msg`; chama `app.update(msg)` + render; teardown limpo.
- **Worker thread** (`worker.rs`): canal de comandos com
  `recv_timeout(tempo_até_próximo_refresh)`. Timeout → `RefreshAll` (5 min).
  Atende também `r` (refresh manual) e `ReadEmail(id, conta)` sob demanda.
  Roda os CLIs com `std::process::Command` e devolve resultados por `mpsc`.
  Nada disso bloqueia o relógio.

## Layout

```
┌───────────────────────────────────────────────┐
│      14:32:07   terça-feira, 09 de junho        │  header relógio
├──────────────────────┬──────────────────────────┤
│      E-MAILS         │       AGENDA             │
│ ● [W] Thiago …       │ 10:00 Daily              │
│ ● [P] Nota fiscal    │ 14:00 1:1 Milton         │
│   [W] Deploy aviso   ├──────────────────────────┤
│   [P] Fatura cartão  │    PRs (ghpending)       │
│   …                  │ #12 metabase fix…        │
├──────────────────────┴──────────────────────────┤
│ Tab · j/k · Enter · r · q                        │  footer help
└───────────────────────────────────────────────┘
```

- Esquerda: **E-mails** (coluna alta). Direita: **Agenda** (cima) e
  **PRs** (baixo).
- Foco: `Tab` cicla painel focado (borda destacada). `j/k` rola.
  `Enter` abre detalhe do e-mail (overlay centralizado); `Esc` fecha.
  `r` refresh manual; `q`/`Ctrl-C` sai.
- `●` = e-mail não lido. `[W]`/`[P]` = conta de origem.

## Dados

- **E-mail:** `himalaya envelope list -a {work,personal} -o json` → parse
  JSON. Detalhe sob demanda: `himalaya message read <id> -a <conta>`.
- **Agenda:** `env XDG_DATA_HOME=$HOME/.local/share/gcalcli-accounts/{personal,work}
  gcalcli agenda --tsv <hoje> <hoje+7d>` → parse TSV. Marcador de conta.
- **PRs:** `ghpending` → strip de códigos ANSI → linhas exibidas.

## Robustez

- Cada fetch é independente; falha de um não derruba os outros.
- Painel guarda último dado bom + indicador "atualizado há Xmin"
  (vermelho se a última busca falhou). O loop nunca quebra por erro de CLI.
- Spinner enquanto carrega.

## Arquivos

- `src/main.rs` — setup/teardown do terminal, event loop, canais.
- `src/app.rs` — estado (Model) + `update` (reducer): foco, scroll, seleção.
- `src/msg.rs` — enum `Msg`.
- `src/ui.rs` — layout e render (header, painéis, overlay, footer).
- `src/clock.rs` — formatação de data/hora pt-BR (pura, testada).
- `src/worker.rs` — thread de fundo (canal de comandos + período).
- `src/data/mod.rs`, `src/data/email.rs`, `src/data/agenda.rs`,
  `src/data/pulls.rs` — fetch (Command) + parsers.

## Testes (TDD)

Funções puras, sem terminal:
- parsers: JSON do himalaya → `Vec<EmailItem>`; TSV do gcalcli →
  `Vec<AgendaItem>`; strip ANSI do ghpending → linhas.
- `clock`: formatação pt-BR (dia da semana, mês, zero-padding).
- `app::update`: ciclo de foco, limites de scroll, seleção/abrir-fechar
  overlay.

## Fora de escopo (YAGNI)

- Escrita/ações (RSVP, responder e-mail).
- Abrir PR no browser.
- Configuração externa / múltiplos temas.
