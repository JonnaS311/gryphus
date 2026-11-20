//! Marcas de rendimiento (`performance.mark`) para medir el arranque.

pub fn mark(name: &str) {
    if let Some(perf) = web_sys::window().and_then(|w| w.performance()) {
        let _ = perf.mark(name);
    }
}
