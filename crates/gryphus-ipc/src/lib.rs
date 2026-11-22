//! Contrato IPC entre `src-tauri` y la UI (Leptos/WASM). Ver docs/architecture/ipc.md.
//!
//! - Todos los comandos reciben un único argumento `req` y devuelven
//!   `Result<T, IpcError>`.
//! - Los ids viajan como `u64`; las rutas como `String` relativas al worktree.
//! - La salida de los agentes no usa JSON: ver [`stream`].
//!
//! No depende de `gryphus-core`: `src-tauri` traduce dominio ⇄ DTO (ADR-011).
#![forbid(unsafe_code)]

pub mod stream;

use serde::{Deserialize, Serialize};

/// Nombres de comandos (`invoke`).
pub mod cmd {
    pub const PICK_FOLDER: &str = "pick_folder";
    pub const OPEN_PROJECT: &str = "open_project";
    pub const GET_PROJECT: &str = "get_project";
    /// Cierra el proyecto de la ventana (detiene sus agentes y guarda); la ventana vuelve a la bienvenida.
    pub const CLOSE_PROJECT: &str = "close_project";
    /// Al arrancar, reabre en la ventana principal el último proyecto abierto.
    /// Devuelve `true` si lo abrió; solo actúa una vez por proceso.
    pub const RESTORE_LAST_PROJECT: &str = "restore_last_project";
    pub const CREATE_FEATURE: &str = "create_feature";
    pub const DELETE_FEATURE: &str = "delete_feature";
    pub const SPAWN_AGENT: &str = "spawn_agent";
    pub const STOP_AGENT: &str = "stop_agent";
    pub const RESTART_AGENT: &str = "restart_agent";
    pub const REMOVE_AGENT: &str = "remove_agent";
    pub const RENAME_AGENT: &str = "rename_agent";
    pub const AGENT_INPUT: &str = "agent_input";
    pub const AGENT_RESIZE: &str = "agent_resize";
    pub const ATTACH_AGENT: &str = "attach_agent";
    pub const DETACH_AGENT: &str = "detach_agent";
    pub const LIST_DIR: &str = "list_dir";
    pub const READ_FILE: &str = "read_file";
    pub const WRITE_FILE: &str = "write_file";
    /// Devuelve la ruta nueva.
    pub const RENAME_PATH: &str = "rename_path";
    /// Pega una copia; devuelve la ruta creada.
    pub const COPY_PATH: &str = "copy_path";
    pub const GIT_STATUS: &str = "git_status";
    pub const GIT_DIFF: &str = "git_diff";
    pub const GIT_OVERVIEW: &str = "git_overview";
    /// La UI lo llama al montarse; devuelve ms desde el arranque del proceso.
    pub const UI_READY: &str = "ui_ready";
    /// Con la guarda activa, cerrar la ventana no la cierra: emite
    /// [`super::event::CLOSE_REQUESTED`] para que la UI pregunte. Req: [`super::CloseGuardReq`].
    pub const SET_CLOSE_GUARD: &str = "set_close_guard";
    /// La UI confirmó el cierre: destruye la ventana (detiene agentes y guarda).
    pub const CONFIRM_CLOSE: &str = "confirm_close";
}

