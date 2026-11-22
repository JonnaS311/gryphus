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

## Piezas internas (`PtyRuntime`)

```
PtyRuntime ─ HashMap<AgentId, AgentProc>
AgentProc
 ├─ master PTY (portable-pty; ConPTY en Windows) + writer + killer
 ├─ hilo lector ──▶ bytes pendientes ──(Condvar)──▶ hilo flusher
 │                                                  └─ bajo un único lock:
 │                                                     ring buffer (VecDeque, 2 MiB)
 │                                                     + envío Data a los suscriptores
 └─ hilo waiter: wait() → deja drenar la salida (100 ms inactiva, máx 2 s)
                 → cierra la PTY → espera al flusher → Exited + on_exit (una sola vez)
```

- **Coalescencia con flanco de subida:** si no se envió nada en los últimos 16 ms, el lote sale **de inmediato** (eco de teclas sin espera). Si la salida no para, se agrupa y sale como mucho una vez cada 16 ms. 3000 líneas llegan en menos de 300 mensajes (`output_is_coalesced_into_few_messages`). La espera usa `thread::sleep`, de alta resolución en Windows. `Condvar::wait_timeout` redondea a ~15,6 ms, y con él el eco medía 52 ms en lugar de 21 ms (`tests/latency.rs`).
- **Orden garantizado:** el flusher añade al ring buffer y envía a los suscriptores bajo el mismo lock que usa `attach` para mandar el Scrollback. Un suscriptor nuevo nunca ve bytes duplicados ni desordenados.
- **Handshake de ConPTY:** al arrancar, ConPTY envía `ESC[6n` (pregunta por la posición del cursor) y no produce salida hasta recibir respuesta. Como un agente puede arrancar sin vista conectada, el runtime responde `ESC[1;1R` y quita la pregunta de la salida (si xterm.js respondiera también, el agente recibiría basura en su entrada). Solo se hace en los primeros 1024 bytes y solo en Windows.
- **Resolución del programa:** se busca en el `PATH` (o en el `PATH` del `env` del proveedor) respetando `PATHEXT`. En Windows, los CLIs instalados con npm (`claude.cmd`, `codex.cmd`, `opencode.cmd`) se lanzan con `cmd.exe /C`. En macOS y Linux solo cuentan los archivos con permiso de ejecución. Un programa inexistente es `UNAVAILABLE`.
- **`PATH` en macOS:** una app abierta desde Finder hereda de `launchd` un `PATH` mínimo, sin Homebrew ni npm global. Al arrancar (`src-tauri/src/shell_env.rs`), si no se lanzó desde un terminal, Gryphus ejecuta `$SHELL -l -i -c` para leer el `PATH` del usuario y lo une al heredado más rutas habituales (`/opt/homebrew/bin`, `~/.local/bin`, …). Si el shell tarda más de 3 s, se usan solo esas rutas.
- **Entorno:** se hereda el del proceso y se añaden `TERM=xterm-256color`, `COLORTERM=truecolor` y el `env` del proveedor.
- **Apagado:** `kill` marca el agente y lo termina; la salida se reporta como `Exited { code: None }`. `kill_all()` se usa al cerrar la app.
- **Cierre robusto:** si el lector no recibe EOF tras cerrar la PTY, se da por terminado a los 2 s para que `on_exit` siempre llegue.

## Aviso de "terminó y espera instrucciones"

`gryphus-app` (`activity.rs`) se suscribe a la salida de cada agente vivo y publica `DomainEvent::AgentIdle` (evento `agent:idle`) cuando:

- tras una orden del usuario (entrada con Enter) el agente produjo salida y luego quedó **4 s en silencio** (mientras trabajan, los CLIs animan un indicador), o
- emite un aviso de terminal: campana (BEL) suelta, OSC 9 (salvo `9;4`, progreso) u OSC 777.

Se avisa una sola vez por orden. La UI marca la feature (cabecera parpadeando y botón rojo) salvo que el usuario ya esté mirando ese agente, y el aviso se apaga al abrirlo.

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

Las pruebas de otros paquetes obtienen la ruta del binario con `gryphus_testkit::fake_agent_bin()`, que lo compila una vez por proceso (`cargo build -p fake-agent --message-format=json`).
