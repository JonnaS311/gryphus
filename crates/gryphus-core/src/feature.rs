//! Feature: unidad de trabajo del panel izquierdo. Agrupa worktrees (≥ 1) y,
//! a través de ellos, agentes. Los agentes de una feature se derivan de
//! [`crate::Project::agents_of`] para no duplicar estado.

use serde::{Deserialize, Serialize};

use crate::ids::{FeatureId, WorktreeId};
use crate::names::FeatureName;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub(crate) id: FeatureId,
    pub(crate) name: FeatureName,
    /// Invariante: nunca vacío (lo garantiza `Project`).
    pub(crate) worktrees: Vec<WorktreeId>,
}

impl Feature {
    pub fn id(&self) -> FeatureId {
        self.id
    }
    pub fn name(&self) -> &FeatureName {
        &self.name
    }
    pub fn worktrees(&self) -> &[WorktreeId] {
        &self.worktrees
    }
    pub fn has_worktree(&self, worktree: WorktreeId) -> bool {
        self.worktrees.contains(&worktree)
    }
}
