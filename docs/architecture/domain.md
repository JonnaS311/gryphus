# Dominio (`gryphus-core`)

`gryphus-core` contiene las entidades, sus invariantes, los errores y los **puertos** (traits). No hace IO, no depende de `tokio` ni de `tauri`. Todo aquí se prueba con pruebas unitarias puras.

## Entidades

```rust
pub struct Project {
    pub id: ProjectId,          // hash estable de la ruta canónica del repo
    pub root: PathBuf,          // raíz del repositorio principal
    pub features: Vec<Feature>,
}

pub struct Feature {
    pub id: FeatureId,
    pub name: String,           // "login", "pagos"
    pub worktrees: NonEmpty<WorktreeId>,   // invariante: mínimo 1
    pub agents: Vec<AgentId>,
}

pub struct Worktree {
    pub id: WorktreeId,
    pub path: PathBuf,
    pub branch: Option<BranchName>,        // None = HEAD desacoplado
    pub is_main: bool,                     // el worktree principal del repo
}

pub struct Agent {
    pub id: AgentId,
    pub feature: FeatureId,
    pub worktree: WorktreeId,   // debe pertenecer a `feature`
    pub provider: ProviderId,   // "opencode" | "claude" | "codex"
    pub status: AgentStatus,
}

pub enum AgentStatus { Starting, Running, Exited { code: Option<i32> }, Failed { reason: String } }
```

Los IDs son newtypes (`struct FeatureId(Uuid)`) para que el compilador impida mezclarlos.

## Invariantes

Se validan en el dominio, no en la UI ni en los adaptadores:

1. **Una feature tiene al menos un worktree.** Crear una feature exige crear o adjuntar un worktree. No se puede desasociar el último worktree.
2. **Un agente solo trabaja en un worktree de su feature.**
3. **Un worktree pertenece a lo sumo a una feature.** Dos features no comparten worktree (evita que agentes de features distintas se pisen).
4. **Una feature con agentes activos no se puede eliminar.** Primero se detienen los agentes.
5. **El worktree principal** (`is_main`) se puede adjuntar a una feature, pero nunca se elimina desde Gryphus.
6. **El nombre de feature es único dentro del proyecto.**

Las transiciones de estado del agente son explícitas:

```
Starting ──▶ Running ──▶ Exited
    │           │
    └───────────┴──────▶ Failed
Exited/Failed ──restart──▶ Starting
```

## Puertos (traits)

Los puertos son la única forma en que la aplicación toca el mundo exterior. Son síncronos o `async` según el caso, y siempre devuelven `Result<_, PortError>` tipado.

```rust
pub trait GitPort: Send + Sync {
    fn repo_info(&self, root: &Path) -> Result<RepoInfo>;            // branch actual, HEAD, remotos
    fn list_branches(&self, root: &Path) -> Result<Vec<Branch>>;     // locales + remotas, ahead/behind
    fn list_worktrees(&self, root: &Path) -> Result<Vec<WorktreeInfo>>;
    fn add_worktree(&self, root: &Path, req: NewWorktree) -> Result<WorktreeInfo>;
    fn remove_worktree(&self, root: &Path, path: &Path, force: bool) -> Result<()>;
    fn status(&self, worktree: &Path) -> Result<Vec<FileStatus>>;
    fn diff(&self, worktree: &Path, base: DiffBase) -> Result<Diff>; // vs HEAD o vs merge-base
}

pub trait AgentRuntimePort: Send + Sync {
    fn spawn(&self, spec: SpawnSpec) -> Result<AgentHandle>;         // cmd, args, env, cwd, tamaño
    fn write(&self, id: AgentId, bytes: &[u8]) -> Result<()>;
    fn resize(&self, id: AgentId, cols: u16, rows: u16) -> Result<()>;
    fn kill(&self, id: AgentId) -> Result<()>;
    fn attach(&self, id: AgentId) -> Result<OutputStream>;           // scrollback + en vivo
}

pub trait FsPort: Send + Sync {
    fn list_dir(&self, dir: &Path) -> Result<Vec<DirEntry>>;         // un nivel, respeta .gitignore
    fn read_file(&self, path: &Path) -> Result<FileContent>;          // texto o binario detectado
    fn write_file(&self, path: &Path, content: &[u8]) -> Result<()>;  // escritura atómica
    fn watch(&self, root: &Path) -> Result<WatchHandle>;              // emite cambios con debounce
}

pub trait StorePort: Send + Sync {
    fn load_project(&self, id: &ProjectId) -> Result<Option<ProjectState>>;
    fn save_project(&self, state: &ProjectState) -> Result<()>;
    fn load_config(&self) -> Result<Config>;                          // proveedores de agentes, UI
}

pub trait EventSink: Send + Sync {
    fn publish(&self, event: DomainEvent);
}
```

## Eventos de dominio

```rust
pub enum DomainEvent {
    FeatureChanged(FeatureId),
    AgentStatusChanged { agent: AgentId, status: AgentStatus },
    FilesChanged { worktree: WorktreeId, paths: Vec<PathBuf> },
    GitChanged { worktree: WorktreeId },   // la UI vuelve a pedir status/branches
}
```

Los eventos son **notificaciones, no datos**: la UI vuelve a pedir lo que necesita. Esto mantiene una sola fuente de verdad y evita sincronizar estado duplicado.

## Errores

Un `enum DomainError` con variantes concretas (`FeatureNotFound`, `WorktreeNotInFeature`, `LastWorktree`, `AgentsStillRunning`, `ProviderNotConfigured`, …). En la frontera IPC se serializan como `{ code, message }` para que la UI pueda reaccionar por `code`.
