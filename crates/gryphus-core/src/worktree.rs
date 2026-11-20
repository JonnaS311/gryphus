//! Worktree registrado en el proyecto. La fuente de verdad es git; el proyecto
//! guarda un registro reconciliado con [`crate::Project::sync_worktrees`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ids::WorktreeId;
use crate::names::BranchName;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub(crate) id: WorktreeId,
    pub(crate) path: PathBuf,
    /// `None` = HEAD desacoplado.
    pub(crate) branch: Option<BranchName>,
    /// Worktree principal del repo: nunca se elimina desde Gryphus.
    pub(crate) is_main: bool,
    /// Ya no aparece en `git worktree list` (borrado fuera de Gryphus).
    pub(crate) missing: bool,
}

impl Worktree {
    pub fn id(&self) -> WorktreeId {
        self.id
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn branch(&self) -> Option<&BranchName> {
        self.branch.as_ref()
    }
    pub fn is_main(&self) -> bool {
        self.is_main
    }
    pub fn is_missing(&self) -> bool {
        self.missing
    }
}
