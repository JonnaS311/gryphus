# Gryphus

Entorno de desarrollo de agentes (opencode, Claude Code, Codex) en Rust + Tauri + Leptos. Simple y eficiente.

Diseño y documentación: [docs/README.md](docs/README.md).

## Requisitos

- Rust (vía [rustup](https://rustup.rs)); `rust-toolchain.toml` instala el target `wasm32-unknown-unknown`.
- Windows: Visual Studio Build Tools (C++) y WebView2 (incluido en Windows 11).
- Linux: dependencias de Tauri (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, …).
- `cargo install trunk tauri-cli --locked`

## Comandos

| Tarea | Comando |
|---|---|
| Ejecutar en desarrollo | `cargo tauri dev` |
| Compilar la UI (necesario antes de `cargo test` la primera vez) | `trunk --config ui/Trunk.toml build` |
| Pruebas | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Formato | `cargo fmt --all` |

## Estructura

```
crates/      dominio, casos de uso, contrato IPC y adaptadores
src-tauri/   binario de escritorio (composition root)
ui/          frontend Leptos (WASM)
tools/       fake-agent y arch-guard (guardias de arquitectura)
docs/        diseño del sistema
```
