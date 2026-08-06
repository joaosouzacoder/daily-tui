//! Máquina de estados do pomodoro. Sem I/O e sem relógio próprio: quem chama
//! passa o `Instant`, e é isso que permite testar a virada de fase sem esperar
//! 25 minutos.

use std::time::{Duration, Instant};

/// Em que metade do ciclo o pomodoro está.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Break,
}

impl Phase {
    /// Como a fase aparece na caixa do header.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Foco",
            Phase::Break => "Descanso",
        }
    }
}

/// O pomodoro: fase atual, quanto falta e quantos focos já fecharam.
///
/// Guarda o **instante em que a fase acaba**, não uma duração que diminui a cada
/// tick. O `ClockTick` do `main` dispara quando `last_tick.elapsed() >= 1s` e o
/// poll de teclado é de 200ms, então dois ticks consecutivos podem estar a 1,2s
/// de distância: decrementar acumularia minutos de erro ao longo de um foco de
/// 25. Com prazo, o tempo restante é uma subtração e não erra.
#[derive(Debug)]
pub struct Pomodoro {
    phase: Phase,
    /// `Some` = rodando, e este é o instante em que a fase acaba.
    deadline: Option<Instant>,
    /// O que sobra da fase quando parado (e no arranque).
    left: Duration,
    /// Focos concluídos nesta sessão.
    done: u32,
    focus: Duration,
    rest: Duration,
}

