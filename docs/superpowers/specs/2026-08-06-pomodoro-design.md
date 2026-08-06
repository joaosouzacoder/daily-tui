# Pomodoro no header, com notificação de sistema

**Data:** 2026-08-06
**Status:** aprovado, aguardando plano de implementação

## Objetivo

Uma caixinha de pomodoro ao lado do relógio: tempo de foco e de descanso vindos
do config, iniciar/pausar pelo teclado, e um aviso quando a fase vira — no
sistema operacional, com o ntfy.sh como rede de segurança.

O canal de notificação nasce **genérico**: nada nele sabe o que é pomodoro, para
que qualquer painel possa avisar depois (e-mail novo, PR aprovado, menção no
Jira) sem reescrever esta parte.

## Escopo

**Dentro:**

1. Caixa no header, à direita do relógio, com fase, tempo restante, barra de
   progresso, contador de focos e as teclas.
2. Tempo de foco e de descanso configuráveis; a caixa desligável pelo config.
3. `P` inicia/pausa, `R` zera a fase atual. Globais.
4. Notificação nativa em Windows, macOS e Linux quando a fase acaba.
5. Fallback para ntfy.sh, com o tópico no config, quando a nativa falha.
6. Falha de notificação visível na tela.

**Fora:**

- pausa longa a cada 4 pomodoros (o ciclo é foco/descanso, sem ciclo maior);
- histórico ou persistência: fechar o painel esquece o contador de focos;
- amarrar o pomodoro a uma tarefa do To Do;
- som ou bell no terminal;
- outros painéis usando o `notify` — o módulo nasce pronto para isso, mas nenhum
  chamador novo entra nesta mudança.

## Decisões registradas

- **Prazo, não contador decrescente.** O `ClockTick` do `main.rs` dispara quando
  `last_tick.elapsed() >= 1s`, e o poll de teclado é de 200ms — dois ticks
  consecutivos podem estar a 1,2s de distância. Decrementar uma duração a cada
  tick acumularia erro de minutos ao longo de um foco de 25. O estado guarda o
  `Instant` em que a fase acaba; o tempo restante é uma subtração.
- **O ciclo encadeia sozinho, e para no fim do descanso.** Fim do foco → o
  descanso já começa contando (você não precisa voltar ao teclado para
  descansar). Fim do descanso → volta para o foco **parado**, esperando você
  decidir que começou.
- **`R` zera só a fase atual** e mantém o contador de focos. Um `R` sem querer
  não apaga o que você já fez no dia.
- **A notificação roda no worker.** Falar com o DBus ou subir um processo é I/O;
  no loop principal, travaria o relógio.
- **ntfy.sh por `curl`, não por cliente HTTP em Rust.** O projeto inteiro delega
  a CLIs externos (himalaya, gcalcli, ghpending, jira, mstodo). Um cliente HTTP
  traria TLS e um bloco de dependências para uma única requisição de uma linha.
- **Falha de notificação não é engolida.** Achar que vai ser avisado e não ser é
  o pior defeito possível aqui.
- **Sem sufixo `(parado)` na fase (revisão do task 5).** A linha de dicas já diz
  `P iniciar` quando parado e `P pausar` quando rodando, então o sufixo era
  redundante — e ele encostava na folga que a caixa de 20 colunas úteis tem
  contra o contador de dois dígitos, cortando o contador de focos em silêncio.

## Arquitetura

Dois módulos novos, independentes um do outro.

### `src/pomodoro.rs` — máquina de estados, pura

```rust
pub enum Phase { Focus, Break }

pub struct Pomodoro {
    phase: Phase,
    /// `Some` = rodando: o instante em que a fase acaba.
    deadline: Option<Instant>,
    /// O que sobra da fase quando pausado (e no arranque).
    left: Duration,
    /// Focos concluídos nesta sessão.
    done: u32,
    focus: Duration,
    rest: Duration,
}
```

Interface:

