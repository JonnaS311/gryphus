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
