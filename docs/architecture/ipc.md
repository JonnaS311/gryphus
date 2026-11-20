# Contrato IPC (UI ⇄ Rust)

`src-tauri` es un adaptador **delgado**: cada comando valida la entrada, resuelve la sesión del proyecto y llama a un caso de uso. No contiene reglas de negocio.

## Ventana ↔ proyecto

- `WindowRegistry`: `HashMap<WindowLabel, Arc<ProjectSession>>`.
- `open_project(path)`: canonicaliza la ruta, calcula el `ProjectId`. Si ya hay una ventana para ese proyecto, la enfoca; si no, crea la ventana `project-<id>` y una `ProjectSession` nueva.
- Ningún comando recibe `projectId`: el proyecto se deduce **siempre** de la ventana que invoca (`tauri::Window`). Así una ventana no puede, por diseño, actuar sobre otro proyecto.
- Al cerrar la ventana: se preguntan los agentes activos, se detienen, se guarda el estado y se libera la sesión.

## Tipos compartidos

Los tipos de entrada/salida viven en el crate **`gryphus-ipc`** (solo `serde`) y los compilan tanto `src-tauri` como la UI en Leptos (WASM). No hay generación de código: si un comando cambia, el backend y la UI fallan al compilar a la vez ([ADR-011](../decisions.md#adr-011--crate-gryphus-ipc-de-tipos-compartidos)).

- Los nombres de comandos y eventos son constantes en `gryphus-ipc` (`pub const SPAWN_AGENT: &str = "spawn_agent";`); ni la UI ni el backend escriben esos strings a mano.
- `src-tauri` traduce entre los tipos del dominio y los DTOs de `gryphus-ipc` (`impl From<Feature> for FeatureDto`). Así el dominio puede cambiar sin tocar el contrato.
- En la UI, `ui/src/bridge/tauri.rs` envuelve `invoke`, `listen` y `Channel` (vía `window.__TAURI__`, con `withGlobalTauri: true`) y deserializa con `serde-wasm-bindgen`.

## Comandos (UI → Rust)

| Comando | Entrada | Salida |
|---|---|---|
| `open_project` | `path` | `ProjectSummary` |
| `get_project` | — | `ProjectView` (features, worktrees, agentes) |
| `create_feature` | `name`, `worktree: New{branch, base} \| Existing{id}` | `Feature` |
| `delete_feature` | `featureId`, `removeWorktrees: bool` | — |
| `attach_worktree` / `detach_worktree` | `featureId`, `worktreeId` | `Feature` |
| `spawn_agent` | `featureId`, `worktreeId`, `provider` | `Agent` |
| `stop_agent` / `restart_agent` | `agentId` | — |
| `agent_input` | `agentId`, `data: string` | — |
| `agent_resize` | `agentId`, `cols`, `rows` | — |
| `attach_agent` | `agentId`, `channel: Channel<AgentChunk>` | — |
| `list_dir` | `worktreeId`, `relPath` | `DirEntry[]` |
| `read_file` / `write_file` | `worktreeId`, `relPath`, (`content`) | `FileContent` / — |
| `git_overview` | — | branches, worktrees, ahead/behind |
| `git_status` | `worktreeId` | `FileStatus[]` |
| `git_diff` | `worktreeId`, `base: Head \| MergeBase{branch}` | `Diff` |

Las rutas de archivo siempre son **relativas al worktree** y se validan en Rust (no se permite salir del worktree con `..` ni con symlinks).

## Channels (Rust → UI, alto volumen)

`attach_agent` recibe un `Channel<AgentChunk>`:

```rust
enum AgentChunk {
    Scrollback(Vec<u8>),   // una vez, al conectar: contenido del ring buffer
    Data(Vec<u8>),         // lotes de salida en vivo (coalescidos ~16 ms o 64 KiB)
    Exited(Option<i32>),
}
```

- **Coalescencia:** la tarea lectora agrupa la salida PTY y envía como máximo un mensaje por frame. Evita miles de mensajes IPC por segundo.
- **Contrapresión:** si la UI no consume, el buffer intermedio se recorta (se conserva el ring buffer de scrollback); el proceso del agente nunca se bloquea por la UI.
- Cambiar de agente en la vista central suelta el Channel anterior; volver a él re-adjunta y recibe el scrollback.

## Eventos (Rust → UI, bajo volumen)

Emitidos **solo a la ventana del proyecto** (`emit_to`):

| Evento | Payload | Reacción de la UI |
|---|---|---|
| `feature:changed` | `featureId` | refrescar panel izquierdo |
| `agent:status` | `agentId`, `status` | actualizar indicador |
| `fs:changed` | `worktreeId`, `paths[]` | refrescar nodos visibles del explorador / editor abierto |
| `git:changed` | `worktreeId` | re-pedir status/branches; refrescar diff si está visible |

## Errores

Todos los comandos devuelven `Result<T, IpcError>` con `IpcError { code: string, message: string }`. `code` proviene de `DomainError`/`PortError` (`"FEATURE_NOT_FOUND"`, `"LAST_WORKTREE"`, `"GIT_CONFLICT"`, …).
