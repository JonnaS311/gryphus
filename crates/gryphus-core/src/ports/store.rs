//! Puerto de persistencia. Implementación real en `gryphus-store` (ADR-009).

use crate::config::Config;
use crate::ids::ProjectId;
use crate::project::Project;

use super::PortResult;

pub trait StorePort: Send + Sync {
    /// El adaptador debe llamar a [`Project::check_invariants`] tras deserializar
    /// y devolver error si el archivo está corrupto.
    fn load_project(&self, id: &ProjectId) -> PortResult<Option<Project>>;
    fn save_project(&self, project: &Project) -> PortResult<()>;
    /// Si no existe el archivo, devuelve [`Config::default`].
    fn load_config(&self) -> PortResult<Config>;
}
