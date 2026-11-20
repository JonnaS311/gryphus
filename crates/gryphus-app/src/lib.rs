//! Casos de uso de Gryphus.
//!
//! [`ProjectSession`] es el estado vivo de **un** proyecto (una ventana). Orquesta
//! los puertos de `gryphus-core` y publica `DomainEvent`s. Reglas:
//! - Nunca se mantiene el lock del proyecto durante una llamada a un puerto.
//! - Cada mutación del proyecto se persiste antes de publicar su evento.
//! - Si un paso con efectos externos falla a mitad, se deshace lo hecho.
//!
//! Ver docs/architecture/overview.md.
#![forbid(unsafe_code)]

mod error;
mod session;
mod watch;

pub use error::{AppError, AppResult};
pub use session::{GitOverview, Ports, ProjectSession, WorktreeChoice};