/// Nombres de eventos (Rust → ventana del proyecto). Son notificaciones: la UI
/// vuelve a pedir lo que necesite.
pub mod event {
    /// Payload: [`super::FeatureEvent`]
    pub const FEATURE_CHANGED: &str = "feature:changed";
    /// Payload: [`super::FeatureEvent`]
    pub const FEATURE_REMOVED: &str = "feature:removed";
    /// Payload: [`super::AgentStatusEvent`]
    pub const AGENT_STATUS: &str = "agent:status";
    /// El agente terminó su tarea y espera instrucciones. Payload: [super::AgentEvent]
    pub const AGENT_IDLE: &str = "agent:idle";
    /// Payload: [`super::FilesChangedEvent`]
    pub const FILES_CHANGED: &str = "fs:changed";
    /// Payload: [`super::WorktreeEvent`]
    pub const GIT_CHANGED: &str = "git:changed";
    /// Payload: `()`
    pub const WORKTREES_CHANGED: &str = "worktrees:changed";
    /// Se pidió cerrar la ventana con la guarda activa. Payload: `()`
    pub const CLOSE_REQUESTED: &str = "window:close-requested";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcError {
    /// Código estable (`FEATURE_NOT_FOUND`, `CONFLICT`, …).
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

// ── Proyecto ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDto {
    pub id: String,
    pub root: String,
    pub name: String,
    pub features: Vec<FeatureDto>,
    pub worktrees: Vec<WorktreeDto>,
    pub agents: Vec<AgentDto>,
    pub providers: Vec<ProviderDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureDto {
    pub id: u64,
    pub name: String,
    pub worktrees: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeDto {
    pub id: u64,
    pub path: String,
    pub branch: Option<String>,
    pub is_main: bool,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDto {
    pub id: u64,
    /// Nombre visible (editable).
    pub name: String,
    pub feature: u64,
    pub worktree: u64,
    pub provider: String,
    pub status: AgentStatusDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum AgentStatusDto {
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { reason: String },
}

impl AgentStatusDto {
    pub fn is_active(&self) -> bool {
        matches!(self, AgentStatusDto::Starting | AgentStatusDto::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDto {
    /// `opencode` | `claude` | `codex`
    pub id: String,
    pub command: String,
    /// El comando existe en el PATH.
    pub available: bool,
}

// ── Peticiones ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenProjectReq {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenProjectRes {
    /// `true` si se abrió en otra ventana (o se enfocó una existente).
    pub other_window: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum WorktreeChoiceDto {
    New { base: Option<String> },
    Existing { id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFeatureReq {
    pub name: String,
    pub worktree: WorktreeChoiceDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFeatureReq {
    pub feature: u64,
    pub remove_worktrees: bool,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnAgentReq {
    pub feature: u64,
    pub worktree: u64,
    pub provider: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReq {
    pub agent: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInputReq {
    pub agent: u64,
    pub data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResizeReq {
    pub agent: u64,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathReq {
    pub worktree: u64,
    /// Relativa al worktree; `""` es la raíz.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameAgentReq {
    pub agent: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamePathReq {
    pub worktree: u64,
    pub path: String,
    /// Solo el nombre nuevo (un componente), no una ruta.
    pub new_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyPathReq {
    pub from_worktree: u64,
    pub from: String,
    pub to_worktree: u64,
    /// Directorio destino; "" es la raíz.
    pub to_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFileReq {
    pub worktree: u64,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseGuardReq {
    /// `true` si cerrar perdería algo (agentes activos, cambios sin guardar).
    pub guard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeReq {
    pub worktree: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffReq {
    pub worktree: u64,
    /// `None` = cambios sin commit; `Some(branch)` = desde el merge-base con esa branch.
    pub base: Option<String>,
}

// ── Archivos ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKindDto {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntryDto {
    pub name: String,
    pub kind: EntryKindDto,
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileContentDto {
    Text { text: String },
    Binary { size: u64 },
    TooLarge { size: u64, limit: u64 },
}

// ── Git ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStateDto {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

impl FileStateDto {
    /// Letra para el explorador (como `git status --short`).
    pub fn letter(self) -> char {
        match self {
            FileStateDto::Added => 'A',
            FileStateDto::Modified => 'M',
            FileStateDto::Deleted => 'D',
            FileStateDto::Renamed => 'R',
            FileStateDto::Untracked => 'U',
            FileStateDto::Conflicted => '!',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStatusDto {
    pub path: String,
    pub state: FileStateDto,
    pub from: Option<String>,
    pub staged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKindDto {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLineDto {
    pub kind: LineKindDto,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkDto {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLineDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiffDto {
    pub path: String,
    pub binary: bool,
    pub hunks: Vec<HunkDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffDto {
    pub files: Vec<FileDiffDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchDto {
    pub name: String,
    pub is_remote: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub checked_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitOverviewDto {
    pub head: Option<String>,
    pub branches: Vec<BranchDto>,
}

// ── Payloads de eventos ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureEvent {
    pub feature: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatusEvent {
    pub agent: u64,
    pub status: AgentStatusDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub agent: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesChangedEvent {
    pub worktree: u64,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeEvent {
    pub worktree: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_enums_have_stable_json() {
        let choice = WorktreeChoiceDto::New { base: None };
        assert_eq!(
            serde_json::to_string(&choice).unwrap(),
            r#"{"kind":"new","base":null}"#
        );
        let status = AgentStatusDto::Exited { code: Some(1) };
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"state":"exited","code":1}"#
        );
        let content = FileContentDto::TooLarge { size: 2, limit: 1 };
        assert_eq!(
            serde_json::to_string(&content).unwrap(),
            r#"{"kind":"too_large","size":2,"limit":1}"#
        );
    }

    #[test]
    fn command_names_are_unique() {
        let names = [
            cmd::PICK_FOLDER,
            cmd::OPEN_PROJECT,
            cmd::GET_PROJECT,
            cmd::CLOSE_PROJECT,
            cmd::RESTORE_LAST_PROJECT,
            cmd::CREATE_FEATURE,
            cmd::DELETE_FEATURE,
            cmd::SPAWN_AGENT,
            cmd::STOP_AGENT,
            cmd::RESTART_AGENT,
            cmd::REMOVE_AGENT,
            cmd::RENAME_AGENT,
            cmd::AGENT_INPUT,
            cmd::AGENT_RESIZE,
            cmd::ATTACH_AGENT,
            cmd::DETACH_AGENT,
            cmd::LIST_DIR,
            cmd::READ_FILE,
            cmd::WRITE_FILE,
            cmd::RENAME_PATH,
            cmd::COPY_PATH,
            cmd::GIT_STATUS,
            cmd::GIT_DIFF,
            cmd::GIT_OVERVIEW,
            cmd::UI_READY,
        ];
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }
}
