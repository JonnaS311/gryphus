# Gryphus

[![CI](https://github.com/JonnaS311/gryphus/actions/workflows/ci.yml/badge.svg)](https://github.com/JonnaS311/gryphus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/JonnaS311/gryphus?include_prereleases&sort=semver)](https://github.com/JonnaS311/gryphus/releases)
[![Licencia: MIT](https://img.shields.io/badge/licencia-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri)
![Leptos](https://img.shields.io/badge/Leptos-0.8-EF3939)
![Plataformas](https://img.shields.io/badge/plataformas-Windows%20%7C%20Linux%20(experimental)-lightgrey)

Entorno de desarrollo de agentes de código (opencode, Claude Code, Codex). Simple y eficiente.

- **Una ventana = un proyecto** (un repositorio git).
- **Features:** el trabajo se agrupa por feature; cada una tiene sus worktrees de git y sus agentes, y cada agente corre aislado en su worktree.
- **Vista central:** terminal del agente, editor de archivos o diff del worktree.
- **Git integrado:** branches, worktrees, status, ahead/behind y diffs.

Diseño y documentación: [docs/README.md](docs/README.md).

## Descarga

Los instaladores están en [Releases](https://github.com/JonnaS311/gryphus/releases):

| Plataforma | Archivo |
|---|---|
| Windows 10/11 (x64) | `Gryphus_<versión>_x64-setup.exe` o `.msi` |
| Linux (x64) | `.AppImage`, `.deb` o `.rpm` |

> Proyecto en versión 0.x: los releases se publican como *pre-release*. Los binarios no están firmados, así que Windows SmartScreen puede pedir confirmación.
>
> **Linux es experimental:** compila y pasa las pruebas unitarias y de integración, pero los flujos E2E de la interfaz aún no están validados ahí ([seguimiento](https://github.com/JonnaS311/gryphus/issues/6)).

## Con qué está construido

| Capa | Tecnología |
|---|---|
| Lenguaje | [Rust](https://www.rust-lang.org) (stable, edición 2021) |
| Escritorio | [Tauri 2](https://tauri.app) (WebView2 en Windows, WebKitGTK en Linux) |
| Interfaz | [Leptos 0.8](https://leptos.dev) en modo CSR, compilado a WebAssembly con [Trunk](https://trunkrs.dev) |
| Terminal y editor | [xterm.js](https://xtermjs.org) y [CodeMirror 6](https://codemirror.net), empaquetados con esbuild en `ui/vendor/` |
| Agentes | PTY con [portable-pty](https://crates.io/crates/portable-pty) (ConPTY en Windows) |
| Git | CLI de `git` ([ADR-005](docs/decisions.md)) |
| Archivos | [notify](https://crates.io/crates/notify) (watcher) e [ignore](https://crates.io/crates/ignore) (`.gitignore`) |
| Pruebas | `cargo test`, [proptest](https://crates.io/crates/proptest), [criterion](https://crates.io/crates/criterion) y E2E con WebDriver ([fantoccini](https://crates.io/crates/fantoccini) + `tauri-driver`) |
| CI/CD | GitHub Actions: fmt, clippy y pruebas en Windows y Linux (E2E informativo); instaladores con [tauri-action](https://github.com/tauri-apps/tauri-action) |

Arquitectura hexagonal: el dominio (`gryphus-core`) define puertos (traits) y los adaptadores (git, fs, agentes, store) los implementan. `arch-guard` verifica en las pruebas la regla de dependencias entre crates. Detalle en [docs/architecture/overview.md](docs/architecture/overview.md).

## Desarrollo

### Requisitos

- Rust (vía [rustup](https://rustup.rs)); `rust-toolchain.toml` instala el target `wasm32-unknown-unknown`.
- Windows: Visual Studio Build Tools (C++) y WebView2 (incluido en Windows 11).
- Linux: dependencias de Tauri (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`).
- `cargo install trunk tauri-cli --locked`

### Comandos

| Tarea | Comando |
|---|---|
| Ejecutar en desarrollo | `cargo tauri dev` |
| Compilar la UI (necesario antes de `cargo test` la primera vez) | `trunk --config ui/Trunk.toml build` |
| Pruebas | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Formato | `cargo fmt --all` |
| E2E (app real) | `$env:GRYPHUS_E2E="1"; cargo test -p gryphus-e2e` (requisitos en docs/testing.md#e2e) |
| Instaladores locales | `cd src-tauri; cargo tauri build` |
| Medir el arranque (release) | `powershell -File tools/measure-startup.ps1` (instrucciones dentro del script) |
| Pegamento JS | `cd ui/js; npm install; npm test; npm run build` (solo si cambias ui/js) |

### Estructura

```
crates/      dominio, casos de uso, contrato IPC y adaptadores
src-tauri/   binario de escritorio (composition root)
ui/          frontend Leptos (WASM)
e2e/         pruebas end-to-end contra la app real
tools/       fake-agent y arch-guard (guardias de arquitectura)
docs/        diseño del sistema
```

### Publicar una versión

1. Sube `version` en `[workspace.package]` de `Cargo.toml` y haz commit.
2. `git tag vX.Y.Z; git push origin vX.Y.Z`

El workflow [Release](.github/workflows/release.yml) compila los instaladores para Windows y Linux, los adjunta al release con notas generadas y lo publica cuando todas las plataformas terminan bien.

## Licencia

[MIT](LICENSE)