| Função | Faz |
|---|---|
| `new(focus, rest)` | Foco cheio, parado, zero focos feitos |
| `toggle(now)` | Inicia (arma o prazo a partir de `left`) ou pausa (guarda o que sobrou) |
| `reset()` | Fase atual volta ao tempo cheio, parada; `done` intacto |
| `tick(now) -> Option<Phase>` | Devolve a fase que **acabou**, e já encadeia |
| `remaining(now) -> Duration` | Nunca negativo (`saturating_sub`) |
| `total() -> Duration` | Tempo cheio da fase atual, para a barra |
| `running() -> bool`, `phase()`, `done()` | Leitura para a tela |

`now` entra por parâmetro em toda função que depende do tempo: é isso que
permite testar o encadeamento sem `sleep`.

`tick` só age quando `deadline` existe e `now >= deadline`. Ao acabar um foco:
`done += 1`, fase vira `Break` com o prazo já armado, devolve `Some(Focus)`. Ao
acabar um descanso: fase vira `Focus`, `deadline = None`, `left = focus`,
devolve `Some(Break)`.

### `src/notify.rs` — canal genérico

```rust
pub struct Notice {
    pub title: String,
    pub body: String,
}

/// Manda pelo primeiro canal que funcionar. Erro só quando nenhum funciona.
pub fn send(n: &Notice) -> Result<(), String>;
```

`Notice` tem os dois campos que todo canal entende, e nada além. Prioridade,
ícone e tags existem no ntfy, mas nenhum chamador precisa deles nesta mudança —
quem precisar depois acrescenta o campo com o caso de uso na mão.

Ordem: notificação nativa via `notify-rust` (Windows/macOS/Linux) → se falhar e
`[notify].ntfy_topic` não estiver vazio, `curl.exe -s -H "Title: …" -d corpo
ntfy.sh/<topico>`. O erro devolvido nomeia os dois canais que falharam.

Sem tópico no config, o único canal é o nativo — e a falha dele aparece na tela.

### Ligação com o que já existe

| Arquivo | Mudança |
|---|---|
| `src/msg.rs` | `Msg::Notified(Result<(), String>)` |
| `src/worker.rs` | `WorkerCmd::Notify(Notice)` → `notify::send` → `Msg::Notified` |
| `src/app.rs` | campo `pomodoro: Pomodoro`, `notify_error: Option<String>`; `ClockTick` chama `tick` e manda o `Notify`; `P`/`R` em `handle_panel_key` |
| `src/ui.rs` | `render_header` divide horizontalmente; `render_pomodoro` novo |
| `src/config.rs` | `PomodoroCfg`, `NotifyCfg`, validação |
| `Cargo.toml` | `notify-rust = "4"` |

O `tick` do pomodoro fica no braço `Msg::ClockTick` do `update`, ao lado do
`self.now = Local::now()` — o tempo já é atualizado ali, e o tick chega mesmo com
overlay aberto, então o contador não congela quando você está lendo um e-mail.

## Fluxo de dados

```
main.rs (1s)  ──Msg::ClockTick──▶  App::update
                                      │ pomodoro.tick(now)
                                      │   Some(Phase) ?
                                      ▼
                              WorkerCmd::Notify(Notice)  ──▶  worker
                                                                │ notify::send
                                                                │   nativo → ntfy
                                      ◀──Msg::Notified(res)─────┘
                                      │
                                      ▼
                              notify_error = res.err()
```

Teclado: `P`/`R` mexem só no `Pomodoro` em memória. Nenhuma das duas fala com o
worker — o aviso nasce da virada de fase, não da tecla.

## Tela

Header continua com 8 linhas. `render_header` passa a dividir a área na
horizontal: relógio em `Constraint::Min(0)`, caixa em `Constraint::Length(22)`.
Com `enabled = false`, o relógio recebe a largura inteira e nada mais muda.

```
┌────────────────────────────────┐┌─ pomodoro ─────────┐
│                                ││ Foco          3 ✓  │
│    ███ ███   ██  ███ ███       ││                    │
│    █ █  █  █ █ █ █ █ █ █       ││ 18:42              │
│    ███ ███   ███ ███ ███       ││ ██████████░░░░░░   │
│                                ││ P pausar · R zerar │
│  terça-feira, 06 de agosto     ││                    │
└────────────────────────────────┘└────────────────────┘
```

- **Fase:** `Foco` ou `Descanso`, sem sufixo para o estado parado — a linha de
  dicas já diz `P iniciar`/`P pausar`.
