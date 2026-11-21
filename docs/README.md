# Gryphus — Documentación

Gryphus es un entorno de desarrollo de agentes (opencode, Claude Code, Codex) construido con **Rust + Tauri**. Tiene dos objetivos: **simplicidad** y **eficiencia**.

> Estado: **implementado** (hitos 0–10 del [roadmap](roadmap.md)). Esta carpeta sigue siendo la fuente de verdad del diseño.

## Qué hace

- **Una ventana = un proyecto** (un repositorio git). Si abres un proyecto que ya está abierto, se enfoca su ventana en lugar de abrir otra.
- **Panel izquierdo — Agentes:** el trabajo se agrupa por **feature**. Cada feature tiene **1 o más worktrees** y **0 o más agentes**. Cada agente corre dentro de uno de los worktrees de su feature.
- **Centro — Vista principal:** muestra una de tres cosas: la **terminal de un agente**, el **editor** de un archivo o el **diff** de un worktree.
- **Panel derecho — Explorador:** las carpetas y archivos del worktree activo.
- **Git integrado:** branches, worktrees, estado (status), ahead/behind y diffs.

```
┌──────────────┬───────────────────────────────┬──────────────┐
│ FEATURES     │  [Agente] [Editor] [Diff]     │ EXPLORADOR   │
│ ▾ login      │                               │ ▾ src/       │
│   ⎇ feat/login (wt)                          │   main.rs    │
│   ● claude   │   salida de la terminal /     │   lib.rs     │
│   ● codex    │   editor / diff               │ ▸ tests/     │
│ ▸ pagos      │                               │ Cargo.toml   │
├──────────────┴───────────────────────────────┴──────────────┤
│ ⎇ main · 2 worktrees · ↑1 ↓0 · 3 archivos modificados        │
└─────────────────────────────────────────────────────────────┘
```

## Índice

| Documento | Contenido |
|---|---|
| [architecture/gryphus-architecture.html](architecture/gryphus-architecture.html) | Diagrama interactivo del sistema (fuente: `gryphus.architecture.json`) |
| [architecture/overview.md](architecture/overview.md) | Capas, crates, regla de dependencias y cómo se comunican |
| [architecture/domain.md](architecture/domain.md) | Modelo de dominio, invariantes y puertos (traits) |
| [architecture/ipc.md](architecture/ipc.md) | Contrato entre la UI y Rust: comandos, Channels y eventos |
| [architecture/agents.md](architecture/agents.md) | Runtime de agentes: PTY, supervisor y configuración de proveedores |
| [architecture/git.md](architecture/git.md) | Integración git: branches, worktrees, status y diff |
| [architecture/ui.md](architecture/ui.md) | Frontend: layout, estado y componentes |
| [testing.md](testing.md) | Estrategia de pruebas (TDD, unitarias, integración, e2e) |
| [decisions.md](decisions.md) | Decisiones de arquitectura (ADRs) y sus alternativas |
| [roadmap.md](roadmap.md) | Orden de implementación por hitos |

## Cómo regenerar el diagrama

El diagrama se genera con la skill `archify` desde `docs/architecture/gryphus.architecture.json`. Si cambia la arquitectura, edita el JSON primero y vuelve a generar el HTML. El diagrama y estos documentos deben cambiar en el mismo commit.
