//! Dominio de Gryphus: entidades, invariantes, errores y puertos (traits). Sin IO.
//!
//! - [`Project`] es la raíz del agregado: toda mutación de features, worktrees y
//!   agentes pasa por él, y es quien hace cumplir las invariantes.
//! - [`ports`] define lo que la aplicación necesita del exterior; los adaptadores
//!   (`gryphus-git`, `gryphus-agents`, `gryphus-fs`, `gryphus-store`) lo implementan.
//!
//! Ver docs/architecture/domain.md.
#![forbid(unsafe_code)]

pub mod agent;
pub mod config;
pub mod error;
pub mod events;
pub mod feature;
pub mod ids;
pub mod names;
pub mod path;
pub mod ports;
pub mod project;
pub mod worktree;

pub use agent::{is_valid_session_id, Agent, AgentSession, AgentStatus, ProviderId, SessionLaunch};
pub use config::{Config, ProviderConfig};
pub use error::{DomainError, DomainResult};
pub use events::{DomainEvent, EventSink};
pub use feature::Feature;
pub use ids::{AgentId, FeatureId, ProjectId, WorktreeId};
pub use names::{AgentName, BranchName, FeatureName, NameError};
pub use path::{PathError, RelPath};
pub use project::Project;
pub use worktree::Worktree;
