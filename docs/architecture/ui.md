# Frontend (`ui/`)

**Leptos en modo CSR, compilado a WASM con Trunk.** Todo en Rust; reactividad fina por señales (sin DOM virtual). Ver [ADR-010](../decisions.md#adr-010--leptos-csr-wasm-para-la-ui).

| Necesidad | Solución |
|---|---|
| Componentes y estado | Leptos (señales, `Resource` para datos de IPC) |
| Llamadas IPC | `gryphus-ipc` (tipos compartidos) + `bridge/tauri.rs` |
| Terminal del agente | `xterm.js` + addon WebGL + addon fit, vía `bridge/xterm.rs` |
| Editor de código | CodeMirror 6 (lenguajes cargados bajo demanda), vía `bridge/editor.rs` |
| Diff | `@codemirror/merge` (lado a lado y unificado), vía `bridge/editor.rs` |

## Layout

Tres paneles redimensionables y una barra de estado:

| Zona | Componente | Fuente de datos |
|---|---|---|
| Izquierda | `AgentsPanel` → `FeatureItem` → `WorktreeItem`, `AgentItem` | `get_project`, eventos `feature:changed`, `agent:status` |
| Centro | `MainView` con pestañas: `AgentTerminal`, `CodeEditor`, `DiffView` | `attach_agent`, `read_file`, `git_diff` |
| Derecha | `FileExplorer` (árbol perezoso del worktree activo) | `list_dir`, `git_status`, `fs:changed` |
| Abajo | `StatusBar` | `git_overview` |

## Estructura

```
ui/
├─ Cargo.toml       # crate cdylib/bin para wasm32-unknown-unknown; depende de leptos y gryphus-ipc
├─ Trunk.toml
├─ index.html
├─ js/              # glue JS mínimo: crea/destruye xterm y CodeMirror, expone funciones planas
└─ src/
   ├─ main.rs       # monta <App/>
   ├─ bridge/       # ÚNICO lugar con wasm-bindgen / JS
   │  ├─ tauri.rs   # invoke, listen, Channel  (window.__TAURI__ con withGlobalTauri)
   │  ├─ xterm.rs
   │  └─ editor.rs
   ├─ ipc/          # API tipada: fn create_feature(req) -> Result<Feature, IpcError> (usa bridge/tauri)
   ├─ state/        # señales globales: ProjectState, ViewState, GitState (provide_context)
   ├─ panels/       # AgentsPanel, FileExplorer, StatusBar
   ├─ views/        # MainView, AgentTerminal, CodeEditor, DiffView
   └─ components/   # piezas pequeñas reutilizables
```

Reglas:
- **Solo `bridge/` usa `wasm-bindgen` o toca JS.** El resto de la UI es Rust normal.
- **Solo `ipc/` llama al bridge de Tauri.** Paneles y vistas usan `state/` e `ipc/`. `ipc/` se define detrás de un trait `IpcClient` para poder inyectar un cliente falso en pruebas.
- **El estado de negocio vive en el backend.** `state/` es caché de lo que devuelve el backend + estado puro de UI (pestaña activa, tamaños de panel).
- **Worktree activo** = el del agente o archivo seleccionado; el explorador y la barra de estado siguen al worktree activo.

## Eficiencia en la UI

- Una sola instancia de xterm en la vista central; al cambiar de agente se limpia y se re-adjunta (el scrollback viene del backend).
- Los bytes del Channel PTY pasan del bridge a xterm sin convertirlos a `String` en Rust (`Uint8Array` directo).
- El árbol del explorador carga un nivel por expansión y virtualiza listas largas.
- Los diffs grandes se muestran por archivo, plegados, y se expanden bajo demanda.
- Build de release con `opt-level = "z"`, `lto = true` y `wasm-opt` para un WASM pequeño.

## Atajos (inicial)

| Atajo | Acción |
|---|---|
| `Ctrl+1/2/3` | Pestaña Agente / Editor / Diff |
| `Ctrl+N` | Nueva feature |
| `Ctrl+Shift+A` | Nuevo agente en la feature seleccionada |
| `Ctrl+S` | Guardar archivo |
| `Ctrl+P` | Buscar archivo en el worktree activo |
