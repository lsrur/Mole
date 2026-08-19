//! FEAT-24/25 (§7.8): estado actual por máquina (REC_STATE) y LEDs de
//! status (REC_STATUS). Pocas entradas y baja frecuencia: Vec lineal.

/// Estado vigente de una máquina: lo mínimo para la tabla interina de
/// FEAT-24 (la lane temporal en canvas llega con F5).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MachineState {
    pub machine_sym: u16,
    pub state_sym: u16,
    /// t absoluto (µs) de la última transición real
    pub since_us: u64,
    pub transitions: u64,
}

#[derive(Debug, Default)]
pub struct StateStore {
    machines: Vec<MachineState>,
}

impl StateStore {
    pub fn push(&mut self, machine_sym: u16, state_sym: u16, t_us: u64) {
        match self.machines.iter_mut().find(|m| m.machine_sym == machine_sym) {
            Some(m) => {
                // re-emisión del mismo estado: idempotente, no es transición
                if m.state_sym != state_sym {
                    m.state_sym = state_sym;
                    m.since_us = t_us;
                    m.transitions += 1;
                }
            }
            None => self.machines.push(MachineState {
                machine_sym,
                state_sym,
                since_us: t_us,
                transitions: 0,
            }),
        }
    }

    pub fn snapshot(&self) -> &[MachineState] {
        &self.machines
    }

    /// "Limpiar" resetea lo acumulado; el estado vigente y su `since` quedan
    /// (son información de ahora, como los LEDs — no se re-emiten solos).
    pub fn reset_transitions(&mut self) {
        for m in &mut self.machines {
            m.transitions = 0;
        }
    }
}

/// Un LED de salud (FEAT-25). `level` según §7.8: OFF(0), VERDE(1),
/// AMARILLO(2), ROJO(3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusLed {
    pub sym: u16,
    pub level: u8,
    pub t_us: u64,
}

#[derive(Debug, Default)]
pub struct StatusStore {
    leds: Vec<StatusLed>,
}

impl StatusStore {
    pub fn push(&mut self, sym: u16, level: u8, t_us: u64) {
        match self.leds.iter_mut().find(|l| l.sym == sym) {
            Some(l) => {
                l.level = level;
                l.t_us = t_us;
            }
            None => self.leds.push(StatusLed { sym, level, t_us }),
        }
    }

    pub fn snapshot(&self) -> &[StatusLed] {
        &self.leds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transiciones_y_reemision() {
        let mut s = StateStore::default();
        s.push(7, 8, 1000);
        assert_eq!(s.snapshot()[0].transitions, 0);
        s.push(7, 8, 2000); // mismo estado: ni transición ni reset de since
        assert_eq!(s.snapshot()[0].transitions, 0);
        assert_eq!(s.snapshot()[0].since_us, 1000);
        s.push(7, 9, 3000);
        assert_eq!(s.snapshot()[0].transitions, 1);
        assert_eq!(s.snapshot()[0].since_us, 3000);
        s.push(3, 4, 3500); // segunda máquina, entrada propia
        assert_eq!(s.snapshot().len(), 2);
    }

    #[test]
    fn leds_actualizan_en_lugar() {
        let mut s = StatusStore::default();
        s.push(11, 1, 100);
        s.push(12, 3, 200);
        s.push(11, 2, 300);
        assert_eq!(s.snapshot().len(), 2);
        assert_eq!(s.snapshot()[0].level, 2);
        assert_eq!(s.snapshot()[0].t_us, 300);
    }
}
