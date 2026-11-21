//! Flujos obligatorios de docs/testing.md#e2e-flujos-obligatorios contra la app
//! real. Ejecutar: `GRYPHUS_E2E=1 cargo test -p gryphus-e2e -- --nocapture`.

mod common;

use common::*;
use serde_json::json;

macro_rules! e2e {
    () => {
        if !enabled() {
            eprintln!("omitido: define GRYPHUS_E2E=1 para ejecutar las pruebas e2e");
            return;
        }
    };
}

/// Flujos 1 y 2: abrir un proyecto; abrirlo otra vez no crea otra ventana.
#[tokio::test(flavor = "multi_thread")]
async fn open_project_once_per_window() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;

    app.open_project(&fx.root()).await;
    app.wait_css(r#".tree-row[data-path="README.md"]"#).await;
    app.wait_text("#status-branch", "main").await;
    app.screenshot("01-proyecto-abierto").await;

    let res = app
        .invoke("open_project", json!({ "path": fx.root() }))
        .await;
    assert_eq!(res["other_window"], json!(false), "{res}");
    assert_eq!(app.client.windows().await.unwrap().len(), 1);

    app.quit().await;
}

/// Flujos 3 a 8: feature, agente, archivos, editor, diff, parar/reiniciar y borrar.
#[tokio::test(flavor = "multi_thread")]
async fn feature_agent_editor_and_diff() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    app.auto_confirm().await;

    // 3. Feature con worktree nuevo → existe la branch en git.
    app.click("#new-feature").await;
    app.type_into("#feature-name", "login").await;
    app.click("#create-feature").await;
    let feature = r#".feature[data-feature="login"]"#;
    app.wait_css(feature).await;
    let branches = fx
        .repo
        .git(fx.repo.root(), &["branch", "--list", "feat/login"]);
    assert!(
        branches.contains("feat/login"),
        "branch creada: {branches:?}"
    );

    // 4. Lanzar agente → banner en la terminal; escribir → eco.
    app.click(&format!("{feature} .launch")).await;
    app.wait_terminal("fake-agent ready").await;
    app.click(".terminal").await;
    app.type_active(&format!("hola{ENTER}")).await;
    app.wait_terminal("echo: hola").await;

    // 5. El agente escribe un archivo → aparece en el explorador y en el diff.
    app.type_active(&format!("::write src/nuevo.txt hola{ENTER}"))
        .await;
    app.wait_terminal("wrote src/nuevo.txt").await;
    app.screenshot("02-agente").await;
    app.click_xpath("//div[contains(@class,'tree-row') and contains(@class,'dir')][span[@class='name' and text()='src']]")
        .await;
    let file_row = r#".tree-row[data-path="src/nuevo.txt"]"#;
    app.wait_css(file_row).await;
    app.click("#tab-diff").await;
    let diff_file = r#".diff-file[data-path="src/nuevo.txt"]"#;
    app.wait_text(diff_file, "hola").await;
    app.screenshot("03-diff").await;

    // 6. Editar y guardar en el editor → el disco y el diff lo reflejan.
    app.click(file_row).await;
    app.wait_text("#editor-path", "src/nuevo.txt").await;
    app.wait_text(".cm-content", "hola").await;
    app.click(".cm-content").await;
    app.type_active(&format!("{CTRL}{END}{RELEASE} mundo"))
        .await;
    app.type_active(&format!("{CTRL}s{RELEASE}")).await;
    app.wait_css("#editor-saved").await;
    let wt = fx.repo.base().join("repo.worktrees").join("login");
    assert_eq!(
        std::fs::read_to_string(wt.join("src").join("nuevo.txt")).unwrap(),
        "hola mundo"
    );
    app.screenshot("04-editor").await;
    app.click("#tab-diff").await;
    app.wait_text(diff_file, "hola mundo").await;

    // 7. Detener → detenido; reiniciar → activo.
    app.click("#tab-agente").await;
    app.click(r#".agent button[title="Detener"]"#).await;
    app.wait_text(".agent .agent-status", "detenido").await;
    app.click(r#".agent button[title="Reiniciar"]"#).await;
    app.wait_text(".agent .agent-status", "activo").await;
    app.wait_terminal("fake-agent ready").await;

    // 8. Borrar con un agente activo → error visible; tras detenerlo, se borra.
    app.click(&format!("{feature} .feature-header button"))
        .await;
    app.wait_text(".toast", "AGENTS_STILL_RUNNING").await;
    app.click(".toast").await;
    app.click(r#".agent button[title="Detener"]"#).await;
    app.wait_text(".agent .agent-status", "detenido").await;
    app.click(&format!("{feature} .feature-header button"))
        .await;
    app.wait_gone(feature).await;
    assert!(!wt.exists(), "el worktree se eliminó del disco");
    let confirms = app.script("return window.__confirms;", vec![]).await;
    assert!(
        confirms.to_string().contains("cambios sin commit"),
        "se pidió confirmar el borrado forzado: {confirms}"
    );
    app.screenshot("05-feature-borrada").await;

    app.quit().await;
}

/// Flujo 9: cerrar y reabrir → las features persisten; los agentes quedan detenidos.
#[tokio::test(flavor = "multi_thread")]
async fn state_persists_across_restarts() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();

    let first = App::start(&fx).await;
    first.open_project(&fx.root()).await;
    first.click("#new-feature").await;
    first.type_into("#feature-name", "persist").await;
    first.click("#create-feature").await;
    let feature = r#".feature[data-feature="persist"]"#;
    first.click(&format!("{feature} .launch")).await;
    first.wait_terminal("fake-agent ready").await;
    first.quit().await;

    let second = App::start(&fx).await;
    second.open_project(&fx.root()).await;
    second.wait_css(feature).await;
    second.wait_text(".agent .agent-status", "detenido").await;
    second.screenshot("06-reabierto").await;
    second.quit().await;
}

/// Features contraíbles, nombres de agente y menú contextual (renombrar, copiar y pegar).
#[tokio::test(flavor = "multi_thread")]
async fn collapse_rename_and_copy_paste() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;

    app.click("#new-feature").await;
    app.type_into("#feature-name", "menus").await;
    app.click("#create-feature").await;
    let feature = r#".feature[data-feature="menus"]"#;
    app.click(&format!("{feature} .launch")).await;
    app.wait_terminal("fake-agent ready").await;

    // El agente tiene un nombre por defecto (no `proveedor #id`) y se renombra con clic derecho.
    let agent = format!("{feature} .agent");
    let default_name = app.text(&format!("{agent} .agent-name")).await;
    assert!(
        !default_name.trim().is_empty() && !default_name.contains('#'),
        "{default_name:?}"
    );
    app.right_click(&agent).await;
    app.click(r#".context-menu [data-action="Renombrar"]"#)
        .await;
    app.rename_to(&agent, "Revisor").await;
    app.wait_text(&format!("{agent} .agent-name"), "Revisor")
        .await;

    // Clic en el nombre de la feature: contrae y expande.
    app.click(&format!("{feature} .feature-name")).await;
    app.wait_gone(&agent).await;
    app.wait_text(&format!("{feature} .feature-count"), "1")
        .await;
    app.screenshot("07-feature-contraida").await;
    app.click(&format!("{feature} .feature-name")).await;
    app.wait_css(&agent).await;

    // Explorador (worktree de la feature): renombrar, copiar y pegar.
    let readme = r#".tree-row[data-path="README.md"]"#;
    app.right_click(readme).await;
    app.click(r#".context-menu [data-action="Renombrar"]"#)
        .await;
    app.rename_to(".pane-right", "LEEME.md").await;
    let leeme = r#".tree-row[data-path="LEEME.md"]"#;
    app.wait_css(leeme).await;

    app.right_click(leeme).await;
    app.click(r#".context-menu [data-action="Copiar"]"#).await;
    app.right_click(leeme).await;
    app.click(r#".context-menu [data-action="Pegar"]"#).await;
    app.wait_css(r#".tree-row[data-path="LEEME copia.md"]"#)
        .await;

    let wt = fx.repo.base().join("repo.worktrees").join("menus");
    assert!(!wt.join("README.md").exists(), "se renombró en disco");
    assert!(
        std::fs::read_to_string(wt.join("LEEME copia.md"))
            .unwrap()
            .contains("hola"),
        "la copia tiene el contenido"
    );
    app.screenshot("08-menu-contextual").await;
    app.quit().await;
}

/// Configuración → agente inicial: al crear una feature se lanza solo; con "Ninguno", no.
#[tokio::test(flavor = "multi_thread")]
async fn initial_agent_from_settings() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;

    let set_initial = |value: &'static str| {
        let app = &app;
        async move {
            app.click("#nav-settings").await;
            app.select_value("#set-initial-agent", value).await;
            app.click(r#".settings button[title="Cerrar (Esc)"]"#).await;
            app.wait_gone(".settings").await;
        }
    };
    let create = |name: &'static str| {
        let app = &app;
        async move {
            app.click("#new-feature").await;
            app.type_into("#feature-name", name).await;
            app.click("#create-feature").await;
            app.wait_css(&format!(r#".feature[data-feature="{name}"]"#))
                .await;
        }
    };

    set_initial("claude").await;
    create("auto").await;
    let agent = r#".feature[data-feature="auto"] .agent"#;
    app.wait_text(&format!("{agent} .agent-provider"), "claude")
        .await;
    app.wait_text(&format!("{agent} .agent-status"), "activo")
        .await;
    app.wait_terminal("fake-agent ready").await;
    app.screenshot("09-agente-inicial").await;

    // Con "Ninguno" la feature se crea sin agente (y deja la preferencia como estaba).
    set_initial("").await;
    create("manual").await;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    assert!(
        !app.exists(r#".feature[data-feature="manual"] .agent"#)
            .await,
        "sin agente inicial no se lanza ninguno"
    );
    app.quit().await;
}

/// Explorador: colores distintos por feature, Escape cancela, renombrar una carpeta
/// mantiene abierto su archivo y pegar dentro de una carpeta.
#[tokio::test(flavor = "multi_thread")]
async fn explorer_folders_and_feature_colors() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;

    for name in ["uno", "dos"] {
        app.click("#new-feature").await;
        app.type_into("#feature-name", name).await;
        app.click("#create-feature").await;
        app.wait_css(&format!(r#".feature[data-feature="{name}"]"#))
            .await;
    }
    let header = |name: &str| format!(r#".feature[data-feature="{name}"] .feature-header"#);
    let (uno, dos) = (
        app.computed_style(&header("uno"), "background-color").await,
        app.computed_style(&header("dos"), "background-color").await,
    );
    assert!(uno.starts_with("rgba"), "fondo semitransparente: {uno}");
    assert_ne!(uno, dos, "cada feature tiene su color");

    // Trabajar en el worktree de "dos".
    app.click(r#".feature[data-feature="dos"] .worktree"#).await;
    app.wait_text(".pane-right .pane-header", "feat/dos").await;
    let wt = fx.repo.base().join("repo.worktrees").join("dos");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join("src").join("a.txt"), "a\n").unwrap();

    let src = r#".tree-row[data-path="src"]"#;
    app.click(src).await;
    let file = r#".tree-row[data-path="src/a.txt"]"#;
    app.click(file).await;
    app.wait_text("#editor-path", "src/a.txt").await;

    // Escape cancela sin tocar el disco.
    app.right_click(file).await;
    app.click(r#".context-menu [data-action="Renombrar"]"#)
        .await;
    app.wait_rename_focus(".pane-right").await;
    app.type_active(&format!("{CTRL}a{RELEASE}otro.txt{ESCAPE}"))
        .await;
    app.wait_gone(".pane-right .rename-input").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(wt.join("src").join("a.txt").exists(), "Escape no renombra");

    // Renombrar la carpeta: sigue expandida y el editor apunta a la ruta nueva.
    app.right_click(src).await;
    app.click(r#".context-menu [data-action="Renombrar"]"#)
        .await;
    app.rename_to(".pane-right", "lib").await;
    app.wait_css(r#".tree-row[data-path="lib/a.txt"]"#).await;
    app.wait_text("#editor-path", "lib/a.txt").await;
    assert!(wt.join("lib").join("a.txt").exists());

    // Copiar README.md y pegarlo dentro de la carpeta.
    app.right_click(r#".tree-row[data-path="README.md"]"#).await;
    app.click(r#".context-menu [data-action="Copiar"]"#).await;
    app.right_click(r#".tree-row[data-path="lib"]"#).await;
    app.click(r#".context-menu [data-action="Pegar"]"#).await;
    app.wait_css(r#".tree-row[data-path="lib/README.md"]"#)
        .await;
    assert!(wt.join("lib").join("README.md").exists());
    app.screenshot("10-explorador-carpetas").await;
    app.quit().await;
}
