//! [`FakeStore`]: guarda los proyectos como JSON en memoria, igual que lo hará
//! `gryphus-store` en disco, para que la (de)serialización también se ejercite.

use std::collections::HashMap;
use std::sync::Mutex;

use gryphus_core::ports::{PortError, PortErrorKind, PortResult, StorePort};
use gryphus_core::{Config, Project, ProjectId};

use super::{lock, FailSwitch};

#[derive(Default)]
pub struct FakeStore {
    projects: Mutex<HashMap<ProjectId, String>>,
    config: Mutex<Option<Config>>,
    saves: Mutex<usize>,
    fail: FailSwitch,
}

impl FakeStore {
    pub fn with_config(config: Config) -> Self {
        let store = Self::default();
        *lock(&store.config) = Some(config);
        store
    }

    pub fn fail_next(&self, error: PortError) {
        self.fail.set(error);
    }

    /// Escribe el contenido tal cual (para simular archivos corruptos).
    pub fn write_raw_project(&self, id: &ProjectId, contents: &str) {
        lock(&self.projects).insert(id.clone(), contents.to_owned());
    }

    /// Número de veces que se llamó a `save_project` con éxito.
    pub fn save_count(&self) -> usize {
        *lock(&self.saves)
    }
}

fn corrupted(message: impl std::fmt::Display) -> PortError {
    PortError::new(PortErrorKind::Corrupted, message.to_string())
}

impl StorePort for FakeStore {
    fn load_project(&self, id: &ProjectId) -> PortResult<Option<Project>> {
        self.fail.check()?;
        let Some(raw) = lock(&self.projects).get(id).cloned() else {
            return Ok(None);
        };
        let project: Project = serde_json::from_str(&raw).map_err(corrupted)?;
        project.check_invariants().map_err(corrupted)?;
        Ok(Some(project))
    }

    fn save_project(&self, project: &Project) -> PortResult<()> {
        self.fail.check()?;
        let raw = serde_json::to_string_pretty(project)
            .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;
        lock(&self.projects).insert(project.id().clone(), raw);
        *lock(&self.saves) += 1;
        Ok(())
    }

    fn load_config(&self) -> PortResult<Config> {
        self.fail.check()?;
        Ok(lock(&self.config).clone().unwrap_or_default())
    }
}
