//! Archivos de fuera de la app en el explorador y en la terminal del agente:
//! soltarlos (arrastrar desde el explorador del sistema) y pegarlos (Ctrl+C en el
//! sistema → Ctrl+V / "Pegar" en Gryphus), contra la app real.
//! Ejecutar: `GRYPHUS_E2E=1 cargo test -p gryphus-e2e --test explorer -- --nocapture`.

mod common;

use std::path::{Path, PathBuf};

use common::*;

macro_rules! e2e {
    () => {
        if !enabled() {
            eprintln!("omitido: define GRYPHUS_E2E=1 para ejecutar las pruebas e2e");
            return;
        }
    };
}

fn row(path: &str) -> String {
    format!(r#".tree-row[data-path="{path}"]"#)
}

/// Archivos "del sistema", fuera del repositorio: `a.txt`, `con espacio.txt` y
/// la carpeta `carpeta/b.txt`.
fn outside_files(fx: &Fixture) -> PathBuf {
    let dir = fx.repo.base().join("fuera");
    std::fs::create_dir_all(dir.join("carpeta")).unwrap();
    std::fs::write(dir.join("a.txt"), "de fuera").unwrap();
    std::fs::write(dir.join("con espacio.txt"), "x").unwrap();
    std::fs::write(dir.join("carpeta").join("b.txt"), "b").unwrap();
    dir
}

/// Deja que lo pegado llegue al agente antes de pulsar Enter.
async fn settle() {
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
}

/// Como `App::wait_terminal`, sin tener en cuenta los saltos por ancho de línea
/// (las rutas largas ocupan varias filas de la terminal).
async fn wait_terminal_unwrapped(app: &App, needle: &str) {
    let squash = |s: &str| s.split_whitespace().collect::<String>();
    let needle = squash(needle);
    wait_for(&format!("{needle:?} en la terminal"), || async {
        squash(&app.terminal_text().await).contains(&needle)
    })
    .await;
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Soltar archivos y carpetas del sistema sobre el explorador: en una carpeta van
/// dentro de ella, sobre un archivo van a su carpeta y en la zona vacía a la raíz;
/// si el nombre existe se crea `nombre copia`. Mientras se arrastra se resalta el destino.
#[tokio::test(flavor = "multi_thread")]
async fn dropping_files_on_the_explorer_copies_them() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let fuera = outside_files(&fx);
    let root = fx.repo.root().to_path_buf();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs").join("leeme.txt"), "l").unwrap();

    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    app.wait_css(&row("docs")).await;

    // Arrastrar por encima resalta la carpeta destino; salir quita el resaltado.
    let a = fuera.join("a.txt");
    app.drag_files("tauri://drag-over", &row("docs"), std::slice::from_ref(&a))
        .await;
    app.wait_css(&format!("{}.drop-target", row("docs"))).await;
    app.drag_files("tauri://drag-leave", &row("docs"), &[])
        .await;
    app.wait_gone(".drop-target").await;

    // Sobre una carpeta: archivo y carpeta completa dentro de ella (y se expande).
    app.drop_files(&row("docs"), &[a.clone(), fuera.join("carpeta")])
        .await;
    app.wait_css(&row("docs/a.txt")).await;
    app.wait_css(&row("docs/carpeta")).await;
    assert_eq!(read(&root.join("docs").join("a.txt")), "de fuera");
    assert_eq!(read(&root.join("docs").join("carpeta").join("b.txt")), "b");
    assert!(
        a.exists(),
        "el original sigue en su sitio (se copia, no se mueve)"
    );

    // Sobre un archivo: a su carpeta; el nombre ya existe → `a copia.txt`.
    app.drop_files(&row("docs/leeme.txt"), std::slice::from_ref(&a))
        .await;
    app.wait_css(&row("docs/a copia.txt")).await;

    // En la zona vacía del explorador: a la raíz.
    app.drop_files(".pane-right .pane-header", &[fuera.join("con espacio.txt")])
        .await;
    app.wait_css(&row("con espacio.txt")).await;
    assert!(root.join("con espacio.txt").exists());
    app.screenshot("11-explorador-soltar").await;
    app.quit().await;
}

/// Portapapeles del sistema: archivos copiados fuera de la app se pegan con el menú
/// "Pegar" o con Ctrl+V en el explorador, y "Copiar" deja el archivo en el
/// portapapeles del sistema (para pegarlo en un agente o en otra app).
#[tokio::test(flavor = "multi_thread")]
async fn system_clipboard_files_paste_into_the_explorer() {
    e2e!();
    // Se conserva vivo durante la prueba: en X11 el contenido se pierde al soltarlo.
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        eprintln!("omitido: sin portapapeles del sistema");
        return;
    };
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let fuera = outside_files(&fx);
    let root = fx.repo.root().to_path_buf();
    std::fs::create_dir_all(root.join("docs")).unwrap();

    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    app.wait_css(&row("docs")).await;

    // "Pegar" del menú contextual sobre una carpeta.
    clipboard
        .set()
        .file_list(&[fuera.join("a.txt"), fuera.join("carpeta")])
        .unwrap();
    app.right_click(&row("docs")).await;
    app.click(r#".context-menu [data-action="Pegar"]"#).await;
    app.wait_css(&row("docs/a.txt")).await;
    app.wait_css(&row("docs/carpeta")).await;
    assert_eq!(read(&root.join("docs").join("carpeta").join("b.txt")), "b");

    // Ctrl+V con el foco en el explorador pega en la carpeta pulsada.
    app.click(&row("docs")).await;
    app.type_active(&format!("{CTRL}v{RELEASE}")).await;
    app.wait_css(&row("docs/a copia.txt")).await;

    // "Pegar" en la zona vacía: a la raíz.
    app.right_click(".pane-right .pane-header").await;
    app.click(r#".context-menu [data-action="Pegar"]"#).await;
    app.wait_css(&row("a.txt")).await;

    // "Copiar" en Gryphus pone la ruta absoluta en el portapapeles del sistema…
    app.right_click(&row("README.md")).await;
    app.click(r#".context-menu [data-action="Copiar"]"#).await;
    let readme = std::fs::canonicalize(root.join("README.md")).unwrap();
    wait_for("README.md en el portapapeles del sistema", || {
        let files = clipboard.get().file_list().unwrap_or_default();
        let ok = files
            .iter()
            .any(|f| std::fs::canonicalize(f).ok().as_ref() == Some(&readme));
        async move { ok }
    })
    .await;
    // …y pegarlo lo duplica como siempre.
    app.right_click(".pane-right .pane-header").await;
    app.click(r#".context-menu [data-action="Pegar"]"#).await;
    app.wait_css(&row("README copia.md")).await;
    app.screenshot("12-explorador-portapapeles").await;
    app.quit().await;
}

/// En la terminal del agente, soltar o pegar archivos escribe sus rutas (entre
/// comillas si tienen espacios) y pegar texto sigue funcionando.
#[tokio::test(flavor = "multi_thread")]
async fn dropping_and_pasting_files_into_the_agent_writes_their_paths() {
    e2e!();
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        eprintln!("omitido: sin portapapeles del sistema");
        return;
    };
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let fuera = outside_files(&fx);
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;

    app.click("#new-feature").await;
    app.type_into("#feature-name", "adjuntos").await;
    app.click("#create-feature").await;
    let feature = r#".feature[data-feature="adjuntos"]"#;
    app.click(&format!("{feature} .launch")).await;
    app.wait_terminal("fake-agent ready").await;

    // Soltar sobre la terminal: rutas como argumentos, con comillas si hace falta.
    let a = fuera.join("a.txt");
    let spaced = fuera.join("con espacio.txt");
    app.drag_files("tauri://drag-over", ".terminal", std::slice::from_ref(&a))
        .await;
    app.wait_css(".terminal.drop-target").await;
    app.drop_files(".terminal", &[a.clone(), spaced.clone()])
        .await;
    app.wait_gone(".terminal.drop-target").await;
    settle().await;
    app.click(".terminal").await;
    app.type_active(ENTER).await;
    let quote = |p: &Path| {
        let p = p.to_string_lossy();
        if cfg!(windows) {
            format!("\"{p}\"")
        } else {
            format!("'{p}'")
        }
    };
    wait_terminal_unwrapped(&app, &format!("echo: {} {}", a.display(), quote(&spaced))).await;

    // Pegar con archivos en el portapapeles del sistema (sin texto).
    let b = fuera.join("carpeta").join("b.txt");
    clipboard.set().file_list(std::slice::from_ref(&b)).unwrap();
    app.paste_in_terminal(None).await;
    settle().await;
    app.type_active(ENTER).await;
    wait_terminal_unwrapped(&app, &format!("echo: {}", b.display())).await;

    // Pegar texto sigue llegando al agente.
    app.paste_in_terminal(Some("texto pegado")).await;
    settle().await;
    app.type_active(ENTER).await;
    app.wait_terminal("echo: texto pegado").await;
    app.screenshot("13-agente-adjuntos").await;
    app.quit().await;
}