impl Pomodoro {
    /// Foco cheio, parado, esperando você.
    pub fn new(focus: Duration, rest: Duration) -> Self {
        Self {
            phase: Phase::Focus,
            deadline: None,
            left: focus,
            done: 0,
            focus,
            rest,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn done(&self) -> u32 {
        self.done
    }

    pub fn running(&self) -> bool {
        self.deadline.is_some()
    }

    /// Tempo cheio da fase atual — o denominador da barra de progresso.
    pub fn total(&self) -> Duration {
        match self.phase {
            Phase::Focus => self.focus,
            Phase::Break => self.rest,
        }
    }

    /// Quanto falta. Nunca negativo: `Duration` não tem sinal, e um prazo
    /// vencido entre dois ticks entregaria panic em vez de zero.
    pub fn remaining(&self, now: Instant) -> Duration {
        match self.deadline {
            Some(deadline) => deadline.saturating_duration_since(now),
            None => self.left,
        }
    }

    /// Inicia ou pausa. Pausar guarda o que sobrou; iniciar arma o prazo a
    /// partir dele.
    pub fn toggle(&mut self, now: Instant) {
        match self.deadline.take() {
            Some(deadline) => self.left = deadline.saturating_duration_since(now),
            None => self.deadline = Some(now + self.left),
        }
    }

    /// Devolve a fase atual ao tempo cheio, parada. Não mexe em `done`.
    pub fn reset(&mut self) {
        self.deadline = None;
        self.left = self.total();
    }

    /// Avança o relógio e devolve a fase que **acabou**, quando acabou.
    ///
    /// A próxima fase é armada a partir de `now`, não do prazo vencido: depois
    /// de a máquina dormir uma hora, encadear pelo prazo antigo dispararia
    /// várias viradas seguidas — uma rajada de notificações por um tempo que
    /// você não passou trabalhando.
    pub fn tick(&mut self, now: Instant) -> Option<Phase> {
        let deadline = self.deadline?;
        if now < deadline {
            return None;
        }
        let ended = self.phase;
        match ended {
            Phase::Focus => {
                self.done += 1;
                self.phase = Phase::Break;
                self.left = self.rest;
                self.deadline = Some(now + self.rest);
            }
            Phase::Break => {
                self.phase = Phase::Focus;
                self.left = self.focus;
                self.deadline = None;
            }
        }
        Some(ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOCUS: Duration = Duration::from_secs(25 * 60);
    const REST: Duration = Duration::from_secs(5 * 60);

    fn pomodoro() -> Pomodoro {
        Pomodoro::new(FOCUS, REST)
    }

    #[test]
    fn starts_stopped_on_a_full_focus() {
        let p = pomodoro();
        assert!(!p.running());
        assert_eq!(p.phase(), Phase::Focus);
        assert_eq!(p.remaining(Instant::now()), FOCUS);
        assert_eq!(p.done(), 0);
    }

    #[test]
    fn toggle_starts_and_the_second_toggle_keeps_what_was_left() {
        let now = Instant::now();
        let mut p = pomodoro();
        p.toggle(now);
        assert!(p.running());

        // Pausar dez minutos adiante tem de guardar os 15 que sobraram — não
        // voltar ao tempo cheio nem seguir contando.
        let later = now + Duration::from_secs(10 * 60);
        p.toggle(later);
        assert!(!p.running());
        assert_eq!(p.remaining(later), Duration::from_secs(15 * 60));
        // Parado, o tempo restante não anda mais.
        assert_eq!(
            p.remaining(later + Duration::from_secs(600)),
            Duration::from_secs(15 * 60)
        );
    }

    #[test]
    fn a_tick_before_the_deadline_changes_nothing() {
        let now = Instant::now();
        let mut p = pomodoro();
        p.toggle(now);
        assert_eq!(p.tick(now + Duration::from_secs(24 * 60)), None);
        assert_eq!(p.phase(), Phase::Focus);
        assert_eq!(p.done(), 0);
    }

    #[test]
    fn a_tick_while_stopped_never_fires() {
        // Sem isso, um pomodoro parado no arranque avisaria sozinho.
        let mut p = pomodoro();
        assert_eq!(p.tick(Instant::now() + Duration::from_secs(9999)), None);
    }

    #[test]
    fn the_end_of_a_focus_counts_it_and_chains_into_the_break() {
        let now = Instant::now();
        let mut p = Pomodoro::new(Duration::ZERO, REST);
        p.toggle(now);
        assert_eq!(p.tick(now), Some(Phase::Focus));
        assert_eq!(p.phase(), Phase::Break);
        assert_eq!(p.done(), 1);
        // O descanso já está correndo: você não precisa voltar ao teclado.
        assert!(p.running());
        assert_eq!(p.remaining(now), REST);
    }

    #[test]
    fn the_end_of_a_break_stops_on_a_full_focus() {
        let now = Instant::now();
        let mut p = Pomodoro::new(FOCUS, Duration::ZERO);
        p.toggle(now);
        let later = now + FOCUS;
        p.tick(later); // fim do foco, entra no descanso de zero
        assert_eq!(p.tick(later), Some(Phase::Break));
        assert_eq!(p.phase(), Phase::Focus);
        // Para e espera: começar o próximo foco é decisão sua.
        assert!(!p.running());
        assert_eq!(p.remaining(later), FOCUS);
        assert_eq!(p.done(), 1);
    }

    #[test]
    fn remaining_on_an_overdue_deadline_is_zero_instead_of_underflowing() {
        // `Duration` não tem negativo: subtrair errado aqui é panic em release.
        let now = Instant::now();
        let mut p = pomodoro();
        p.toggle(now);
        assert_eq!(p.remaining(now + Duration::from_secs(3600)), Duration::ZERO);
    }

    #[test]
    fn reset_refills_the_phase_and_keeps_the_finished_focuses() {
        // Um `R` sem querer não pode apagar o que você já fez no dia.
        let now = Instant::now();
        let mut p = Pomodoro::new(Duration::ZERO, REST);
        p.toggle(now);
        p.tick(now);
        assert_eq!(p.done(), 1);

        p.reset();
        assert!(!p.running());
        assert_eq!(p.phase(), Phase::Break);
        assert_eq!(p.remaining(now), REST);
        assert_eq!(p.done(), 1);
    }

    #[test]
    fn total_follows_the_current_phase_so_the_bar_has_a_denominator() {
        let now = Instant::now();
        let mut p = Pomodoro::new(Duration::ZERO, REST);
        assert_eq!(p.total(), Duration::ZERO);
        p.toggle(now);
        p.tick(now);
        assert_eq!(p.total(), REST);
    }

    #[test]
    fn the_phase_label_is_the_one_shown_on_screen() {
        assert_eq!(Phase::Focus.label(), "Foco");
        assert_eq!(Phase::Break.label(), "Descanso");
    }

    #[test]
    fn waking_from_sleep_arms_the_next_phase_from_now_not_from_the_stale_deadline() {
        // Sem armação a partir de `now`, a máquina que acorda de uma hora de sono
        // dispararia vários descansos seguidos — uma rajada de transições que você
        // não trabalhou para passar.
        let now = Instant::now();
        let mut p = Pomodoro::new(Duration::ZERO, REST);
        p.toggle(now);
        // Ticking uma hora depois da deadline (que seria `now + 0`).
        let much_later = now + Duration::from_secs(3600);
        assert_eq!(p.tick(much_later), Some(Phase::Focus));
        // O descanso acaba de começar. Se fosse armado do prazo antigo
        // (now + 0), o deadline seria (now + 0) + REST = now + REST, e
        // remaining() mostraria menos que REST. Se armado de `now` (correto),
        // seria much_later + REST, e remaining() mostra o tempo cheio.
        assert_eq!(p.remaining(much_later), REST);
    }
}
