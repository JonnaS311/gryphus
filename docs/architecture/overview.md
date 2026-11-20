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

## Modelo de concurrencia

- Un único runtime `tokio` multihilo (el de Tauri).
- Cada `ProjectSession` es dueña de su estado (`features`, `agents`) detrás de un `RwLock` corto; nunca se mantiene un lock durante IO.
- Cada agente tiene una tarea lectora dedicada (bloqueante, `spawn_blocking`) que llena un ring buffer y un `mpsc` hacia el Channel.
- Las operaciones git pesadas (status, diff) corren en `spawn_blocking` y se **eliminan duplicados**: si llega un nuevo `fs:changed` mientras se calcula un status, se recalcula una sola vez al terminar.

## Presupuestos de rendimiento (objetivos medibles)

| Métrica | Objetivo |
|---|---|
| Arranque en frío hasta ventana interactiva | < 400 ms |
| Memoria en reposo (1 proyecto, 0 agentes) | < 120 MB |
| Latencia tecla → eco en terminal del agente | < 16 ms |
| `git status` en repo de 10k archivos | < 150 ms |
| Abrir directorio en el explorador | < 30 ms (carga perezosa, un nivel) |

Los presupuestos se verifican con benchmarks (`criterion`) en los crates de infraestructura.
