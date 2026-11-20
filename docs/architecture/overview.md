# Visión general de la arquitectura

Diagrama: [gryphus-architecture.html](gryphus-architecture.html)

## Principio rector

**Arquitectura hexagonal (puertos y adaptadores) sin ceremonia extra.** El dominio define *qué* necesita (traits). Los adaptadores definen *cómo* (git, PTY, disco). Los casos de uso los combinan. Tauri es solo un adaptador más.

Esto nos da tres cosas:
1. **Probar sin IO:** cada caso de uso se prueba con fakes en memoria (rápido y determinista).
2. **Cambiar piezas sin romper el resto:** cambiar `gix` por la CLI de git solo afecta a `gryphus-git`.
3. **Entender rápido:** cada crate tiene una sola responsabilidad y una dirección de dependencia.

## Estructura del repositorio

```
gryphus/
├─ Cargo.toml                 # workspace
├─ crates/
│  ├─ gryphus-core/           # dominio + puertos. SIN IO, sin tokio, sin tauri
│  ├─ gryphus-app/            # casos de uso, ProjectSession, bus de eventos
│  ├─ gryphus-git/            # adaptador GitPort (gix para lecturas, CLI git para mutaciones)
│  ├─ gryphus-agents/         # adaptador AgentRuntimePort (portable-pty + supervisor)
│  ├─ gryphus-fs/             # adaptador FsPort (árbol perezoso, lectura/escritura, notify)
│  ├─ gryphus-store/          # adaptador StorePort (estado del proyecto + config global)
│  ├─ gryphus-ipc/            # DTOs compartidos backend ⇄ UI (solo serde; compila a WASM)
│  └─ gryphus-testkit/        # fakes de todos los puertos + fixtures (repos git temporales)
├─ tools/
│  └─ fake-agent/             # binario que simula un CLI de agente (para integración y e2e)
├─ src-tauri/                 # binario Tauri: comandos, Channels, gestión de ventanas
├─ ui/                        # frontend Leptos (CSR, WASM, Trunk)
├─ e2e/                       # pruebas e2e (WebdriverIO + tauri-driver)
└─ docs/
```

## Capas y responsabilidades

| Capa | Crate | Responsabilidad | NO hace |
|---|---|---|---|
| Presentación | `ui/` (Leptos/WASM) | Pintar los tres paneles, capturar input, renderizar terminal/editor/diff | Lógica de negocio, acceso a disco |
| Contrato IPC | `gryphus-ipc` | Tipos de comandos, Channels, eventos y errores compartidos | Lógica; no depende del dominio |
| Adaptador IPC | `src-tauri` | Traducir comandos Tauri ⇄ casos de uso y dominio ⇄ DTO; mapear ventana → proyecto; reenviar eventos | Reglas de negocio |
| Aplicación | `gryphus-app` | Casos de uso (`create_feature`, `spawn_agent`, …), orquestar puertos, publicar eventos | Hablar con Tauri o con el SO directamente |
| Dominio | `gryphus-core` | Entidades, invariantes, errores, **definición de puertos** | Cualquier IO |
| Infraestructura | `gryphus-git`, `gryphus-agents`, `gryphus-fs`, `gryphus-store` | Implementar un puerto cada uno | Conocer a otros adaptadores |

## Regla de dependencias

```
ui ──IPC──▶ src-tauri ──▶ gryphus-app ──▶ gryphus-core ◀── adaptadores (git, agents, fs, store)
 │               │   └──────────────── construye e inyecta los adaptadores ───────────────┘
 └──▶ gryphus-ipc ◀┘   (tipos compartidos)
```

