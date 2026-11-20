# Estrategia de pruebas

Todo el desarrollo es **orientado a pruebas (TDD)**: primero la prueba que falla, luego el código mínimo, luego refactor. Objetivo: que ningún cambio rompa el sistema sin que CI lo detecte.

## Pirámide

| Nivel | Dónde | Qué prueba | Herramientas | Velocidad |
|---|---|---|---|---|
| Unitarias de dominio | `gryphus-core` | Invariantes, transiciones de estado, validaciones | `cargo test`, `proptest` | ms |
| Unitarias de casos de uso | `gryphus-app` | Orquestación con **fakes** de todos los puertos | `gryphus-testkit` | ms |
| Contrato de puertos | `gryphus-testkit` | Una suite por puerto que corren el fake **y** el adaptador real | macros de suite compartida | ms–s |
| Integración de adaptadores | `gryphus-git`, `-agents`, `-fs`, `-store` | IO real: repos git temporales, PTY con `fake-agent`, disco temporal | `tempfile`, `fake-agent` | s |
| UI lógica | `ui/` | Estado (`state/`) y transformaciones con `IpcClient` falso | `cargo test` (nativo, sin navegador) | ms |
| UI componentes | `ui/` | Render de componentes Leptos y reacción a señales | `wasm-bindgen-test` en navegador headless | s |
| E2E | `e2e/` | App real completa, flujos de usuario | WebdriverIO + `tauri-driver` | min |

## Pruebas de contrato

Cada puerto tiene una suite de comportamiento escrita una sola vez en `gryphus-testkit/src/contracts/`. La deben pasar **el fake y el adaptador real**. Así el fake que usa `gryphus-app` en sus pruebas no puede divergir del real: si divergen, falla el contrato y no una prueba de alto nivel de forma misteriosa.

Cada suite tiene dos partes:

- Un **harness**: un trait que dice cómo obtener el puerto y cómo preparar el escenario desde fuera de él (escribir un archivo, hacer un commit…).
- Una **macro** que genera un `#[test]` por caso, para que cada caso aparezca con su nombre en la salida.

```rust
// En las pruebas de un adaptador (p. ej. crates/gryphus-git/tests/contract.rs)
struct Harness { repo: TempRepo, git: GixGit }
impl gryphus_testkit::GitHarness for Harness { /* port(), root(), write_file(), commit_all()… */ }
gryphus_testkit::git_contract!(Harness::new);
```

| Suite | Macro | Casos | Semántica que fija |
|---|---|---|---|
| `GitPort` | `git_contract!` | 16 | worktrees y branches, errores `AlreadyExists`/`Conflict`/`NotFound`, status con untracked/modified/deleted, diff contra HEAD y contra merge-base, **diff incluye archivos sin seguimiento**, `DiffLine` sin salto de línea |
| `FsPort` | `fs_contract!` | 11 | orden (directorios primero, sin distinguir mayúsculas), `.git` oculto, `.gitignore`, `Text`/`Binary`/`TooLarge`, escritura que crea padres, watcher con cancelación |
| `StorePort` | `store_contract!` | 6 | ida y vuelta, sobrescritura, config por defecto, `Corrupted` si el JSON es ilegible o viola invariantes |
| `AgentRuntimePort` | `agents_contract!` | 10 | Scrollback antes que Data, eco, código de salida, `on_exit` exactamente una vez, flood completo y en orden, relanzar, `Unavailable`, `NotFound` |

Estado actual: los **fakes pasan los 43 casos**. Cada adaptador real los ejecutará en su hito (3: git, 4: fs y store, 5: agentes).

La eficacia de los contratos está comprobada: al quitar del `FakeGit` el reporte de archivos borrados, falla exactamente `status_reports_untracked_modified_and_deleted` con un diff claro.

## Fakes (`gryphus-testkit::fakes`)

