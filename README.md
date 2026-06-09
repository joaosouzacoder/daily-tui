# daily-tui

Painel TUI para deixar sempre rodando num monitor, com o que importa no dia a dia:

- **Relógio** em tempo real (`HH:MM:SS` + data por extenso em pt-BR).
- **E-mails** agregados das contas work + personal (via `himalaya`).
- **Agenda** dos próximos 7 dias, agregada das duas contas Google (via `gcalcli`).
- **PRs/issues** pendentes nos repos monitorados (via `ghpending`).

Painel passivo com navegação leve: rola as listas e abre o corpo de um e-mail.
Os dados atualizam sozinhos a cada 5 minutos (ou na hora, com `r`).

## Pré-requisitos

As CLIs precisam estar instaladas e autenticadas:

- [`himalaya`](https://github.com/pimalaya/himalaya) com as contas `work` e `personal`.
- [`gcalcli`](https://github.com/insanum/gcalcli) com `XDG_DATA_HOME` isolado por
  conta em `~/.local/share/gcalcli-accounts/{work,personal}`.
- [`ghpending`](https://github.com/akitaonrails/ghpending) com os repos já adicionados.

## Build & run

```sh
cargo run --release
```

## Teclas

| Tecla        | Ação                                    |
|--------------|-----------------------------------------|
| `Tab` / `⇧Tab` | Troca o painel focado                 |
| `j` / `k`    | Rola para baixo / cima                  |
| `g` / `G`    | Topo / fim da lista                     |
| `Enter`      | Abre o corpo do e-mail (painel E-mails) |
| `Esc`        | Fecha o detalhe                         |
| `r`          | Atualiza os dados agora                 |
| `q` / `Ctrl-C` | Sai                                   |

## Arquitetura

- Estilo Elm/TEA do [`ratatui-tea`](https://crates.io/crates/ratatui-tea) +
  tema/componentes do [`ratatui-bubbletea`](https://github.com/akitaonrails/ratatui-bubbletea),
  mas com event loop próprio em `main.rs`.
- Uma **thread worker** (`worker.rs`) roda as CLIs e manda os resultados pelo
  canal do `ratatui-tea`, então nada bloqueia o relógio nem o teclado.

Veja o design em `docs/superpowers/specs/2026-06-09-daily-tui-design.md`.
