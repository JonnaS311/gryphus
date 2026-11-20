# Frontend (`ui/`)

**Leptos 0.8 en modo CSR, compilado a WASM con Trunk.** Todo en Rust salvo un pegamento JS mínimo para xterm.js y CodeMirror 6. Ver [ADR-010](../decisions.md#adr-010--leptos-csr-wasm-para-la-ui).

## Estructura

```
ui/
├─ index.html, style.css, Trunk.toml
├─ js/                 pegamento JS (npm + esbuild): src/index.js, src/stream.js, test/
├─ vendor/             gryphus-js.js + .css generados por `npm run build` (versionados:
│                      compilar la app no requiere Node)
└─ src/
   ├─ main.rs          monta <App/>
   ├─ app.rs           bienvenida o espacio de trabajo, eventos de Rust, atajos
   ├─ store.rs         estado global (señales): proyecto, selección, vista, status, ticks
   ├─ logic.rs         reglas de presentación PURAS (con pruebas nativas)
   ├─ layout.rs        CenterView (pestañas) y atajos
   ├─ ipc.rs           API tipada del backend (única capa que llama al bridge de Tauri)
   ├─ bridge/          ÚNICO lugar con wasm-bindgen / js-sys / web-sys
   │  ├─ tauri.rs      invoke, invoke_with_channel, listen
   │  ├─ xterm.rs      Terminal + TerminalChannel
   │  ├─ editor.rs     Editor (CodeMirror)
│  ├─ glue.rs       carga bajo demanda de ui/vendor/gryphus-js.js
│  ├─ perf.rs       performance.mark (medición del arranque)
   │  └─ dialog.rs     confirm()
   ├─ panels/          agents.rs (izquierda), explorer.rs (derecha), statusbar.rs
   └─ views/           main_view.rs, terminal.rs, editor.rs, diff.rs, welcome.rs
```

Reglas (verificadas por `tools/arch-guard`):
- **Solo `bridge/` toca JS.**
- **Solo `ipc.rs` llama al bridge de Tauri.** Paneles y vistas usan `store` e `ipc`.
- **El estado de negocio vive en Rust.** `store` es caché de lo que devuelve el backend más estado puro de UI (pestaña, selección, carpetas expandidas).

## Layout

| Zona | Componente | Datos |
|---|---|---|
| Izquierda | `AgentsPanel` → `FeatureItem` → `WorktreeRow`, `AgentRow`, `NewAgent`; `NewFeatureForm` | `get_project`; eventos `feature:*`, `agent:status`, `worktrees:changed` |
| Centro | `MainView` con pestañas `AgentTerminal`, `CodeEditor`, `DiffView` (las tres montadas; se ocultan con CSS) | `attach_agent`, `read_file`/`write_file`, `git_diff` |
| Derecha | `Explorer` → `TreeDir`/`TreeEntry` recursivos y perezosos | `list_dir`, `git_status`, `fs:changed` |
| Abajo | `StatusBar` (branch, ↑↓, cambios, agentes activos, ruta) | `git_overview`, `git_status` |
| Sin proyecto | `Welcome` (diálogo nativo o ruta escrita) | `pick_folder`, `open_project` |

**Worktree activo:** el del agente seleccionado o el que se elija en el panel izquierdo. El explorador, el diff y la barra de estado lo siguen.

## Flujo de datos

- **Eventos → señales:** `feature:*` y `worktrees:changed` recargan el proyecto. `agent:status` actualiza el estado del agente en sitio. `fs:changed` incrementa `fs_tick`, con lo que los directorios visibles se vuelven a listar y el archivo abierto se recarga si no tiene cambios sin guardar. `git:changed` del worktree activo incrementa `git_tick`, que recarga status, resumen y diff.
- **Salida de agentes sin pasar por WASM:** `Terminal::channel()` crea un `Channel` de Tauri en JS cuyo `onmessage` decodifica el formato binario y escribe directo en xterm. La entrada (`onData`) y el tamaño (`onResize`) sí pasan por Rust → `agent_input` / `agent_resize`.
- **Una terminal por ventana:** al cambiar de agente se reinicia y se vuelve a conectar; el backend reenvía el scrollback.
- **Editor:** Ctrl+S guarda (escritura atómica en Rust). Un indicador ● marca cambios sin guardar. Cargar otro archivo reinicia el historial de deshacer.
- **Explorador:** carga un nivel por expansión. El estado expandido vive en el `store`, así que sobrevive a las recargas causadas por cambios en disco (este fallo lo detectaron las pruebas e2e). Un listado solo se reemplaza si cambió.

## Eficiencia

- Una sola instancia de xterm (WebGL, con DOM como respaldo) y una de CodeMirror por ventana.
- Los bytes de salida van del backend a xterm sin copias en WASM ni JSON.
- Árbol perezoso y listas que solo se repintan si cambian.
- WASM de release con `opt-level = "z"`, LTO y `wasm-opt`.
- El pegamento JS (1,1 MB: xterm + CodeMirror) **no está en el camino crítico del arranque**. `bridge/glue.rs` lo inyecta bajo demanda y `App` lo precarga en segundo plano tras montar la UI; `Terminal::create` y `Editor::create` esperan a que esté listo.

## Atajos

| Atajo | Acción |
|---|---|
| `Ctrl+1/2/3` | Pestaña Agente / Editor / Diff |
| `Ctrl+S` | Guardar (en el editor) |

## Pegamento JS (`ui/js`)

```
cd ui/js && npm install && npm test && npm run build   # regenera ui/vendor/
```

Expone `window.GryphusJs.createTerminal`, `createEditor` y `debugTerminalText` (gancho para e2e). `src/stream.js` decodifica el mismo formato que `gryphus_ipc::stream`; `npm test` lo verifica.

## Pruebas

| Nivel | Qué |
|---|---|
| `cargo test -p gryphus-ui` | `logic.rs` y `layout.rs` (nativo, sin navegador) |
| `npm test` en `ui/js` | decodificador del formato binario |
| e2e (`gryphus-e2e`) | la UI real dentro de la app: ver [testing.md](../testing.md#e2e) |

Desviación del plan: no hay pruebas de componentes con `wasm-bindgen-test`. Los componentes se cubren con e2e contra la app real, que además verifican la integración con xterm, CodeMirror y Tauri. Esa integración no se probaría en un navegador aislado.