- **Contador:** focos concluídos, à direita. Zero focos não mostra nada.
- **Tempo:** `MM:SS`, texto normal (o relógio grande ao lado já é o destaque).
- **Barra:** cheia proporcional ao decorrido da fase.
- **Última linha:** `P pausar · R zerar`, ou `P iniciar · R zerar` quando parado.
  Com `notify_error` preenchido, ela cede o lugar para `⚠ aviso não saiu`, em
  estilo de erro. Volta ao normal no próximo aviso que sai, ou num `P`/`R`.

O bloco da esquerda não ganha título nem muda de estilo: a mudança é o split e a
caixa nova.

## Mensagens do aviso

Tom das outras notificações dele: o resultado na primeira frase, uma ou duas
linhas, português.

| Virada | Título | Corpo |
|---|---|---|
| Fim do foco | `Pomodoro: hora da pausa` | `25 min de foco fechados. Descanso de 5 min já começou.` |
| Fim do descanso | `Pomodoro: de volta ao foco` | `Pausa de 5 min terminou. Aperte P para o próximo.` |

Os minutos vêm do config, não são fixos no texto.

## Config

```toml
[pomodoro]
enabled = true
focus   = 25   # minutos de foco
rest    = 5    # minutos de descanso

[notify]
# Vazio = só a notificação do sistema. Com tópico, o ntfy.sh entra quando a
# notificação nativa falha.
ntfy_topic = ""
```

Validação, junto da que já recusa config sem painel: `focus` e `rest` têm de ser
maiores que zero. Zero faria a fase virar a cada tick, num laço de notificações.
Erro de uma linha, no stderr, antes da tela abrir — como os outros.

`print_shell` não muda: o `setup-auth.sh` cobra autenticação de ferramenta
externa, e o pomodoro não tem nenhuma.

## Erros

| Situação | Comportamento |
|---|---|
| Notificação nativa falha, sem tópico ntfy | `⚠ aviso não saiu` na caixa; o ciclo continua |
| Nativa falha, ntfy funciona | Silêncio: o aviso saiu |
| Nativa e ntfy falham | `⚠ aviso não saiu`; o erro nomeia os dois canais |
| `curl` não existe no PATH | Conta como falha do ntfy, com o motivo |
| `focus = 0` no config | Arranque recusado, com o motivo |

Uma virada de fase nunca é desfeita por falha de aviso: o pomodoro é a fonte da
verdade do tempo, e a notificação é um efeito colateral dele.

## Testes

**`pomodoro.rs`** — a máquina, sem `sleep`:

- parado no arranque, com o foco cheio;
- `toggle` inicia; `toggle` de novo pausa preservando o que sobrou;
- `tick` antes do prazo não vira fase e não devolve nada;
- `tick` no prazo devolve `Focus`, incrementa `done` e deixa o descanso rodando;
- fim do descanso devolve `Break` e deixa o foco **parado**;
- `remaining` num prazo já vencido é zero, não estoura;
- `reset` volta a fase ao cheio e não mexe em `done`.

**`config.rs`:**

- o `config.example.toml` comitado parseia com o novo `[pomodoro]` e `[notify]`;
- arquivo vazio continua valendo o default (25/5, ligado, sem tópico);
- `focus = 0` é recusado e o erro diz qual campo.

**`ui.rs`** — os formatadores puros:

- `MM:SS` com zero à esquerda; 90s vira `01:30`;
- a barra respeita a largura pedida e enche proporcionalmente;
- fase cheia e fase vazia dão barra cheia e barra vazia.

**`notify.rs`:** a montagem do `Notice` e dos argumentos do `curl` são funções
puras testáveis. O envio de verdade não entra em teste — depende de DBus, de
toast do Windows e de rede.

## Risco conhecido

`notify-rust` no Windows depende de um AppUserModelID válido para o toast. Se na
prática o toast não aparecer nesta máquina, o plano de implementação troca o
módulo nativo por subprocesso por plataforma (`powershell` no Windows,
`notify-send` no Linux, `osascript` no macOS) — a interface `notify::send` e
tudo que depende dela ficam iguais. É por isso que o canal é um módulo com uma
função, e não código espalhado no `app.rs`.
