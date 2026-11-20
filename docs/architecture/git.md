# Integración git (`gryphus-git`)

Implementa `GitPort` con `CliGit`, que ejecuta el binario `git` (decisión y mediciones en [ADR-005](../decisions.md#adr-005--git-sobre-la-cli-revisada-en-el-hito-3)).

| Operación | Comando |
|---|---|
| Info del repo | `rev-parse --show-toplevel` + `symbolic-ref --short HEAD` |
| Branches, upstream, ahead/behind, checkout | `for-each-ref --format=%(refname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(worktreepath)` |
| Worktrees | `worktree list --porcelain -z` |
| Status | `status --porcelain=v2 -z --untracked-files=all` |
| Diff | `diff --no-renames -U3 <base> --` + `ls-files --others --exclude-standard -z` (los archivos sin seguimiento se añaden como "todo añadido"; binarios o > 1 MiB se marcan `binary`) |
| Merge-base | `merge-base HEAD <branch>` |
| Crear/eliminar worktree | `worktree add [-b]` / `worktree remove [--force]` |

Detalles del adaptador:
- Todas las invocaciones usan `-c core.quotepath=false -c color.ui=false`, `LC_ALL=C` (para clasificar los errores por su texto en inglés) y `GIT_TERMINAL_PROMPT=0`. En Windows se lanzan sin ventana de consola.
- Las lecturas usan `GIT_OPTIONAL_LOCKS=0`.
- Los errores se clasifican en `PortErrorKind` según el stderr (`classify_failure`, con pruebas sobre mensajes reales).
- Una `base` que empiece por `-` se rechaza (evita inyectar opciones).
- Los parsers son funciones puras con pruebas sobre salidas fijas, incluidas rutas con comillas C, espacios y no-ASCII.

`CliGit` pasa **los 16 casos del contrato** contra repos reales (ver [testing.md](../testing.md#pruebas-de-contrato)) y 10 casos propios. Benchmark: `cargo bench -p gryphus-git`.

## Worktrees y features

- **Crear feature con worktree nuevo:** `git worktree add <ruta> -b <branch> <base>`.
  - Ruta por defecto: `<padre-del-repo>/<nombre-repo>.worktrees/<feature>` (fuera del repo para que no aparezca en el árbol ni en `git status`). Configurable.
- **Adjuntar worktree existente:** se elige uno de `git worktree list` que no pertenezca a otra feature.
- **Eliminar feature:** opcionalmente elimina sus worktrees (`git worktree remove`); si hay cambios sin commit, la UI exige confirmación y se usa `--force` solo con ella. La branch nunca se borra automáticamente.
- **Worktrees externos:** si alguien crea o borra worktrees fuera de Gryphus, el watcher sobre `.git/worktrees` publica `GitChanged` y el panel se reconcilia (los worktrees desaparecidos se marcan como "faltante" en vez de romper la feature).

## Información git visible

- **Barra de estado:** branch del worktree activo, ahead/behind respecto al upstream, nº de archivos modificados.
- **Panel izquierdo:** cada worktree muestra su branch y un indicador de cambios.
- **Explorador:** marcas de status por archivo (M, A, D, ?).
- **Vista diff:** cambios del worktree respecto a `HEAD` (sin commit) o respecto al `merge-base` con la branch base (todo lo que hizo la feature). Ver un diff por agente = ver el diff de su worktree.

## Actualización eficiente

```
notify (fs) ──debounce 150 ms──▶ FilesChanged ──▶ GitRefresher (por worktree)
                                                   ├─ si ya hay un status en curso: marca "sucio" y sale
                                                   └─ al terminar: si está sucio, recalcula una vez más
                                                   ──▶ GitChanged ──▶ UI
```

Nunca hay más de un `status` en curso por worktree, y nunca se pierde un cambio.
