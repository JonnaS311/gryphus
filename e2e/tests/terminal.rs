//! La terminal del agente contra la app real: cambiar de agente y pegar imágenes.
//! Ejecutar: `GRYPHUS_E2E=1 cargo test -p gryphus-e2e --test terminal -- --nocapture`.

mod common;

use common::*;

macro_rules! e2e {
    () => {
        if !enabled() {
            eprintln!("omitido: define GRYPHUS_E2E=1 para ejecutar las pruebas e2e");
            return;
        }
    };
}

fn agent(id: &str) -> String {
    format!(r#".agent[data-agent="{id}"]"#)
}

/// Ids de los agentes en el orden del panel.
async fn agent_ids(app: &App) -> Vec<String> {
    let ids = app
        .script(
            "return [...document.querySelectorAll('.agent')].map(e => e.dataset.agent);",
            vec![],
        )
        .await;
    serde_json::from_value(ids).unwrap_or_default()
}

/// Crea la feature `name` y lanza en ella un agente `claude`; devuelve su id.
async fn launch_agent(app: &App, feature: &str) -> String {
    let before = agent_ids(app).await;
    let scope = format!(r#".feature[data-feature="{feature}"]"#);
    app.select_value(&format!("{scope} select.provider"), "claude")
        .await;
    app.click(&format!("{scope} .launch")).await;
    wait_for("el agente nuevo en el panel", || async {
        agent_ids(app).await.len() > before.len()
    })
    .await;
    let id = agent_ids(app)
        .await
        .into_iter()
        .find(|i| !before.contains(i))
        .expect("agente nuevo");
    app.wait_css(&format!("{}.selected", agent(&id))).await;
    app.wait_text(&format!("{} .agent-status", agent(&id)), "activo")
        .await;
    id
}

/// Al volver a un agente, su pantalla se ve aunque la salida guardada ya no la
/// contenga: un TUI redibuja en el sitio (indicador de progreso) y el ring buffer
/// acaba guardando solo esos cuadros. Lo que la recupera es el resize que la
/// terminal manda al conectarse (ConPTY repinta la pantalla completa); sin él, la
/// terminal se queda solo con el indicador.
#[tokio::test(flavor = "multi_thread")]
async fn switching_agents_restores_the_screen() {
    e2e!();
    let _serial = SERIAL.lock().await;
    // Un scrollback mínimo: con unos cientos de cuadros del indicador ya se descarta
    // lo anterior, como pasa con 2 MB en una sesión larga de Claude Code.
    let fx = Fixture::new().with_env("GRYPHUS_SCROLLBACK_BYTES", "256");
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;

    app.click("#new-feature").await;
    app.type_into("#feature-name", "cambios").await;
    app.click("#create-feature").await;
    app.wait_css(r#".feature[data-feature="cambios"]"#).await;

    let first = launch_agent(&app, "cambios").await;
    app.wait_terminal("fake-agent ready").await;
    app.click(".terminal").await;
    app.type_active(&format!("PANTALLA-UNO{ENTER}")).await;
    app.wait_terminal("echo: PANTALLA-UNO").await;
    app.type_active(&format!("::spin 300{ENTER}")).await;
    app.wait_terminal("spin done").await;

    // Otro agente: la terminal muestra el suyo.
    let second = launch_agent(&app, "cambios").await;
    wait_for("la terminal del segundo agente", || async {
        let text = app.terminal_text().await;
        text.contains("fake-agent ready") && !text.contains("PANTALLA-UNO")
    })
    .await;

    // Premisa: el scrollback del primero ya no contiene su pantalla (solo cuadros
    // del indicador). Leerlo conecta la ventana a ese canal; el clic de abajo la
    // vuelve a conectar a la terminal.
    let scrollback = app
        .script(
            "return new Promise(done => {
                 const ch = new window.__TAURI__.core.Channel();
                 ch.onmessage = m => done(new TextDecoder().decode(new Uint8Array(m).subarray(1)));
                 window.__TAURI__.core.invoke('attach_agent', { req: { agent: Number(arguments[0]) }, channel: ch });
             });",
            vec![serde_json::json!(first)],
        )
        .await;
    let scrollback = scrollback.as_str().unwrap_or_default();
    assert!(
        scrollback.contains("spin done") && !scrollback.contains("PANTALLA-UNO"),
        "scrollback truncado: {scrollback:?}"
    );

    // De vuelta al primero: se ve todo lo que tenía en pantalla.
    for _ in 0..3 {
        app.click(&agent(&first)).await;
        app.wait_css(&format!("{}.selected", agent(&first))).await;
        app.wait_terminal("echo: PANTALLA-UNO").await;
        app.wait_terminal("spin done").await;
        app.screenshot("terminal-vuelta-al-agente").await;

        app.click(&agent(&second)).await;
        app.wait_css(&format!("{}.selected", agent(&second))).await;
        wait_for("la terminal del segundo agente", || async {
            !app.terminal_text().await.contains("PANTALLA-UNO")
        })
        .await;
    }
    app.quit().await;
}

/// Sin reborde negro alrededor de la terminal: en xterm 6 `.xterm-viewport` cubre
/// también el padding del contenedor con fondo #000, y sus sombras de scroll son
/// negras. Nada visible dentro de la terminal debe pintarse de negro.
#[tokio::test(flavor = "multi_thread")]
async fn terminal_has_no_black_border() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    app.click("#new-feature").await;
    app.type_into("#feature-name", "borde").await;
    app.click("#create-feature").await;
    app.wait_css(r#".feature[data-feature="borde"]"#).await;
    launch_agent(&app, "borde").await;
    app.wait_terminal("fake-agent ready").await;
    // Con scrollback (más líneas que filas) para que xterm active su scroll.
    app.click(".terminal").await;
    for i in 0..60 {
        app.type_active(&format!("linea-{i}{ENTER}")).await;
    }
    app.wait_terminal("echo: linea-59").await;
    app.screenshot("terminal-sin-borde").await;

    let black = app
        .script(
            "const t = document.querySelector('.terminal');
             return [t, ...t.querySelectorAll('*')]
               .filter(el => {
                 const cs = getComputedStyle(el);
                 const r = el.getBoundingClientRect();
                 if (r.width < 2 || r.height < 2 || cs.display === 'none') return false;
                 return cs.backgroundColor === 'rgb(0, 0, 0)'
                   || /rgb\\(0, 0, 0\\)/.test(cs.boxShadow);
               })
               .map(el => `${el.tagName}.${el.className}`);",
            vec![],
        )
        .await;
    assert_eq!(black, serde_json::json!([]), "elementos negros en la terminal");
    app.quit().await;
}

/// Pegar sin texto ni archivos en el portapapeles (una captura de pantalla) pide al
/// CLI que lea la imagen: Claude Code en Windows con Alt+V, si no con Ctrl+V.
#[tokio::test(flavor = "multi_thread")]
async fn pasting_an_image_asks_the_agent_to_read_the_clipboard() {
    e2e!();
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        eprintln!("omitido: sin portapapeles del sistema");
        return;
    };
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;

    app.click("#new-feature").await;
    app.type_into("#feature-name", "imagen").await;
    app.click("#create-feature").await;
    app.wait_css(r#".feature[data-feature="imagen"]"#).await;
    launch_agent(&app, "imagen").await;
    app.wait_terminal("fake-agent ready").await;

    clipboard.clear().unwrap();
    app.record_agent_input().await;
    app.paste_in_terminal(None).await;
    let expected = if cfg!(windows) { "\x1bv" } else { "\u{16}" };
    let start = std::time::Instant::now();
    while app.agent_inputs().await.is_empty() && start.elapsed() < TIMEOUT {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(app.agent_inputs().await, vec![expected.to_owned()]);
    app.quit().await;
}
