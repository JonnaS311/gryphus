# Contrato IPC (UI ⇄ Rust)

`src-tauri` es un adaptador **delgado**. Cada comando valida la entrada, resuelve la sesión del proyecto a partir de la ventana que llama y ejecuta un caso de uso de `gryphus-app` en un hilo bloqueante (`spawn_blocking`), nunca en el hilo de la UI. No contiene reglas de negocio.

```
src-tauri/src/
├─ lib.rs        Builder: plugin de diálogo, AppState, cierre de ventanas, handlers
├─ state.rs      AppState (adaptadores compartidos) + Registry (ventana → ProjectSession)
├─ commands.rs   un #[tauri::command] por operación
├─ events.rs     TauriEvents: DomainEvent → emit_to(ventana del proyecto)
└─ dto.rs        traducción dominio ⇄ DTO de gryphus-ipc
```

## Ventana ↔ proyecto

- `Registry`: `HashMap<etiqueta de ventana, ProjectSession>`.
- **`open_project(path)`:** git resuelve la raíz real del repo.
  - Si otra ventana ya muestra ese proyecto, se enfoca esa ventana.
  - Si la ventana que llama no tiene proyecto (la de bienvenida), lo abre ahí mismo.
  - Si ya tiene uno, crea la ventana `project-<id>` con la sesión ya registrada.
- **Ningún comando recibe `projectId`:** el proyecto se deduce **siempre** de la ventana que invoca (ADR-003).
- **Al destruirse una ventana:** se suelta su terminal, `ProjectSession::close()` detiene sus agentes y guarda, y la sesión se elimina.
- **Cada sesión tiene su propio `PtyRuntime`,** porque los ids de agente son secuenciales por proyecto. `CliGit`, `LocalFs` y `JsonStore` se comparten entre sesiones.
- **Rutas:** datos en `app_data_dir()/projects/<id>.json` y configuración en `app_config_dir()/config.toml`.

## Tipos compartidos

Los tipos viven en **`gryphus-ipc`** (solo `serde`) y los compilan tanto `src-tauri` como la UI (WASM). Si un comando cambia, los dos fallan al compilar (ADR-011).

- Los nombres están en `gryphus_ipc::cmd::*` y `gryphus_ipc::event::*`.
- Todos los comandos reciben **un único argumento `req`** (`invoke("x", { req: {...} })`) y devuelven `Result<T, IpcError { code, message }>`.
- Los ids viajan como `u64`. Las rutas de archivo son `String` relativas al worktree; Rust las valida como `RelPath` (no se puede salir del worktree).

## Comandos

| Comando | `req` | Respuesta |
|---|---|---|
| `pick_folder` | — | `Option<String>` (diálogo nativo) |
| `open_project` | `OpenProjectReq { path }` | `OpenProjectRes { other_window }` |
| `get_project` | — | `Option<ProjectDto>` (`None` = pantalla de bienvenida) |
| `close_project` | — | — (detiene los agentes, guarda y deja la ventana en la bienvenida; idempotente; olvida el último proyecto) |
| `restore_last_project` | — | `bool` (al arrancar reabre en `main` el último proyecto abierto, guardado en `<data_dir>/last_project.txt`; una vez por proceso) |
| `create_feature` | `CreateFeatureReq { name, worktree: New{base} \| Existing{id} }` | id de la feature |
| `delete_feature` | `DeleteFeatureReq { feature, remove_worktrees, force }` | — |
| `spawn_agent` | `SpawnAgentReq { feature, worktree, provider }` | id del agente |
| `stop_agent` / `restart_agent` / `remove_agent` | `AgentReq { agent }` | — |
| `rename_agent` | `RenameAgentReq { agent, name }` (1–40 caracteres) | — |
| `agent_input` | `AgentInputReq { agent, data }` | — (síncrono: camino caliente) |
| `agent_resize` | `AgentResizeReq { agent, cols, rows }` | — |
| `attach_agent` | `AgentReq` + `channel` | — (ver Channels) |
| `detach_agent` | — | — |
| `list_dir` / `read_file` | `PathReq { worktree, path }` | `Vec<DirEntryDto>` / `FileContentDto` |
| `write_file` | `WriteFileReq { worktree, path, content }` | — |
| `rename_path` | `RenamePathReq { worktree, path, new_name }` (`new_name` es un solo componente; nunca sobrescribe) | ruta nueva |
| `copy_path` | `CopyPathReq { from_worktree, from, to_worktree, to_dir }` (si el nombre existe usa `nombre copia.ext`) | ruta creada |
| `git_status` | `WorktreeReq { worktree }` | `Vec<FileStatusDto>` |
| `git_diff` | `DiffReq { worktree, base: Option<branch> }` | `DiffDto` (`None` = sin commit; `Some` = desde el merge-base) |
| `git_overview` | — | `GitOverviewDto { head, branches }` |

