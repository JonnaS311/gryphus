# Roadmap de implementación

Cada hito termina con su suite de pruebas en verde y la documentación actualizada. El orden va de adentro hacia afuera: dominio → adaptadores → IPC → UI.

Estado: ✅ hecho · ⏳ siguiente

| # | Hito | Entregable | Pruebas que lo cierran |
|---|---|---|---|
| 0 ✅ | Esqueleto | Workspace Cargo, crates vacíos (incl. `gryphus-ipc`), `src-tauri`, `ui/` (Leptos CSR + Trunk), CI con fmt/clippy/test, guardias de dependencias | CI verde con una prueba trivial por crate; la app abre una ventana Leptos vacía |
| 1 ✅ | Dominio | Entidades, invariantes, errores, puertos y eventos en `gryphus-core` | Unitarias + `proptest` de invariantes |
| 2 ✅ | Testkit | Fakes de todos los puertos, `TempRepo`, `fake-agent`, suites de contrato | Contratos pasan contra los fakes |
| 3 ✅ | Git | `CliGit` sobre la CLI de git (ADR-005) | Contrato git contra adaptador real; benchmark de status |
| 4 ✅ | Fs y store | Árbol perezoso, lectura/escritura atómica, watcher, persistencia JSON | Contratos fs/store; pruebas de path traversal |
| 5 ✅ | Agentes | Supervisor PTY, ring buffer, coalescencia, config de proveedores | Integración con `fake-agent` (eco, flood, exit code) |
| 6 ✅ | Casos de uso | `ProjectSession` (features, agentes, archivos, git, vigilancia) | Unitarias con fakes de cada flujo |
| 7 ✅ | IPC | DTOs en `gryphus-ipc`, comandos, Channels, eventos, `WindowRegistry` | Pruebas de comandos con `tauri::test`; mapeos dominio ⇄ DTO |
| 8 ✅ | UI | Bridge (Tauri, xterm, CodeMirror), tres paneles, terminal, editor, diff, barra de estado | `cargo test` de `logic.rs` + e2e (sustituye a `wasm-bindgen-test`) |
| 9 ✅ | E2E | Los 9 flujos de [testing.md](testing.md#flujos-obligatorios) en Rust (`fantoccini`) | Pasan localmente en Windows; jobs de CI configurados (sin ejecutar aún) |
| 10 ✅ | Rendimiento | Medir contra los presupuestos de [overview.md](architecture/overview.md#presupuestos-de-rendimiento-objetivos-medibles) | Benchmarks con línea base en CI |