| Fake | Modelo | Ganchos para pruebas |
|---|---|---|
| `FakeGit` | commits, branches y worktrees en memoria; diff por LCS | `write_file`, `delete_file`, `commit_all`, `create_branch` |
| `FakeFs` | árbol en memoria; `.gitignore` simplificado (nombres exactos y `dir/`) | `write_raw` (cambio externo que avisa a los watchers), `create_dir`, `with_limit` |
| `FakeStore` | guarda **JSON** en memoria (ejercita la serialización real) | `write_raw_project`, `with_config`, `save_count` |
| `FakeAgentRuntime` | interpreta el lenguaje de `fake-agent` de forma síncrona | `emit`, `finish`, `spawned`, `written`, `size`, `allow_program` |

Todos tienen `fail_next(err)` para simular el fallo de la próxima llamada.

## Fixtures y utilidades

- `TempRepo::new()`: repo git **real** en un directorio temporal, con una configuración aislada (`GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM`). Arranca en `main` con un commit que contiene `README.md` = `"hola\n"`. Tiene `write_file`, `delete_file`, `commit_all`, `create_branch`, `add_worktree` y `git(cwd, args)`. Se borra al soltarse.
- `fake-agent`: CLI de agente determinista (ver [agents.md](architecture/agents.md#agente-falso-para-pruebas)). Su lenguaje vive en `gryphus_testkit::agent_script` y lo comparten el binario y `FakeAgentRuntime`.
- `RecordingEventSink`, `CollectingChunkSink`, `CollectingFsSink`: acumulan lo recibido para hacer aserciones.
- `wait_until(timeout, cond)` para efectos asíncronos, y `strip_ansi(bytes)` para comparar salida de terminal (ConPTY añade secuencias VT).

## E2E: flujos obligatorios

Corren contra el binario real con config que apunta los tres proveedores a `fake-agent`:

1. Abrir proyecto → los tres paneles cargan; el explorador muestra la raíz.
2. Abrir el mismo proyecto dos veces → se enfoca la ventana existente (no hay segunda ventana).
3. Crear feature con worktree nuevo → aparece en el panel, la branch existe en git.
4. Lanzar agente → la terminal muestra el banner; escribir texto produce eco.
5. El agente escribe un archivo → aparece en el explorador y en la vista diff.
6. Editar y guardar un archivo en el editor → el diff lo refleja.
7. Detener agente → estado `Exited`; reiniciar → vuelve a `Running`.
8. Intentar eliminar la feature con agente activo → error visible; tras detenerlo, se elimina.
9. Cerrar y reabrir → features y agentes persistidos (agentes en `Exited`).

> Nota Windows: `tauri-driver` usa `msedgedriver` (WebView2). En macOS no hay driver de WKWebView; los e2e corren en Windows y Linux en CI.

## Guardias de arquitectura

- Un test en CI (`cargo metadata` + script) falla si se viola la [regla de dependencias](architecture/overview.md#regla-de-dependencias), p. ej. si `gryphus-app` depende de `gryphus-git`.
- `cargo clippy -D warnings` y `cargo fmt --check` (incluye la UI, `--target wasm32-unknown-unknown`).
- Otra guardia comprueba que `wasm-bindgen`/`js-sys`/`web-sys` solo se importan en `ui/src/bridge/`.
- El contrato IPC no necesita comprobación aparte: `gryphus-ipc` es compartido y cualquier cambio incompatible falla al compilar.
- Benchmarks (`criterion`) de status, diff y throughput PTY; CI falla con regresiones > 20 % respecto a la línea base.

## Definición de "hecho"

Un cambio está terminado cuando:
1. Tiene pruebas que fallaban antes del cambio.
2. Pasa toda la suite local (`cargo test --workspace` y `wasm-pack test --headless` en `ui/`).
3. Si toca un flujo de usuario, tiene o actualiza un e2e.
4. Si cambia la arquitectura o el IPC, actualiza `docs/` (y el diagrama) en el mismo commit.
