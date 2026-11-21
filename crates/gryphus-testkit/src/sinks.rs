//! Receptores que acumulan lo recibido para hacer aserciones.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use gryphus_core::ports::{AgentChunk, AgentExit, ChunkSink, FsChangeSink};
use gryphus_core::{DomainEvent, EventSink, RelPath};

use crate::fakes::lock;
use crate::wait::strip_ansi;

/// Registra cada `DomainEvent` publicado.
#[derive(Default)]
pub struct RecordingEventSink {
    events: Mutex<Vec<DomainEvent>>,
}

impl RecordingEventSink {
    pub fn events(&self) -> Vec<DomainEvent> {
        lock(&self.events).clone()
    }

    /// Devuelve y vacía los eventos registrados.
    pub fn take(&self) -> Vec<DomainEvent> {
        std::mem::take(&mut *lock(&self.events))
    }
}

impl EventSink for RecordingEventSink {
    fn publish(&self, event: DomainEvent) {
        lock(&self.events).push(event);
    }
}

/// Acumula la salida de un agente. [`close`](Self::close) simula que la UI se fue.
#[derive(Default)]
pub struct CollectingChunkSink {
    chunks: Mutex<Vec<AgentChunk>>,
    closed: AtomicBool,
}

impl CollectingChunkSink {
    pub fn chunks(&self) -> Vec<AgentChunk> {
        lock(&self.chunks).clone()
    }

    /// Texto de `Scrollback` + `Data`, sin secuencias ANSI ni `\r`.
    pub fn output(&self) -> String {
        let bytes: Vec<u8> = lock(&self.chunks)
            .iter()
            .filter_map(|c| match c {
                AgentChunk::Scrollback(b) | AgentChunk::Data(b) => Some(b.as_slice()),
                AgentChunk::Exited(_) => None,
            })
            .flatten()
            .copied()
            .collect();
        strip_ansi(&bytes)
    }

    pub fn exited(&self) -> Option<AgentExit> {
        lock(&self.chunks).iter().find_map(|c| match c {
            AgentChunk::Exited(e) => Some(e.clone()),
            _ => None,
        })
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

impl ChunkSink for CollectingChunkSink {
    fn send(&self, chunk: AgentChunk) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        lock(&self.chunks).push(chunk);
        true
    }
}

/// Acumula rutas reportadas por un watcher.
#[derive(Default)]
pub struct CollectingFsSink {
    paths: Mutex<Vec<RelPath>>,
    ignored: Mutex<Vec<RelPath>>,
}

impl CollectingFsSink {
    /// Todas las rutas recibidas (ignoradas por git incluidas).
    pub fn paths(&self) -> Vec<RelPath> {
        lock(&self.paths).clone()
    }

    /// Solo las que llegaron por `changed_ignored`.
    pub fn ignored(&self) -> Vec<RelPath> {
        lock(&self.ignored).clone()
    }

    pub fn contains(&self, rel: &str) -> bool {
        lock(&self.paths).iter().any(|p| p.as_str() == rel)
    }
}

impl FsChangeSink for CollectingFsSink {
    fn changed(&self, paths: Vec<RelPath>) {
        lock(&self.paths).extend(paths);
    }

    fn changed_ignored(&self, paths: Vec<RelPath>) {
        lock(&self.ignored).extend(paths.iter().cloned());
        lock(&self.paths).extend(paths);
    }
}
