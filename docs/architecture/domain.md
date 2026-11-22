# Dominio (`gryphus-core`)

`gryphus-core` contiene las entidades, sus invariantes, los errores y los **puertos** (traits). No hace IO y no depende de `tokio` ni de `tauri`; sus únicas dependencias son `serde` y `thiserror`. Todo se prueba con pruebas unitarias y de propiedades.

```
crates/gryphus-core/src/
├─ ids.rs        FeatureId, WorktreeId, AgentId (secuenciales), ProjectId (hash de la ruta)
├─ names.rs      FeatureName, BranchName (validados al construirse)
├─ path.rs       RelPath (ruta relativa que no puede salir del worktree)
├─ agent.rs      ProviderId, AgentStatus (máquina de estados), Agent
├─ feature.rs    Feature
├─ worktree.rs   Worktree
├─ project.rs    Project: raíz del agregado, hace cumplir las invariantes
├─ config.rs     Config, ProviderConfig, ruta por defecto de worktrees
├─ error.rs      DomainError con code() estable
├─ events.rs     DomainEvent, EventSink
└─ ports/        GitPort, AgentRuntimePort, FsPort, StorePort, PortError, Subscription
```

## Tipos validados

Si un valor existe, es válido: la validación ocurre al construirlo (`parse`) y también al deserializar (`serde(try_from)`).

| Tipo | Reglas |
|---|---|
| `FeatureName` | ASCII alfanumérico, `-`, `_`, `.`; empieza por letra o número; ≤ 64; sin `..`, sin terminar en `.` o `.lock`; no es un nombre reservado de Windows (`con`, `nul`, `com1`…). Se usa como carpeta y como branch. |
| `BranchName` | Subconjunto conservador de `git check-ref-format`. `BranchName::for_feature(name)` = `feat/<name>`, siempre válido (verificado por propiedades). |
| `RelPath` | Ruta relativa al worktree: sin `..`, sin rutas absolutas, sin `C:` ni streams NTFS (`a:b`), separadores normalizados a `/`. Toda ruta que venga de la UI pasa por aquí. Los symlinks que apuntan fuera los bloquea `gryphus-fs`. |

## Entidades

`Project` es la **raíz del agregado**: los campos de `Feature`, `Worktree` y `Agent` son privados al crate y solo `Project` los modifica. Fuera del crate solo hay getters.

```rust
pub struct Project  { id: ProjectId, root: PathBuf, features: Vec<Feature>, worktrees: Vec<Worktree>, agents: Vec<Agent>, next_id: u64 }
pub struct Feature  { id: FeatureId, name: FeatureName, worktrees: Vec<WorktreeId> /* ≥ 1 */ }
pub struct Worktree { id: WorktreeId, path: PathBuf, branch: Option<BranchName>, is_main: bool, missing: bool }
pub struct Agent    { id: AgentId, feature: FeatureId, worktree: WorktreeId, provider: ProviderId, status: AgentStatus }
```

- **Ids secuenciales** asignados por `Project` (deterministas, sin aleatoriedad). `ProjectId` es FNV-1a de la ruta canónica, estable entre versiones de Rust.
- **Los agentes de una feature se derivan** (`project.agents_of(feature)`) en vez de guardarse en `Feature`. Así hay una sola fuente de verdad.
- **Registro de worktrees:** git es la fuente de verdad. `sync_worktrees(&[WorktreeInfo])` registra los nuevos, actualiza branch y principal, y marca `missing` los que desaparecieron (sin romper la feature que los usa).

## API de `Project`

