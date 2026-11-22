# Actualizaciones automáticas

> Estado: **propuesta, sin implementar.** Este documento describe cómo añadir auto-actualización a Gryphus y qué hay que decidir antes de hacerlo.

## Objetivo

Que al abrir Gryphus, la app detecte si hay una versión más reciente publicada, la descargue, verifique su firma, la instale y se reinicie, sin que el usuario tenga que bajar el instalador a mano.

## Cómo encaja con lo que ya existe

Gryphus usa **Tauri v2**, compila los instaladores con `tauri-action` y los publica en **GitHub Releases** (`.github/workflows/release.yml`). Tauri tiene un plugin oficial, `tauri-plugin-updater`, que funciona justo sobre ese flujo:

```
Usuario abre Gryphus
      │
      ▼
GET latest.json  ──►  ¿versión > la instalada?  ── no ──► sigue normal
(GitHub Release)              │ sí
                              ▼
               descarga el artefacto de su plataforma
                              │
                              ▼
               verifica la firma con la clave pública embebida
                              │
                              ▼
               instala  ──►  reinicia (tauri-plugin-process)
```

`latest.json` es un manifiesto que genera `tauri-action` y que trae, para cada plataforma, la versión, la URL del artefacto y su firma.

## Qué hace falta

### 1. Claves de firma del updater

Son obligatorias: el updater no instala nada que no esté firmado. Estas claves son de Tauri y **no tienen nada que ver** con firmar el código para Apple o Microsoft.

```
cargo tauri signer generate -w ~/.tauri/gryphus.key
```

- La **clave pública** va en `src-tauri/tauri.conf.json` (ver paso 2).
- La **clave privada** y su contraseña van como secrets del repositorio en GitHub:
  - `TAURI_SIGNING_PRIVATE_KEY`
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- **Guarda la clave privada en un lugar seguro.** Si se pierde, las versiones ya instaladas no podrán actualizarse nunca más: rechazarán cualquier artefacto firmado con una clave nueva.

### 2. Configuración de Tauri

En `src-tauri/tauri.conf.json`:

```json
"bundle": {
  "createUpdaterArtifacts": true
},
"plugins": {
  "updater": {
    "pubkey": "<contenido de la clave pública>",
    "endpoints": [
      "https://github.com/JonnaS311/gryphus/releases/latest/download/latest.json"
    ]
  }
}
```

En `src-tauri/capabilities/default.json` hay que añadir los permisos:

- `updater:default`
- `process:allow-restart`

### 3. Dependencias

En el workspace y en `src-tauri/Cargo.toml`:

- `tauri-plugin-updater`
- `tauri-plugin-process` (para relanzar la app después de instalar)

Los dos plugins se registran en el builder de Tauri:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    // ...
```

### 4. CI (`release.yml`)

En el paso de `tauri-action` solo hay que pasar los dos secrets como variables de entorno:

```yaml
- uses: tauri-apps/tauri-action@action-v1.0.0
  env:
    GITHUB_TOKEN: ${{ github.token }}
    TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
```

Con eso, la action firma los artefactos y sube el `latest.json` al release sin más configuración.

### 5. Lógica al arrancar

Esquema mínimo en Rust:

```rust
use tauri_plugin_updater::UpdaterExt;

async fn check_for_update(app: tauri::AppHandle) -> tauri_plugin_updater::Result<()> {
    if let Some(update) = app.updater()?.check().await? {
        update
            .download_and_install(|_downloaded, _total| {}, || {})
            .await?;
        app.restart();
    }
    Ok(())
}
```

La comprobación tiene que hacerse en segundo plano, sin bloquear la apertura de la ventana, y si falla (por ejemplo, sin red) se ignora.

## Puntos a tener en cuenta

### Pre-releases y el endpoint `/releases/latest`

El workflow actual marca como **pre-release** todas las versiones `0.x` y las que llevan sufijo (`-rc.1`). La URL `/releases/latest/` de GitHub **se salta los pre-releases**, así que con la configuración actual el endpoint no encontraría ninguna versión.

Opciones:

| Opción | Pros | Contras |
|---|---|---|
| Dejar de marcar `0.x` como pre-release (mantener solo los sufijos `-rc`, `-beta`) | Lo más sencillo; no cambia nada más | Las `0.x` aparecen como versiones estables en GitHub |
| Publicar `latest.json` en otro sitio (GitHub Pages, un gist) desde el workflow | Control total sobre qué versión se ofrece; permite canales (estable / beta) | Un paso más en CI y otra pieza que mantener |

### No reiniciar a la fuerza

Gryphus tiene agentes y terminales (PTY) corriendo. Reiniciar en medio de una sesión mataría esos procesos y se perdería trabajo. En lugar de `download_and_install` + `restart` directos, conviene:

1. Comprobar la actualización al abrir.
2. Descargarla en segundo plano.
3. Mostrar un aviso no intrusivo: *"Nueva versión X disponible — Reiniciar para actualizar"*.
4. Instalar cuando el usuario acepte, o al cerrar la app.

Si se reinicia con agentes vivos, debería pedirse confirmación, igual que al cerrar una ventana con agentes activos.

### Comportamiento por plataforma

| Plataforma | Artefacto que se actualiza | Notas |
|---|---|---|
| Windows | Instalador NSIS / MSI | Funciona bien. SmartScreen sigue avisando porque no hay firma de código, igual que hoy. El instalador corre en modo pasivo. |
| macOS | `.app.tar.gz` | Con la firma ad-hoc actual (`signingIdentity: "-"`) suele funcionar, porque lo que descarga el updater no lleva el atributo de cuarentena. |
| Linux | **Solo AppImage** | El `.deb` no se puede actualizar solo: lo gestiona el gestor de paquetes del sistema. Quien instale con `.deb` seguirá actualizando a mano. |

### Primera instalación con updater

Quien tenga instalada una versión sin el updater (la 0.2.0 o anteriores) tiene que instalar **a mano** la primera versión que lo traiga. A partir de ahí, las actualizaciones son automáticas.

## Decisiones pendientes

1. **Pre-releases:** ¿dejar de marcar `0.x` como pre-release o alojar `latest.json` aparte?
2. **Experiencia de usuario:** ¿aviso con botón "Reiniciar para actualizar", o instalación silenciosa al cerrar?
3. **Canales:** ¿hace falta un canal beta separado del estable? (Solo viable si `latest.json` se aloja aparte.)

## Referencias

- Plugin updater de Tauri v2: <https://v2.tauri.app/plugin/updater/>
- Plugin process de Tauri v2: <https://v2.tauri.app/plugin/process/>
- `tauri-action` y `latest.json`: <https://github.com/tauri-apps/tauri-action>
