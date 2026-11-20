# Integración git (`gryphus-git`)

Implementa `GitPort`. Estrategia híbrida para priorizar velocidad sin perder fiabilidad:

| Operación | Implementación | Por qué |
|---|---|---|
| Info del repo, branches, ahead/behind | `gix` (gitoxide) | En proceso, sin lanzar procesos; muy rápido |
| Listar worktrees | `gix` | Lectura de `.git/worktrees` |
| Status | `gix` (status con índice) | Se ejecuta a menudo; evitar `fork` |
| Diff | `gix` + `imara-diff` | Rápido y sin dependencias de C |
| `worktree add/remove`, crear branch | **CLI `git`** | Mutaciones: usamos la herramienta de referencia para no corromper nada |

Si alguna lectura de `gix` no cubre un caso (p. ej. configuraciones raras), el adaptador cae a la CLI. Las dos implementaciones pasan **la misma suite de pruebas de contrato** (ver [testing.md](../testing.md#pruebas-de-contrato)).

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
