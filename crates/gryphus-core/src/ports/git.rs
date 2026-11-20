//! Puerto git. Implementación real en `gryphus-git` (docs/architecture/git.md).

use std::path::{Path, PathBuf};

use crate::names::BranchName;
use crate::path::RelPath;

use super::PortResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    /// Raíz canónica del worktree principal.
    pub root: PathBuf,
    /// Branch actual del worktree principal; `None` si HEAD está desacoplado.
    pub head: Option<BranchName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: BranchName,
    pub is_remote: bool,
    /// Rama remota a la que sigue, p. ej. `origin/main`.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Es la branch actual de algún worktree.
    pub checked_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<BranchName>,
    /// Oid del commit en HEAD (hex).
    pub head: Option<String>,
    pub is_main: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorktree {
    pub path: PathBuf,
    pub branch: BranchName,
    /// Crear `branch` nueva desde `base` (si es `false`, `branch` debe existir).
    pub create_branch: bool,
    /// Referencia de partida (branch, tag u oid). `None` = HEAD actual.
    pub base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Added,
    Modified,
    Deleted,
    Renamed { from: RelPath },
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    pub path: RelPath,
    pub state: FileState,
    /// El cambio está en el índice (staged).
    pub staged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffBase {
    /// Cambios sin commit del worktree.
    Head,
    /// Todo lo hecho en el worktree desde el merge-base con esta branch.
    MergeBase(BranchName),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub files: Vec<FileDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: RelPath,
    pub old_path: Option<RelPath>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

pub trait GitPort: Send + Sync {
    fn repo_info(&self, root: &Path) -> PortResult<RepoInfo>;
    /// Locales y remotas, con ahead/behind respecto a su upstream.
    fn list_branches(&self, root: &Path) -> PortResult<Vec<Branch>>;
    /// Incluye el worktree principal.
    fn list_worktrees(&self, root: &Path) -> PortResult<Vec<WorktreeInfo>>;
    fn add_worktree(&self, root: &Path, req: &NewWorktree) -> PortResult<WorktreeInfo>;
    /// `force` permite borrar con cambios sin commit. Nunca borra la branch.
    fn remove_worktree(&self, root: &Path, worktree: &Path, force: bool) -> PortResult<()>;
    fn status(&self, worktree: &Path) -> PortResult<Vec<FileStatus>>;
    fn diff(&self, worktree: &Path, base: &DiffBase) -> PortResult<Diff>;
}
