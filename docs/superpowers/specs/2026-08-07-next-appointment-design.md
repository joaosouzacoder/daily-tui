# Próximo compromisso no header

**Data:** 2026-08-07
**Status:** aprovado

## Objetivo

Uma linha no header dizendo qual é o próximo compromisso e quanto falta para ele.

Nasceu ao lado do pomodoro e por causa dele: não faz sentido apertar `P` e
começar um bloco de foco de 25 minutos com reunião em 10. O painel de agenda já
mostra a semana, mas responder "posso começar agora?" exigia procurar na lista.

## Escopo

**Dentro:** a linha no header, derivada do que a agenda já busca — o próximo
evento com hora marcada dentro dos 7 dias que o `gcalcli` traz.

**Fora:**

- busca nova ou CLI novo: tudo sai de `app.agenda.items`, já em memória;
- eventos de dia inteiro — não colidem com um bloco de foco;
- saber se um evento está **acontecendo agora**: o `AgendaItem` guarda só o
  início, não o fim, e inventar duração seria mentira;
- qualquer ação sobre o compromisso (abrir, entrar na call, recusar);
- alerta sonoro ou notificação — o pomodoro já tem canal para isso, e ninguém
  pediu aviso de reunião.

## Decisões registradas

- **O header cresce de 8 para 9 linhas.** As 8 de hoje estão todas ocupadas
  (borda, 5 de glifos, data, borda). A alternativa — espremer o compromisso na
  linha da data — foi descartada: a data já tem ~33 colunas centralizadas e as
  duas coisas brigariam em terminal estreito.
- **A linha mora no bloco do relógio, não na caixa do pomodoro.** Assim ela usa
  toda a largura que sobra e comporta um título longo.
- **Sem agenda ligada, a linha não existe** e o header volta a 8 linhas. Mesma
  regra do pomodoro: recurso desligado não ocupa espaço.
- **`now` entra por parâmetro** nas funções puras, como no pomodoro. É o que
  permite testar "amanhã" e "quinta" sem depender de que dia é hoje.
- **Evento com data ou hora malformada é pulado, não derruba o painel.** Os dois
  campos vêm como string da saída do `gcalcli`.
- **Abaixo de 5 minutos a linha muda de cor.** Uma contagem que você não percebe
  não serve para nada, e 5 minutos é a janela em que ela precisa te interromper
  antes do `P`.
- **O painel de agenda passa a mostrar 2 dias; a busca continua em 7.** Pedido
  durante a implementação. São recortes diferentes do mesmo dado: o painel cabe
  em menos altura, e a linha do header continua enxergando a segunda-feira
  quando você olha numa sexta à noite. Cortar a *busca* teria apagado justamente
  o caso que motivou a decisão de "vai para o próximo dia".
- **A altura que a agenda liberou foi para os PRs** (`Agenda 40→25`,
  `Pulls 30→45`), também a pedido.
- **A linha do header não usa o `room_for` dos painéis.** O piso de 12 colunas
  dele empurra o marcador de conta para fora em terminal estreito. Aqui o
  "quando" e o marcador valem mais que três letras a mais do assunto.

## Arquitetura

### `src/data/agenda.rs` — a lógica, pura

```rust
/// O próximo evento que ainda não começou.
///
/// Ignora os de dia inteiro: eles não colidem com um bloco de foco. Item com
/// data ou hora que não parseia é pulado — a saída vem de um CLI externo.
pub fn next_upcoming<'a>(items: &'a [AgendaItem], now: DateTime<Local>) -> Option<&'a AgendaItem>;

/// O "quando" do próximo compromisso, já em português.
pub fn format_lead(item: &AgendaItem, now: DateTime<Local>) -> String;
```

`format_lead`, por distância:

| Situação | Texto |
|---|---|
| menos de 1 minuto | `agora` |
| menos de 1 hora, hoje | `em 12 min` |
| mais de 1 hora, hoje | `14:00 (em 5h)` |
| amanhã | `amanhã 09:00` |
| de 2 a 7 dias | `quinta 14:00` |

O nome do dia sai de `clock::weekday_short_ptbr`, que já existe.

### `src/ui.rs` — a linha

`render` passa o header de `Constraint::Length(8)` para 9 quando a agenda está
ligada; `render_clock` ganha uma terceira faixa de 1 linha abaixo da data.

Conteúdo, centralizado:

```
Próxima: em 12 min · 1:1 com o Milton [W]
```

O marcador de conta vem de `Account::marker()`, o mesmo `[W]`/`[P]` do painel de
agenda.

A linha nasceu em `muted` e foi promovida a `text` a pedido: `muted` é a cor da
data, e informação sobre a qual você age não pode ser tão apagada quanto o dia da
semana. A hierarquia do header fica em três níveis — relógio em `accent` negrito,
compromisso em `text`, data em `muted`.

Faltando 5 minutos ou menos, a linha vai para `warning` em negrito. `warning` e
não `error`: reunião chegando é aviso, não defeito, e assim o vermelho continua
significando só "quebrou" no resto do painel.

Sem próximo evento — agenda vazia, tudo no passado, ou a busca falhou — a linha
fica em branco e a altura não muda. Layout que pula quando o dado chega é pior
que uma linha vazia.

Título longo é cortado com o `clip` que o arquivo já usa, pela largura da faixa.

## Fluxo

Nada de estado novo. A cada `ClockTick` o `app.now` já avança e o `render` refaz
a linha a partir de `app.agenda.items`, que o refresh periódico mantém. A
contagem anda sozinha, um segundo por vez.

## Testes

**`agenda.rs`:**

- `next_upcoming` pula evento de dia inteiro;
- pula evento que já começou e devolve o seguinte;
- devolve `None` com a lista vazia e com tudo no passado;
- pula item com data ou hora malformada em vez de entrar em pânico;
- `format_lead` nos cinco formatos da tabela, com `now` fixo no teste.

**`ui.rs`:**

- o header mostra a linha quando há próximo evento;
- sem nada à frente a linha fica em branco e a altura não muda;
- com a agenda desligada no config o header volta a 8 linhas e a linha some;
- título longo é cortado **e o marcador de conta sobrevive** — o widget já
  trunca sozinho o que passa da largura, então "o fim do título sumiu" não prova
  nada sobre o nosso corte; o que só ele garante é o que vem depois do título. A
  asserção olha a linha do header, não a tela: o painel de agenda também desenha
  `[W]`, e olhar a tela inteira encontraria o dele.

### O que não é coberto por teste

A cor da linha abaixo de 5 minutos. O harness de render compara símbolos, não
estilo, e não há como afirmar pelo buffer que a linha ficou em destaque.