| Operación | Errores posibles |
|---|---|
| `sync_worktrees(infos) -> bool` | — (devuelve si algo cambió) |
| `forget_worktree(wt)` | `WORKTREE_NOT_FOUND`, `MAIN_WORKTREE`, `WORKTREE_ALREADY_ASSIGNED` |
| `create_feature(name, wt)` | `DUPLICATE_FEATURE_NAME`, `WORKTREE_NOT_FOUND`, `WORKTREE_MISSING`, `WORKTREE_ALREADY_ASSIGNED` |
| `attach_worktree(f, wt)` (idempotente) | `FEATURE_NOT_FOUND`, `WORKTREE_*` |
| `detach_worktree(f, wt)` | `WORKTREE_NOT_IN_FEATURE`, `LAST_WORKTREE`, `WORKTREE_IN_USE` |
| `delete_feature(f) -> Feature` | `AGENTS_STILL_RUNNING` (elimina también sus agentes inactivos) |
| `add_agent(f, wt, provider)` → `Starting` | `WORKTREE_NOT_IN_FEATURE`, `WORKTREE_MISSING` |
| `set_agent_status(a, status)` | `INVALID_TRANSITION` |
| `remove_agent(a)` | `AGENT_STILL_RUNNING` |
| `set_agent_session(a, session)` | `AGENT_NOT_FOUND` (conversación del CLI, ver [agents.md](agents.md#reanudar-conversaciones)) |
| `mark_active_for_reopen() -> Vec<AgentId>` | — (al cerrar: los activos se relanzarán al reabrir) |
| `reset_after_restart() -> Vec<AgentId>` | — (activos → `Exited { code: None }`, ADR-013; devuelve los que hay que relanzar: activos o marcados, y quita la marca) |
| `check_invariants()` | `INVARIANT_VIOLATION` |

Garantía: **una operación que devuelve error no modifica el proyecto** (verificado por propiedades).

## Invariantes

1. Una feature tiene al menos un worktree.
2. Un agente solo trabaja en un worktree de su feature.
3. Un worktree pertenece a lo sumo a una feature.
4. Una feature con agentes activos no se puede eliminar.
5. El worktree principal nunca se olvida ni se elimina desde Gryphus (sí puede respaldar una feature).
6. El nombre de feature es único, sin distinguir mayúsculas.
7. No se puede desasociar de su feature un worktree que usa un agente.

Además, `check_invariants` verifica que los ids sean únicos y menores que `next_id` y que las rutas de worktree sean únicas. `gryphus-store` debe llamarla tras deserializar.

## Máquina de estados del agente

```
Starting ──▶ Running ──▶ Exited
    │  │        │
    │  └────────┴──────▶ Failed
    └──────────────────▶ Exited      (el proceso terminó al instante)
Exited/Failed ──restart──▶ Starting
```

`Starting` y `Running` son **activos**.

## Puertos

Son **síncronos y sin runtime async**: `gryphus-app` decide en qué hilo llamarlos (`spawn_blocking`). Los flujos se entregan por callbacks (`ChunkSink`, `FsChangeSink`) y se cancelan **soltando** una `Subscription`. Todos devuelven `PortResult<T>` con `PortError { kind, message }`, y `kind` tiene un `code()` estable (`NOT_FOUND`, `CONFLICT`, `UNAVAILABLE`…).

```rust
pub trait GitPort: Send + Sync {
    fn repo_info(&self, root: &Path) -> PortResult<RepoInfo>;
    fn list_branches(&self, root: &Path) -> PortResult<Vec<Branch>>;
    fn list_worktrees(&self, root: &Path) -> PortResult<Vec<WorktreeInfo>>;
    fn prune_worktrees(&self, root: &Path) -> PortResult<()>;
    fn add_worktree(&self, root: &Path, req: &NewWorktree) -> PortResult<WorktreeInfo>;
    fn remove_worktree(&self, root: &Path, worktree: &Path, force: bool) -> PortResult<()>;
    fn status(&self, worktree: &Path) -> PortResult<Vec<FileStatus>>;
    fn diff(&self, worktree: &Path, base: &DiffBase) -> PortResult<Diff>;
}

pub trait AgentRuntimePort: Send + Sync {
    fn spawn(&self, spec: SpawnSpec, on_exit: ExitCallback) -> PortResult<()>;
    fn write(&self, agent: AgentId, bytes: &[u8]) -> PortResult<()>;
    fn resize(&self, agent: AgentId, size: TermSize) -> PortResult<()>;
    fn kill(&self, agent: AgentId) -> PortResult<()>;
    fn attach(&self, agent: AgentId, sink: Arc<dyn ChunkSink>) -> PortResult<Subscription>;
}

pub trait FsPort: Send + Sync {
    fn list_dir(&self, root: &Path, dir: &RelPath) -> PortResult<Vec<DirEntry>>;
    fn read_file(&self, root: &Path, file: &RelPath) -> PortResult<FileContent>;
    fn write_file(&self, root: &Path, file: &RelPath, content: &[u8]) -> PortResult<()>;
    fn watch(&self, root: &Path, sink: Arc<dyn FsChangeSink>) -> PortResult<Subscription>;
}

pub trait StorePort: Send + Sync {
    fn load_project(&self, id: &ProjectId) -> PortResult<Option<Project>>;
    fn save_project(&self, project: &Project) -> PortResult<()>;
    fn load_config(&self) -> PortResult<Config>;
}

pub trait EventSink: Send + Sync {
    fn publish(&self, event: DomainEvent);
}
```

`SpawnSpec::new(agent, &ProviderConfig, cwd, size)` construye el lanzamiento desde la configuración. `AgentExit` se convierte en `AgentStatus` (`Code` → `Exited`, `Failed` → `Failed`).

## Eventos

```rust
pub enum DomainEvent {
    FeatureChanged(FeatureId),
    FeatureRemoved(FeatureId),
    AgentStatusChanged { agent: AgentId, status: AgentStatus },
    FilesChanged { worktree: WorktreeId, paths: Vec<RelPath> },
    GitChanged { worktree: WorktreeId },
    WorktreesChanged,
}
```

Son **notificaciones, no datos** (ADR-008): la UI vuelve a pedir lo que necesita.

## Configuración

`Config::default()` trae los tres proveedores con su comando por defecto (`opencode`, `claude`, `codex`). `config.provider(id)` falla con `PROVIDER_NOT_CONFIGURED` si falta o el comando está vacío. `config.worktree_path(repo_root, feature)` aplica ADR-012: `<padre>/<repo>.worktrees/<feature>`, o `<worktrees_dir>/<repo>/<feature>` si está configurado.

`gryphus-store` combina `config.toml` con los valores por defecto: un proveedor que el usuario no declara conserva su comando por defecto. Un proveedor desconocido o un campo desconocido es un error `INVALID_INPUT` con el motivo.

## Pruebas

| Archivo | Qué cubre |
|---|---|
| `src/*.rs` (`#[cfg(test)]`) | Reglas de nombres, rutas, ids, máquina de estados, códigos de error, configuración |
| `tests/project.rs` | Comportamiento de `Project` solo por su API pública (15 casos) |
| `tests/invariants.rs` | Propiedades: 512 secuencias aleatorias de hasta 60 operaciones. Tras cada una se cumplen las invariantes, un error no cambia el estado y el resultado sobrevive a JSON |

La prueba de propiedades se validó rompiendo a propósito la regla del último worktree: falló y redujo el caso a una secuencia mínima.
