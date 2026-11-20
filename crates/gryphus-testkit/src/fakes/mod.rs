//! Implementaciones en memoria de los puertos. Cumplen las mismas suites de
//! contrato que los adaptadores reales (ver `tests/fakes_contracts.rs`).
//!
//! Todas aceptan `fail_next(err)` para simular el fallo de la próxima llamada.

mod agents;
mod fs;
mod git;
mod store;

pub use agents::{FakeAgentRuntime, FAKE_AGENT_PROGRAM, SCROLLBACK_LIMIT};
pub use fs::{FakeFs, DEFAULT_SIZE_LIMIT};
pub use git::FakeGit;
pub use store::FakeStore;

use std::sync::{Mutex, MutexGuard};

use gryphus_core::ports::{PortError, PortErrorKind, PortResult};

/// `lock()` que ignora el envenenamiento: en pruebas un pánico ya hizo fallar el test.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn err<T>(kind: PortErrorKind, message: impl Into<String>) -> PortResult<T> {
    Err(PortError::new(kind, message))
}

/// Error programado para la próxima llamada a cualquier método del puerto.
#[derive(Default)]
pub(crate) struct FailSwitch(Mutex<Option<PortError>>);

impl FailSwitch {
    pub(crate) fn set(&self, error: PortError) {
        *lock(&self.0) = Some(error);
    }

    pub(crate) fn check(&self) -> PortResult<()> {
        match lock(&self.0).take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
