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

Cada puerto tiene una suite de comportamiento escrita una sola vez en `gryphus-testkit`:

```rust
// gryphus-testkit/src/contracts/git.rs
pub fn git_port_contract<P: GitPort>(make: impl Fn(&TempRepo) -> P) {
    adds_worktree_on_new_branch(&make);
    lists_worktrees_including_main(&make);
    status_reports_untracked_and_modified(&make);
    // ...
}
```

Se ejecuta contra `FakeGit` (en memoria) y contra `GixGit`. Así garantizamos que el fake usado en las pruebas de `gryphus-app` se comporta como el real; si divergen, falla el contrato y no las pruebas de alto nivel de forma misteriosa.

## Fixtures

- `TempRepo::new()` → repo git real en un directorio temporal con un commit inicial; helpers `commit_file`, `branch`, `add_worktree`.
- `fake-agent` → CLI de agente determinista (ver [agents.md](architecture/agents.md#agente-falso-para-pruebas)).
- `FakeClock`, `FakeEventSink` (registra eventos para aserciones).

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
