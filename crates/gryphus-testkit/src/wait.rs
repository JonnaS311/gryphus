//! Utilidades para pruebas con efectos asíncronos (watchers, procesos).

use std::thread::sleep;
use std::time::{Duration, Instant};

/// Reintenta `cond` cada 10 ms hasta que sea cierta o venza `timeout`.
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if cond() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        sleep(Duration::from_millis(10));
    }
}

/// Quita secuencias de escape ANSI/VT y `\r`, para comparar salida de terminal.
pub fn strip_ansi(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // CSI: ESC [ ... byte final en 0x40..=0x7E
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] ... BEL o ESC \
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_osc_and_carriage_returns() {
        let raw = b"\x1b[?25l\x1b[2Jhola\r\n\x1b]0;titulo\x07mundo\x1b[0m\r\n\x1b]0;t\x1b\\fin";
        assert_eq!(strip_ansi(raw), "hola\nmundo\nfin");
    }

    #[test]
    fn wait_until_times_out() {
        assert!(!wait_until(Duration::from_millis(30), || false));
        let mut n = 0;
        assert!(wait_until(Duration::from_secs(1), || {
            n += 1;
            n == 3
        }));
    }
}
