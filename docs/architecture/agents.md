# Runtime de agentes (`gryphus-agents`)

Implementa `AgentRuntimePort`. Lanza los CLIs de agente dentro de una **pseudo-terminal (PTY)** para que se comporten exactamente como en una terminal real (colores, TUI, prompts interactivos).

## Proveedores soportados

Solo tres, configurables: **opencode**, **claude** (Claude Code) y **codex**. Cada proveedor es un dato de configuración, no código: añadir flags o cambiar el binario no requiere recompilar.

Archivo de configuración global: `~/.config/gryphus/config.toml` (en Windows `%APPDATA%\gryphus\config.toml`).

```toml
[agents.claude]
command = "claude"
args = []
env = {}

[agents.codex]
command = "codex"
args = []

[agents.opencode]
command = "opencode"
args = []
```

- `ProviderId` es un enum cerrado (`OpenCode | Claude | Codex`); la configuración solo ajusta comando, argumentos y variables de entorno.
- Al iniciar se verifica que cada `command` exista en el `PATH`; los no encontrados aparecen deshabilitados en la UI con el motivo.
- `gryphus-agents` no tiene lógica específica de proveedor más allá de construir el `SpawnSpec`. Si en el futuro un proveedor necesita algo especial, se añade un `ProviderAdapter` con una sola función `fn spawn_spec(&self, cfg, worktree) -> SpawnSpec`.

## Piezas internas

```
AgentSupervisor
 ├─ HashMap<AgentId, AgentProcess>
 └─ AgentProcess
     ├─ pty master (portable-pty / ConPTY en Windows)
     ├─ child process
     ├─ reader task (spawn_blocking) ──▶ RingBuffer (scrollback, 2 MiB por defecto)
     │                                └─▶ subscribers (mpsc) ──▶ Channel de la UI
     └─ waiter task ──▶ publica AgentStatusChanged(Exited/Failed)
```

- **RingBuffer:** guarda los últimos N bytes de salida. Permite cambiar de vista y volver sin perder contexto, y sin mantener un xterm vivo por agente en la UI.
- **Entrada:** `write` escribe directo al master de la PTY.
- **Resize:** la UI informa columnas/filas al cambiar el tamaño de la vista; se propaga a la PTY.
- **Apagado:** `kill` envía la señal de terminación; si el proceso no termina en 3 s, se fuerza.

## Persistencia de agentes

El **registro** de agentes (id, feature, worktree, proveedor) se persiste en el estado del proyecto. Los **procesos** no sobreviven al cierre de la app: al reabrir el proyecto, los agentes aparecen como `Exited` y se pueden reiniciar con un clic en el mismo worktree.

## Agente falso para pruebas

`tools/fake-agent` es un binario mínimo que imita a un CLI de agente. Se usa en las pruebas de integración de `gryphus-agents` y en e2e (configurado como `command` de los tres proveedores), de modo que ninguna prueba depende de tener instalados o autenticados los CLIs reales.

Al arrancar imprime `fake-agent ready` y luego interpreta línea a línea:

| Línea | Efecto |
|---|---|
| `hola` | imprime `echo: hola` |
| `::exit [N]` | termina con código `N` (0 por defecto) |
| `::flood N` | imprime `line 0` … `line N-1` |
| `::write RUTA CONTENIDO` | escribe el archivo relativo al cwd (crea carpetas) e imprime `wrote RUTA` |
| otro `::…` | imprime `unknown command: …` |

El lenguaje está definido una sola vez en `gryphus_testkit::agent_script`. Lo usan el binario y `FakeAgentRuntime`, así que el fake en memoria y el proceso real responden igual (ver [testing.md](../testing.md#fakes-gryphus-testkitfakes)).

> Pendiente para el hito 5: los tests de `gryphus-agents` necesitan la ruta del binario `fake-agent`, y `CARGO_BIN_EXE_*` solo existe dentro de su propio paquete. Plan: un helper en el testkit que lo compile con `cargo build -p fake-agent` y devuelva la ruta.
