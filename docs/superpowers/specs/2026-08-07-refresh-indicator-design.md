# Indicador de "recarregando" no footer

**Data:** 2026-08-07
**Status:** aprovado

## Objetivo

O footer mostra `⟳ HH:MM:SS` — a hora do último refresh — e nada mais. Enquanto
os dados estão sendo rebuscados, a tela fica idêntica a uma tela parada: você não
sabe se o painel está trabalhando ou se travou. Este indicador fecha essa lacuna.

## Escopo

**Dentro:** o refresh completo — o periódico (300s) e o disparado por `r`,
incluindo o do arranque.

**Fora:**

- as buscas de painel (`f` no Jira, `n` nas menções) e as rebuscas depois de uma
  escrita. Elas já dão retorno no próprio painel, e piscar o footer a cada tecla
  vira ruído;
- a listagem de pastas (`refresh_folders`), que tem TTL próprio e nada na tela
  espera por ela;
- animação mais rápida que o tick de 1s do app (ver "Cadência").

## Decisões registradas

- **O worker avisa; o App não deduz.** O refresh periódico nasce do timeout do
  `recv_timeout` do próprio worker, então nada fora dele sabe que começou.
- **Um par de mensagens cercando `refresh_all`, não uma contagem de painéis.**
  Deduzir "acabou" de "todos os painéis responderam" prende o spinner para sempre
  quando um painel falha e não responde. `refresh_all` sempre retorna, então o
  par é exato — e é o que o teste da regressão cobre.
- **O mesmo spinner do painel vazio.** `app.spinner` já existe e já é ticado a
  cada segundo. Um segundo spinner com animação própria faria a tela discordar
  de si mesma.

## Arquitetura

### Mensagens

```rust
// src/msg.rs
/// O worker começou um refresh completo.
RefreshStarted,
/// O worker terminou o refresh completo (todos os resultados já foram enviados).
RefreshDone,
```

### Worker

O par mora **dentro** de `refresh_all`, não nos dois lugares que a chamam (o
arranque e o laço). Foi mudado durante a implementação: cercar nos call sites
duplicaria as duas linhas e deixaria um terceiro chamador futuro livre para
esquecer o fechamento. `refresh_folders`, que roda em seguida, fica de fora do
par — quando ela começa, todo resultado de painel já foi enviado.

### App

```rust
// src/app.rs
/// Refresh completo em andamento, mostrado no footer.
pub refreshing: bool,
```

`RefreshStarted` liga, `RefreshDone` desliga. Nada mais escreve nesse campo.

### Footer

Em `src/ui.rs`, `render_footer`, a coluna da direita (`Length(22)`):

| Estado | Mostra |
|---|---|
| `refreshing` | `⠙ recarregando` (o quadro atual do spinner + a palavra) |
| `last_refresh = Some(t)` | `⟳ HH:MM:SS` — como hoje |
| `last_refresh = None` | `⟳ …` — como hoje |

O quadro sai de `app.spinner.current_frame()`, que devolve `&'static str`.

## Cadência

O spinner avança em `Msg::ClockTick`, que chega uma vez por segundo. É uma
pulsação visível, não um giro suave. Acelerar exigiria um tick mais rápido e mais
redesenhos para um ganho estético, e faria este spinner discordar do que já roda
no painel vazio.

## Largura

`⠙ recarregando` são 14 colunas nas 22 disponíveis. Um teste fixa isso: texto que
passe da coluna é cortado em silêncio pelo widget, e a palavra ficaria truncada.

## Testes

- `RefreshStarted` liga o campo; `RefreshDone` desliga.
- `RefreshDone` desliga **mesmo depois de um painel ter voltado com erro** — é a
  regressão do spinner preso, e o motivo de o par existir.
- O footer mostra `recarregando` durante o refresh e a hora fora dele.
- O texto de recarregando cabe nas 22 colunas da coluna do status, em todo quadro
  do spinner — e o teste também exige a palavra, senão ele mediria a largura do
  texto errado e ficaria verde com o ramo de "recarregando" apagado.

### O que não é coberto por teste

O envio do par pelo próprio `refresh_all`. Testá-lo exigiria chamar a função, que
busca de verdade nos CLIs de cada painel ligado — e os painéis vêm do config, que
é um `OnceLock` que teste nenhum consegue virar. Os testes cobrem o que o App faz
ao receber cada mensagem; que o worker as manda é verificado à mão, abrindo o
painel e olhando o indicador no arranque.
