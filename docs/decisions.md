# Decisiones de arquitectura (ADRs)

Formato corto: decisión, motivo, alternativa descartada. Una decisión nueva o revertida se agrega aquí, no se borra la anterior (se marca como *Reemplazada*).

### ADR-001 — Tauri 2 + Rust
- **Decisión:** shell de escritorio con Tauri 2; toda la lógica en Rust.
- **Motivo:** usa el WebView del sistema (WebView2 en Windows): binario y memoria mucho menores que Electron.
- **Alternativa:** Electron (más pesado), UI nativa en Rust con egui/iced (no hay terminal y editor maduros equivalentes a xterm.js/CodeMirror).

### ADR-002 — Hexagonal con crates por responsabilidad
- **Decisión:** `core` (dominio+puertos), `app` (casos de uso), un crate por adaptador, `src-tauri` como composition root.
- **Motivo:** pruebas rápidas con fakes, cambios aislados, compilación incremental más rápida.
- **Alternativa:** un solo crate con módulos (más simple al inicio, pero los límites no se pueden hacer cumplir).

### ADR-003 — Una ventana por proyecto, proyecto deducido de la ventana
- **Decisión:** los comandos no reciben `projectId`; se resuelve por la ventana que invoca.
- **Motivo:** hace imposible por construcción que una ventana actúe sobre otro proyecto.

### ADR-004 — SolidJS para la UI *(Reemplazada por ADR-010)*
- **Decisión:** SolidJS + TypeScript.
- **Motivo:** reactividad fina sin DOM virtual; muy bajo coste de render.
- **Por qué se reemplazó:** se prefirió tener todo el proyecto en Rust (un solo lenguaje, tipos compartidos con el backend).

### ADR-010 — Leptos (CSR, WASM) para la UI
- **Decisión:** UI en Leptos en modo CSR (client-side rendering) compilada a WASM con Trunk.
- **Motivo:** un solo lenguaje en todo el proyecto; reactividad fina por señales (igual de eficiente que SolidJS); los tipos del IPC se comparten en compilación mediante el crate `gryphus-ipc`, sin generación de código.
- **Coste aceptado:** xterm.js y CodeMirror 6 siguen siendo JS. Se integran con un módulo JS pequeño (`ui/js/`) y bindings `wasm-bindgen` aislados en `ui/src/bridge/`. Ningún otro módulo de la UI toca JS.
- **Alternativa:** SolidJS (ADR-004).

### ADR-011 — Crate `gryphus-ipc` de tipos compartidos
- **Decisión:** los DTOs de comandos, Channels, eventos y errores viven en `gryphus-ipc` (solo `serde`), compilado tanto en `src-tauri` como en `ui` (WASM).
- **Motivo:** si un comando cambia de firma, el backend y la UI dejan de compilar a la vez. Elimina `specta` y la comprobación de bindings en CI.
- **Regla:** `gryphus-ipc` no depende de `gryphus-core`; `src-tauri` traduce dominio ⇄ DTO. Así la UI no arrastra el dominio y el dominio puede cambiar sin romper el contrato.

### ADR-005 — Git híbrido: gix para lecturas, CLI git para mutaciones
- **Motivo:** lecturas frecuentes en proceso (rápidas); mutaciones con la herramienta de referencia (seguras).
- **Alternativa:** `git2` (libgit2): dependencia C, soporte de worktrees más limitado.

### ADR-006 — Agentes en PTY con ring buffer en Rust
- **Motivo:** los CLIs de agente son TUIs interactivas; la PTY es la única forma fiel. El scrollback en Rust permite una sola instancia de xterm en la UI.
- **Alternativa:** modos no interactivos/JSON de cada CLI (difieren entre proveedores y pierden la interacción).

### ADR-007 — Streaming por `tauri::ipc::Channel` coalescido
- **Motivo:** los eventos globales de Tauri no están pensados para alto volumen; Channel es ordenado y por suscripción. Lotes de ~16 ms limitan los mensajes a ~60/s por agente.

### ADR-008 — Eventos como notificaciones, no como datos
- **Motivo:** una sola fuente de verdad (Rust); la UI re-pide. Evita bugs de sincronización.

### ADR-009 — Persistencia en JSON por proyecto
- **Decisión:** `<app-data>/projects/<projectId>.json`, escritura atómica (archivo temporal + rename). Nada se escribe dentro del repo del usuario.
- **Motivo:** el estado es pequeño (features, worktrees, registro de agentes); JSON es inspeccionable y suficiente.
- **Alternativa:** SQLite. Se reconsidera si se necesita historial de sesiones o búsqueda.

### ADR-012 — Worktrees fuera del repositorio
- **Decisión:** por defecto en `<padre-del-repo>/<nombre-repo>.worktrees/<feature>` (configurable).
- **Motivo:** no aparecen en el explorador ni en `git status` del repo principal, y no hace falta tocar `.gitignore`.

### ADR-013 — Los procesos de agente no sobreviven al cierre
- **Decisión:** al cerrar la app se detienen los agentes; al reabrir aparecen en `Exited` y se reinician con un clic en el mismo worktree.
- **Motivo:** evita procesos huérfanos y un daemon en segundo plano; mantiene el sistema simple.

### ADR-014 — E2E solo en Windows y Linux
- **Decisión:** la suite e2e corre en CI en Windows y Linux.
- **Motivo:** `tauri-driver` no soporta WKWebView en macOS. En macOS se cubre con pruebas unitarias, de integración y de componentes.
