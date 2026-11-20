# Estrategia de pruebas

Todo el desarrollo es **orientado a pruebas (TDD)**: primero la prueba que falla, luego el código mínimo, luego refactor. Objetivo: que ningún cambio rompa el sistema sin que CI lo detecte.

## Pirámide

| Nivel | Dónde | Qué prueba | Herramientas | Velocidad |
|---|---|---|---|---|
| Unitarias de dominio | `gryphus-core` | Invariantes, transiciones de estado, validaciones | `cargo test`, `proptest` | ms |
| Unitarias de casos de uso | `gryphus-app` | Orquestación con **fakes** de todos los puertos | `gryphus-testkit` | ms |
| Contrato de puertos | `gryphus-testkit` | Una suite por puerto que corren el fake **y** el adaptador real | macros de suite compartida | ms–s |
| Integración de adaptadores | `gryphus-git`, `-agents`, `-fs`, `-store` | IO real: repos git temporales, PTY con `fake-agent`, disco temporal | `tempfile`, `fake-agent` | s |
| Backend completo | `src-tauri/tests/backend.rs` | Casos de uso con **todos** los adaptadores reales, sin UI | `TempRepo`, `fake-agent` | s |
| UI lógica | `ui/src/logic.rs` | Reglas de presentación puras | `cargo test` (nativo) | ms |
| Pegamento JS | `ui/js/test` | Decodificador del formato binario de salida | `node --test` | ms |
| E2E | `e2e/` | App real (WebView) completa, flujos de usuario | Rust + `fantoccini` + `tauri-driver` | s |

Estado actual: **233 pruebas** en `cargo test --workspace`, todas en verde. Incluyen las 3 e2e, que sin `GRYPHUS_E2E=1` se cuentan pero no hacen nada. Aparte hay 4 pruebas de JS. Las pruebas de componentes con `wasm-bindgen-test` previstas al inicio se sustituyeron por e2e contra la app real (ver [ui.md](architecture/ui.md#pruebas)).

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

Estado actual: los 43 casos pasan **contra los fakes y contra los adaptadores reales** (`CliGit`, `LocalFs`, `JsonStore`, `PtyRuntime` con ConPTY).

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

## E2E

Las pruebas e2e están **en Rust** (`e2e/`, crate `gryphus-e2e`, cliente WebDriver `fantoccini`). Arrancan la app real a través de `tauri-driver` y la controlan como un usuario: clics, teclado en xterm y CodeMirror, lectura del DOM y capturas solo de la WebView (en `target/e2e-screens/`).

Cada prueba prepara:
- un repo git temporal (`TempRepo`);
- un `GRYPHUS_DATA_DIR` propio, para no tocar los datos reales del usuario, cuya `config.toml` apunta los tres proveedores a `fake-agent`.

Las confirmaciones nativas se aceptan sustituyendo `window.confirm` y quedan registradas para hacer aserciones. El texto de la terminal se lee con `GryphusJs.debugTerminalText()`, porque WebGL no deja texto en el DOM.

```powershell
# Requisitos (una vez)
cargo install tauri-driver --locked
# Windows: msedgedriver de la MISMA versión que WebView2, en target/e2e-tools/msedgedriver-<versión>/
#          (o su ruta en GRYPHUS_NATIVE_DRIVER). Linux: WebKitWebDriver (paquete webkit2gtk-driver).

# Ejecutar (compila la app con el frontend embebido en target/e2e)
$env:GRYPHUS_E2E = "1"; cargo test -p gryphus-e2e -- --nocapture
```

Sin `GRYPHUS_E2E=1` se omiten, así `cargo test --workspace` sigue siendo rápido.

### Flujos obligatorios

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

Los 9 flujos se cubren con 3 pruebas en `e2e/tests/flows.rs`: `open_project_once_per_window` (1-2), `feature_agent_editor_and_diff` (3-8) y `state_persists_across_restarts` (9). **Estado: pasan en Windows 11 (WebView2 158) en ~16 s.**

Las e2e ya detectaron un fallo real: el explorador colapsaba las carpetas abiertas con cada cambio en disco.

> En macOS no hay driver de WKWebView. En CI los e2e están configurados para Windows y Linux (ver `.github/workflows/ci.yml`), aunque esos jobs aún no se han ejecutado.

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