- `gryphus-core` no depende de ningún otro crate del workspace.
- `gryphus-ipc` solo depende de `serde`. Lo usan `ui` y `src-tauri`; nadie más.
- `ui` depende solo de `gryphus-ipc` (y de Leptos); nunca del dominio ni de los adaptadores.
- `gryphus-app` depende **solo** de `gryphus-core` (usa los traits, nunca los adaptadores concretos).
- Cada adaptador depende **solo** de `gryphus-core`. Los adaptadores no se conocen entre sí.
- `src-tauri` es la *composition root*: el único lugar que conoce todas las implementaciones concretas y las inyecta.
- Esta regla se verifica en CI (ver [testing.md](../testing.md#guardias-de-arquitectura)).

## Cómo se comunican las partes

Hay exactamente **tres canales**, y cada uno tiene un uso:

| Canal | Dirección | Uso | Ejemplo |
|---|---|---|---|
| **Comandos** (`invoke`) | UI → Rust | Peticiones/respuestas tipadas | `create_feature`, `read_file`, `git_diff` |
| **Channels** (`tauri::ipc::Channel`) | Rust → UI | Flujos de alto volumen, uno por suscripción | Salida PTY de un agente |
| **Eventos** (`emit_to(window)`) | Rust → UI | Notificaciones de cambio de bajo volumen | `agent:status`, `fs:changed`, `git:changed` |

Dentro de Rust, los adaptadores **no** llaman a la UI. Publican `DomainEvent`s en el **bus de eventos** de la `ProjectSession` (`tokio::sync::broadcast`). `src-tauri` se suscribe al bus y reenvía a la ventana del proyecto. Detalle en [ipc.md](ipc.md).

### Ejemplo: lanzar un agente

1. La UI llama `invoke("spawn_agent", { featureId, worktreeId, provider: "claude" })`.
2. `src-tauri` resuelve la ventana → `ProjectSession` y llama `AgentService::spawn`.
3. `gryphus-app` valida contra el dominio (la feature existe, el worktree pertenece a la feature, el proveedor está configurado), persiste el agente vía `StorePort` y pide a `AgentRuntimePort` que arranque el proceso.
4. `gryphus-agents` abre una PTY en el directorio del worktree y lanza el CLI.
5. Se publica `AgentStatusChanged(Running)`; la UI lo recibe y muestra el agente.
6. La vista central abre un Channel con `attach_agent`; recibe primero el scrollback guardado y después la salida en vivo, en lotes.

## Casos de uso (`gryphus-app`)

`ProjectSession` es el estado vivo de **un** proyecto (una ventana). Recibe los puertos en un `Ports { git, agents, fs, store, events }` y expone:

| Grupo | Operaciones |
|---|---|
| Ciclo de vida | `open(path, ports)` (carga o crea el proyecto, `reset_after_restart`, sincroniza worktrees, empieza a vigilar), `close()` (mata agentes activos y guarda) |
| Features | `create_feature(name, New{base} \| Existing(wt))`, `delete_feature(f, remove_worktrees, force)`, `attach_worktree`, `detach_worktree` |
| Agentes | `spawn_agent`, `stop_agent`, `restart_agent`, `remove_agent`, `agent_input`, `agent_resize`, `attach_agent` |
| Archivos | `list_dir`, `read_file`, `write_file` (rutas relativas al worktree, validadas como `RelPath`) |
| Git | `git_status`, `git_diff`, `git_overview`, `refresh_worktrees` |

Reglas de implementación:
- **Nunca** se mantiene el lock del proyecto durante una llamada a un puerto.
- Toda mutación pasa por `mutate()`: aplica la regla de dominio, guarda y solo entonces publica el evento.
- **Deshacer en fallos parciales:** si `create_feature` crea el worktree en git pero no puede registrar la feature, borra el worktree y lo olvida en el registro. `delete_feature` valida contra una copia del proyecto antes de tocar git.
- **Carrera de salida instantánea:** si un agente termina antes de marcarse `Running`, se respeta el estado de salida.
- **Vigilancia:** hay un watcher por worktree presente. Los cambios generan `FilesChanged` + `GitChanged`. Un cambio en el `.git` del principal (commit, worktree creado fuera de Gryphus…) vuelve a sincronizar los worktrees y publica `GitChanged` para todos.
- **Sin `GitRefresher` propio** (desviación del diseño inicial): el status se calcula cuando la UI lo pide tras un `GitChanged`. Como el watcher ya agrupa los eventos (150 ms), no hacía falta otra capa.
- Los errores son `AppError` (`Domain` | `Port`) con el mismo `code()` estable que llega a la UI.

Pruebas: 25 casos con fakes de todos los puertos (`crates/gryphus-app/tests/session.rs`).

## Modelo de concurrencia

- **Comandos IPC:** el trabajo bloqueante (git, disco, procesos) corre en `spawn_blocking` del runtime de Tauri, nunca en el hilo de la UI. La entrada de teclado (`agent_input`) es síncrona porque escribir en la PTY es inmediato.
- **Sesión:** `ProjectSession` guarda el `Project` tras un `Mutex` que nunca se mantiene durante una llamada a un puerto.
- **Agentes:** tres hilos por agente (lector, flusher y waiter; ver [agents.md](agents.md#piezas-internas-ptyruntime)), sin runtime async.
- **Watchers:** un hilo por worktree vigilado agrupa los eventos de `notify` (150 ms).

## Presupuestos de rendimiento

Medidos con la app en **release** en Windows 11 (WebView2 158). Los mide la prueba `e2e/tests/perf.rs`, que guarda `target/perf.json`, junto con `cargo bench -p gryphus-git` y `crates/gryphus-agents/tests/latency.rs`.

| Métrica | Objetivo | Medido | Estado |
|---|---|---|---|
| Abrir directorio en el explorador (1000 archivos, vía IPC) | < 30 ms | **3 ms** (raíz: 1,6 ms) | ✅ |
| Latencia tecla → eco (UI → IPC → PTY → agente → xterm) | < 16 ms | **18 ms** mediana, p95 18 ms | ⚠️ límite de ConPTY |
| `git status` en repo de 10 000 archivos (adaptador) | < 150 ms | **151 ms** | ⚠️ coste de `lstat` en NTFS |
| Arranque en frío hasta UI montada | < 400 ms | **455–477 ms** | ❌ |
| Memoria en reposo, proceso Gryphus (Rust) | — | **6,9 MB** privados | ✅ |
| Memoria en reposo, total con WebView2 | < 120 MB | 258 MB privados / 412 MB working set | ❌ presupuesto irreal para WebView2 |

**Lectura de los resultados:**
- **Eco:** ConPTY por sí solo tarda ~21 ms en el backend, medido sin Gryphus (coalescencia de 1 ms). Gryphus no añade latencia: el throttle con flanco de subida envía el primer lote de inmediato. Así se corrigió un problema real (de 52 ms a 21 ms), ver [agents.md](agents.md#piezas-internas-ptyruntime). En Linux una PTY no tiene ese coste.
- **`git status`:** ~70 % es leer los metadatos de los archivos (ADR-005). La mejora posible es `core.fsmonitor`, como opción de configuración.
- **Memoria:** WebView2 es Chromium multiproceso (navegador, GPU, renderer, utilidades). Su consumo base supera los 120 MB por sí solo, así que ese presupuesto no era alcanzable con Tauri en Windows. Lo que controla Gryphus, el proceso Rust, ocupa 7 MB. Propuesta: medir por separado "Gryphus < 50 MB" y "total < 300 MB".
- **Arranque (pendiente de optimizar):** incluye crear el entorno WebView2, cargar el WASM de la UI (474 KB tras `wasm-opt`) y el bundle JS (1,1 MB). Candidatos, de mayor a menor impacto esperado:
  1. Cargar CodeMirror y sus lenguajes bajo demanda: no hacen falta para pintar la primera pantalla.
  2. Crear la ventana oculta y mostrarla al montar la UI, para que se vea antes y sin parpadeo.
  3. Medir el desglose (WebView2 / JS / WASM) antes de seguir.