//! Detecta cuándo un agente terminó su tarea y espera instrucciones.
//!
//! Los CLIs de agente no avisan de forma uniforme, así que se combinan dos señales:
//! - **Silencio tras trabajar:** el usuario envió algo (Enter), el agente produjo
//!   salida (mientras trabaja anima un indicador) y luego dejó de escribir durante
//!   [`IDLE_AFTER`].
//! - **Aviso explícito de terminal:** campana (BEL) u OSC 9 / OSC 777 (notificaciones
//!   de escritorio que emiten Claude Code y otros al terminar).
//!
//! [`Activity`] es la regla pura (tiempo inyectado); [`ActivitySink`] la conecta a
//! la salida del agente y el hilo de [`crate::session`] la consulta periódicamente.

use std::sync::Weak;
use std::time::{Duration, Instant};

use gryphus_core::ports::{AgentChunk, ChunkSink};
use gryphus_core::AgentId;

use crate::session::Inner;

/// Silencio que se interpreta como "terminó".
pub(crate) const IDLE_AFTER: Duration = Duration::from_secs(4);
/// Cada cuánto se revisa el silencio.
pub(crate) const POLL_EVERY: Duration = Duration::from_millis(500);

/// Estado de una secuencia de escape en curso (solo interesa distinguir el BEL
/// suelto del que termina una OSC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Esc {
    #[default]
    Ground,
    /// Tras ESC.
    Escape,
    /// Dentro de `ESC ]`: se acumula el número de la OSC.
    Osc,
    /// ESC dentro de una OSC (posible terminador `ESC \`).
    OscEscape,
}

#[derive(Debug)]
pub(crate) struct Activity {
    /// Se envió una orden (Enter) y aún no se avisó de su fin.
    armed: bool,
    /// Hubo salida desde que se armó.
    worked: bool,
    last_output: Instant,
    esc: Esc,
    osc: Vec<u8>,
}

impl Activity {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            armed: false,
            worked: false,
            last_output: now,
            esc: Esc::Ground,
            osc: Vec::new(),
        }
    }

    /// Entrada del usuario hacia el agente. Enter arma la detección.
    pub(crate) fn on_input(&mut self, bytes: &[u8], now: Instant) {
        if bytes.contains(&b'\r') || bytes.contains(&b'\n') {
            self.armed = true;
            self.worked = false;
            self.last_output = now;
        }
    }

    /// Salida del agente. Devuelve `true` si contiene un aviso explícito de terminal.
    pub(crate) fn on_output(&mut self, bytes: &[u8], now: Instant) -> bool {
        self.last_output = now;
        if self.armed {
            self.worked = true;
        }
        let mut notified = false;
        for &b in bytes {
            self.esc = match (self.esc, b) {
                (Esc::Ground, 0x07) => {
                    notified = true;
                    Esc::Ground
                }
                (Esc::Ground, 0x1b) => Esc::Escape,
                (Esc::Ground, _) => Esc::Ground,
                (Esc::Escape, b']') => {
                    self.osc.clear();
                    Esc::Osc
                }
                (Esc::Escape, 0x1b) => Esc::Escape,
                (Esc::Escape, _) => Esc::Ground,
                (Esc::Osc, 0x07) => {
                    notified |= self.osc_is_notification();
                    Esc::Ground
                }
                (Esc::Osc, 0x1b) => Esc::OscEscape,
                (Esc::Osc, b) => {
                    // Solo hace falta el número inicial; el resto (título, texto) se descarta.
                    if self.osc.len() < 8 {
                        self.osc.push(b);
                    }
                    Esc::Osc
                }
                (Esc::OscEscape, b'\\') => {
                    notified |= self.osc_is_notification();
                    Esc::Ground
                }
                (Esc::OscEscape, _) => Esc::Ground,
            };
        }
        if notified {
            self.armed = false;
        }
        notified
    }

    /// `true` una sola vez cuando, tras una orden, el agente trabajó y quedó en silencio.
    pub(crate) fn poll(&mut self, now: Instant) -> bool {
        if self.armed && self.worked && now.duration_since(self.last_output) >= IDLE_AFTER {
            self.armed = false;
            return true;
        }
        false
    }

    /// OSC 9 (iTerm2/ConEmu; `9;4` es progreso, no aviso) u OSC 777 (`notify`).
    fn osc_is_notification(&self) -> bool {
        let osc = self.osc.as_slice();
        (osc.starts_with(b"9;") && !osc.starts_with(b"9;4;")) || osc.starts_with(b"777;")
    }
}

/// Suscriptor interno a la salida de un agente.
pub(crate) struct ActivitySink {
    pub(crate) session: Weak<Inner>,
    pub(crate) agent: AgentId,
}

impl ChunkSink for ActivitySink {
    fn send(&self, chunk: AgentChunk) -> bool {
        let Some(inner) = self.session.upgrade() else {
            return false;
        };
        match chunk {
            AgentChunk::Scrollback(_) => true,
            AgentChunk::Data(bytes) => {
                inner.agent_output(self.agent, &bytes);
                true
            }
            AgentChunk::Exited(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn silence_after_work_fires_once() {
        let t = Instant::now();
        let mut a = Activity::new(t);
        a.on_input(b"haz algo\r", t);
        a.on_output(b"pensando...", at(t, 100));
        a.on_output(b"listo", at(t, 2000));
        assert!(!a.poll(at(t, 5000)));
        assert!(a.poll(at(t, 6000)));
        assert!(!a.poll(at(t, 20000)));
    }

    #[test]
    fn output_without_an_order_does_not_fire() {
        let t = Instant::now();
        let mut a = Activity::new(t);
        a.on_output(b"banner de arranque", t);
        a.on_input(b"escribiendo", at(t, 10));
        a.on_output(b"escribiendo", at(t, 20));
        assert!(!a.poll(at(t, 60000)));
    }

    #[test]
    fn an_order_without_output_does_not_fire() {
        let t = Instant::now();
        let mut a = Activity::new(t);
        a.on_input(b"\r", t);
        assert!(!a.poll(at(t, 60000)));
    }

    #[test]
    fn bell_notifies_but_osc_terminator_does_not() {
        let t = Instant::now();
        let mut a = Activity::new(t);
        assert!(!a.on_output(b"\x1b]0;titulo\x07texto", t));
        assert!(a.on_output(b"fin\x07", t));
    }

    #[test]
    fn desktop_notifications_notify_and_disarm() {
        let t = Instant::now();
        let mut a = Activity::new(t);
        a.on_input(b"\r", t);
        assert!(a.on_output(b"\x1b]9;Claude termino\x1b\\", t));
        assert!(!a.poll(at(t, 60000)), "no se avisa dos veces");
        assert!(a.on_output(b"\x1b]777;notify;Codex;listo\x07", t));
        assert!(!a.on_output(b"\x1b]9;4;1;50\x07", t), "OSC 9;4 es progreso");
    }

    #[test]
    fn osc_split_across_chunks_is_parsed() {
        let t = Instant::now();
        let mut a = Activity::new(t);
        assert!(!a.on_output(b"\x1b]9;hol", t));
        assert!(a.on_output(b"a\x07", t));
    }
}
