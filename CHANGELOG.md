# Changelog

Cambios notables de cada versión de Gryphus. Los instaladores están en
[Releases](https://github.com/JonnaS311/gryphus/releases).

## [0.3.0] - 2026-11-22

### Novedades

- **Los agentes retoman su conversación.** Cada agente guarda la sesión de su CLI y,
  al reabrir el proyecto (o tras un cierre inesperado), los que seguían activos se
  relanzan solos continuando donde iban. Reiniciar un agente detenido también retoma
  la suya. Funciona con Claude Code (`--session-id` / `--resume`), OpenCode
  (`--session`) y Codex (`resume`, localizando la conversación de su worktree).
- **Arrastrar desde el explorador:** una fila se puede soltar en otra carpeta (la
  mueve, sin sobrescribir nunca), en el editor (abre el archivo) o en la terminal del
  agente (escribe su ruta, relativa si el agente trabaja en ese worktree). Una
  etiqueta junto al puntero indica qué pasará; Esc cancela.
- **Explorador:** "Mostrar en el Explorador / Finder", "Copiar ruta" y "Copiar ruta
  relativa" en el menú contextual (también sobre la raíz, en la zona vacía).
- **Editor:** resaltado de sintaxis para unos 40 lenguajes (Rust, JS/TS, Python, Go,
  Java, C/C++, C#, Kotlin, PHP, Ruby, SQL, YAML, TOML, Dockerfile, PowerShell, shell…),
  reconociendo variantes como `Dockerfile.dev` o `.env.local`.
- **Diff con colores de sintaxis**, con la misma paleta que el editor. La versión
  vieja y la nueva de cada hunk se analizan por separado para que mezclarlas no
  rompa el resaltado.

### Mejoras

- El diff ya no se desborda: las líneas largas se parten dentro de la caja de su
  archivo, alineadas tras el signo `+`/`-`, sin scroll horizontal.
- Terminal del agente actualizada a xterm.js 6.
- Al cambiar de agente y volver, su pantalla se recupera aunque el scrollback ya no
  la contenga (p. ej. tras un indicador de progreso largo).
- Pegar una captura de pantalla en la terminal pide al CLI que lea la imagen
  (Alt+V para Claude Code en Windows, Ctrl+V en el resto).

### Correcciones

- Reborde negro alrededor de la terminal del agente (introducido con xterm.js 6).

## [0.2.0] - 2026-11-22

### Novedades

- **Versión para macOS:** `.dmg` para Apple Silicon e Intel, menú nativo, atajos con
  ⌘ y PATH del shell de login al abrir desde Finder (agentes de Homebrew, npm,
  `~/.local/bin`).
- **Paneles acoplables:** Features y Explorador se redimensionan y cambian de lado;
  Agente / Editor / Diff se reorganizan arrastrando sus pestañas. La disposición se
  recuerda.
- **Color por feature**, elegido con clic derecho en su cabecera.
- **Aviso cuando un agente termina:** la feature parpadea y aparece un botón rojo
  cuando el agente queda esperando instrucciones (silencio tras una orden, campana u
  OSC 9 / OSC 777).
- **Reabrir el último proyecto** al arrancar.
- **Archivos del sistema:** soltarlos o pegarlos en el explorador los copia al
  worktree; en la terminal del agente escribe sus rutas.

### Correcciones

- Crear o eliminar features cuando sus worktrees se habían borrado a mano ya no
  falla (`git worktree prune` y reutilización de la branch si está libre).

## [0.1.0] - 2026-11-21

Primera versión pública.

[0.3.0]: https://github.com/JonnaS311/gryphus/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/JonnaS311/gryphus/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/JonnaS311/gryphus/releases/tag/v0.1.0
