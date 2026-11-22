//! Latencia de eco en el backend: write → PTY → fake-agent → ConPTY → lector →
//! flusher → suscriptor. Se mide con el instante exacto de llegada de cada chunk
//! (sin sondeo: en Windows `sleep` tiene una resolución de ~15,6 ms).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gryphus_agents::PtyRuntime;
use gryphus_core::ports::{AgentChunk, AgentRuntimePort, ChunkSink, SpawnSpec, TermSize};
use gryphus_core::{AgentId, ProviderConfig};
use gryphus_testkit::{fake_agent_bin, strip_ansi};

/// Acumula la salida y avisa (con el instante de llegada) en cada chunk.
struct TimedSink {
    output: Mutex<Vec<u8>>,
    tx: Mutex<Sender<Instant>>,
}

impl ChunkSink for TimedSink {
    fn send(&self, chunk: AgentChunk) -> bool {
        let at = Instant::now();
        if let AgentChunk::Data(b) | AgentChunk::Scrollback(b) = chunk {
            self.output.lock().unwrap().extend_from_slice(&b);
        }
        let _ = self.tx.lock().unwrap().send(at);
        true
    }
}

impl TimedSink {
    fn text(&self) -> String {
        strip_ansi(&self.output.lock().unwrap())
    }
}

/// Espera hasta que la salida contenga `needle`; devuelve cuándo llegó.
fn arrival(sink: &TimedSink, rx: &Receiver<Instant>, needle: &str) -> Instant {
    loop {
        let at = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("no llegó {needle:?}: {}", sink.text()));
        if sink.text().contains(needle) {
            return at;
        }
    }
}

#[test]
fn echo_latency_is_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = PtyRuntime::new();
    let agent = AgentId::from_raw(1);
    let spec = SpawnSpec::new(
        agent,
        &ProviderConfig::with_command(fake_agent_bin().to_string_lossy()),
        dir.path().to_path_buf(),
        TermSize::default(),
    );
    runtime.spawn(spec, Box::new(|_| {})).unwrap();
    let (tx, rx) = mpsc::channel();
    let sink = Arc::new(TimedSink {
        output: Mutex::new(Vec::new()),
        tx: Mutex::new(tx),
    });
    let _sub = runtime.attach(agent, sink.clone()).unwrap();
    arrival(&sink, &rx, "fake-agent ready");

    let mut samples = Vec::new();
    for i in 0..40 {
        std::thread::sleep(Duration::from_millis(40)); // reposo entre teclas
        while rx.try_recv().is_ok() {}
        let start = Instant::now();
        runtime.write(agent, format!("t{i}\r").as_bytes()).unwrap();
        let at = arrival(&sink, &rx, &format!("echo: t{i}\n"));
        samples.push(at - start);
    }
    runtime.kill_all();

    samples.sort();
    let median = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 95 / 100];
    let min = samples[0];
    println!("latencia de eco (backend): min {min:?}, mediana {median:?}, p95 {p95:?}");
    if timers_are_unreliable() {
        println!("umbral no exigido: runner de macOS en CI (timers imprecisos)");
        return;
    }
    assert!(median < Duration::from_millis(50), "mediana {median:?}");
}

/// En las VMs de macOS de GitHub Actions un `thread::sleep` de ≤16 ms (la espera
/// de coalescencia del flusher) tarda 50–140 ms: medido min 0,1 ms, mediana
/// 54–60 ms, p95 135–140 ms. Es el timer de la VM, no la PTY (el mínimo lo
/// demuestra). En un Mac local el umbral sí se exige.
fn timers_are_unreliable() -> bool {
    cfg!(target_os = "macos") && std::env::var_os("CI").is_some()
}
