//! Kit de pruebas de Gryphus.
//!
//! - [`fakes`]: implementaciones en memoria de cada puerto, deterministas y con
//!   ganchos para simular eventos y fallos. Las usa `gryphus-app` en sus pruebas.
//! - [`contracts`]: suites de comportamiento que deben cumplir **el fake y el
//!   adaptador real** de cada puerto. Así el fake no puede divergir del real.
//! - [`TempRepo`]: repositorio git real en un directorio temporal.
//! - [`agent_script`]: lenguaje de comandos de `fake-agent` (compartido con
//!   [`fakes::FakeAgentRuntime`] para que ambos se comporten igual).
//!
//! Ver docs/testing.md.
#![forbid(unsafe_code)]

pub mod agent_script;
pub mod contracts;
pub mod fakes;
pub mod repo;
pub mod sinks;
pub mod wait;

pub use contracts::{AgentHarness, FsHarness, GitHarness, StoreHarness};
pub use fakes::{FakeAgentRuntime, FakeFs, FakeGit, FakeStore};
pub use repo::TempRepo;
pub use sinks::{CollectingChunkSink, CollectingFsSink, RecordingEventSink};
pub use wait::{strip_ansi, wait_until};