`ProjectDto` incluye los proveedores con `available`, que indica si su comando existe en el PATH. Así la UI puede deshabilitar los que no están instalados.

## Channel de salida de agentes

`attach_agent` recibe un `tauri::ipc::Channel` y envía **bytes crudos**, sin JSON: la salida de una PTY ya son bytes. Formato (`gryphus_ipc::stream`):

| tag | Significado | payload |
|---|---|---|
| 0 | Scrollback (una vez, al conectar) | bytes |
| 1 | Data (en vivo, coalescida ~16 ms) | bytes |
| 2 | Exited | `i32` LE (`i32::MIN` = sin código) |
| 3 | Failed | motivo UTF-8 |

- **Una terminal por ventana:** un `attach_agent` nuevo suelta la conexión anterior de esa ventana.
- **Contrapresión:** si el Channel falla (la vista se fue), el runtime quita al suscriptor; el proceso nunca se bloquea por la UI.
- Si el agente no tiene proceso en esta sesión (p. ej. tras reabrir la app), se envía un Scrollback vacío y un Exited.

## Eventos (Rust → ventana del proyecto)

Se emiten con `emit_to(etiqueta)`, **solo a la ventana del proyecto**. Son notificaciones: la UI vuelve a pedir lo que necesite.

| Evento | Payload | Reacción de la UI |
|---|---|---|
| `feature:changed` / `feature:removed` | `FeatureEvent { feature }` | refrescar el proyecto |
| `agent:status` | `AgentStatusEvent { agent, status }` | actualizar el indicador |
| `agent:idle` | `AgentEvent { agent }` | avisar en su feature que terminó y espera instrucciones |
| `fs:changed` | `FilesChangedEvent { worktree, paths }` | refrescar el explorador y el archivo abierto |
| `git:changed` | `WorktreeEvent { worktree }` | volver a pedir status/diff si está visible |
| `worktrees:changed` | `()` | refrescar el proyecto |

## Errores

`IpcError.code` es el `code()` de `DomainError` o `PortError` (`FEATURE_NOT_FOUND`, `LAST_WORKTREE`, `CONFLICT`, `UNAVAILABLE`…), más estos códigos propios del IPC:

| Código | Significado |
|---|---|
| `NO_PROJECT` | la ventana no tiene proyecto |
| `INVALID_INPUT` | DTO inválido |
| `INTERNAL` | fallo del hilo de trabajo |

## Pruebas

| Dónde | Qué cubre |
|---|---|
| `gryphus-ipc` | JSON estable de los enums con etiqueta, nombres de comando únicos, ida y vuelta del formato binario |
| `src-tauri/src/dto.rs` | traducción dominio ⇄ DTO y validación de peticiones |
| `src-tauri/src/state.rs` | registro de un proyecto por ventana |
| `src-tauri/tests/backend.rs` | **flujo completo con adaptadores reales** (git, disco, ConPTY con `fake-agent`, JSON): feature → worktree → agente escribe un archivo → watcher → status/diff → borrar con `force` → reabrir |
