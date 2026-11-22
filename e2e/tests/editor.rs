//! Lista de archivos del diff y pestañas del editor, contra la app real.
//! Ejecutar: `GRYPHUS_E2E=1 cargo test -p gryphus-e2e --test editor -- --nocapture`.

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

fn diff_item(path: &str) -> String {
    format!(r#".diff-files-item[data-path="{path}"]"#)
}

fn tab(path: &str) -> String {
    format!(r#".editor-tab[data-path="{path}"]"#)
}

/// El panel izquierdo del diff lista los archivos cambiados; un clic salta a los
/// cambios de ese archivo (y lo despliega si estaba colapsado).
#[tokio::test(flavor = "multi_thread")]
async fn diff_file_list_jumps_to_the_file() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    // Bastantes archivos largos para que el diff tenga scroll.
    let body: String = (1..=60).map(|i| format!("línea {i}\n")).collect();
    for i in 0..30 {
        fx.repo
            .write_file(fx.repo.root(), &format!("src/f{i:02}.txt"), &body);
    }
    fx.repo
        .write_file(fx.repo.root(), "README.md", "hola\nmundo\n");

    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    app.click("#tab-diff").await;

    // Un elemento por archivo, con nombre, carpeta y contadores.
    app.wait_css(&diff_item("src/f29.txt")).await;
    let items = app
        .script(
            "return document.querySelectorAll('.diff-files-item').length;",
            vec![],
        )
        .await;
    assert_eq!(items, json!(31), "30 archivos nuevos + README.md");
    app.wait_text(&diff_item("src/f15.txt"), "f15.txt").await;
    app.wait_text(&diff_item("src/f15.txt"), "src").await;
    app.wait_text(&diff_item("src/f15.txt"), "+60").await;
    app.wait_text(&diff_item("README.md"), "+1").await;

    // Clic → el bloque del archivo queda arriba del área desplazable.
    let at_top = |path: &str| {
        let path = path.to_owned();
        let app = &app;
        async move {
            app.script(
                "const c = document.querySelector('.diff');
                 const el = document.getElementById('diff-file:' + arguments[0]);
                 if (!el) return false;
                 const d = el.getBoundingClientRect().top - c.getBoundingClientRect().top;
                 return c.scrollTop > 0 && Math.abs(d) < 3;",
                vec![json!(path)],
            )
            .await
                == json!(true)
        }
    };
    assert!(!at_top("src/f15.txt").await, "al inicio no está arriba");
    app.click(&diff_item("src/f15.txt")).await;
    wait_for("f15 arriba del diff", || at_top("src/f15.txt")).await;
    app.wait_css(&format!("{}.selected", diff_item("src/f15.txt")))
        .await;
    app.screenshot("10-diff-lista").await;

    // Un archivo colapsado se despliega al saltar a él.
    app.script(
        "document.getElementById('diff-file:src/f20.txt').open = false; return null;",
        vec![],
    )
    .await;
    app.wait_css(r#".diff-file[data-path="src/f20.txt"]:not([open])"#)
        .await;
    app.click(&diff_item("src/f20.txt")).await;
    app.wait_css(r#".diff-file[data-path="src/f20.txt"][open]"#)
        .await;
    wait_for("f20 arriba del diff", || at_top("src/f20.txt")).await;
    app.wait_gone(&format!("{}.selected", diff_item("src/f15.txt")))
        .await;

    app.quit().await;
}

/// Resaltado de sintaxis por tipo de archivo (`ui/js/src/languages.js`): cada archivo
/// abre en el editor real con su lenguaje, tanto los de `@codemirror/lang-*` como los
/// `legacy-modes`, y los nombres especiales (`Dockerfile`, `Makefile`, `.env.local`).
/// Un `.txt` sigue sin resaltado. Detecta que un lenguaje mapeado deje de cargarse
/// (p. ej. un import roto en el bundle) aunque los tests de `node --test` pasen.
#[tokio::test(flavor = "multi_thread")]
async fn editor_highlights_by_file_type() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let root = fx.repo.root();
    // La primera línea es un comentario del lenguaje con el nombre del archivo: así
    // se sabe que el editor ya muestra ese documento (y no el anterior, que también
    // estaba resaltado) y siempre hay algo que colorear.
    let files = [
        ("main.go", "// {}\npackage main\n"),
        ("config.yaml", "# {}\nclave: valor\n"),
        ("query.sql", "-- {}\nSELECT 1;\n"),
        ("Program.java", "// {}\nclass A {}\n"),
        ("app.toml", "# {}\n[seccion]\n"),
        ("Dockerfile", "# {}\nFROM alpine\n"),
        ("Dockerfile.dev", "# {}\nFROM alpine\n"),
        ("Makefile", "# {}\nall:\n"),
        (".env.local", "# {}\nCLAVE=valor\n"),
        ("deploy.sh", "# {}\necho hola\n"),
        ("estilos.scss", "// {}\n$a: 1px;\n"),
        ("vista.xml", "<!-- {} -->\n<a/>\n"),
        ("Main.kt", "// {}\nfun main() {}\n"),
        ("notas.txt", "# {}\nhola\n"),
    ];
    for (name, body) in files {
        fx.repo
            .write_file(root, name, &body.replacen("{}", name, 1));
    }
    fx.repo.commit_all(root, "archivos de varios lenguajes");

    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    let row = |path: &str| format!(r#".tree-row[data-path="{path}"]"#);
    // Tokens resaltados (CodeMirror los envuelve en <span>) si el editor ya muestra
    // `name`; `None` mientras siga el documento anterior. Una sola lectura del DOM.
    let tokens = |name: &'static str| {
        let app = &app;
        async move {
            app.script(
                "const c = document.querySelector('.cm-content');
                 if (!c || !c.textContent.includes(arguments[0])) return null;
                 return c.querySelectorAll('.cm-line span').length;",
                vec![json!(name)],
            )
            .await
            .as_u64()
        }
    };

    let (plain, highlighted) = files.split_last().unwrap();
    for &(name, _) in highlighted {
        app.click(&row(name)).await;
        app.wait_css(&format!("{}.active", tab(name))).await;
        wait_for(&format!("resaltado de {name}"), || async {
            tokens(name).await.is_some_and(|n| n > 0)
        })
        .await;
    }

    // Sin lenguaje: el texto se ve, pero sin tokens.
    let (name, _) = *plain;
    app.click(&row(name)).await;
    wait_for(&format!("{name} en el editor"), || async {
        tokens(name).await.is_some()
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(tokens(name).await, Some(0), "un .txt no se resalta");

    app.quit().await;
}

/// Pestañas del editor: abrir varias, cambiar con clic y Ctrl+Tab, conservar el
/// texto sin guardar, recargar desde disco y cerrar con el diálogo
/// Cancelar / No guardar / Guardar (el modal real, sin auto-confirmación).
#[tokio::test(flavor = "multi_thread")]
async fn editor_tabs_switch_and_close_with_save_prompt() {
    e2e!();
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    let root = fx.repo.root();
    fx.repo.write_file(root, "a.txt", "alpha");
    fx.repo.write_file(root, "b.txt", "beta");
    fx.repo.write_file(root, "docs/a.txt", "otra alpha");
    fx.repo.commit_all(root, "archivos");

    let app = App::start(&fx).await;
    app.open_project(&fx.root()).await;
    let row = |path: &str| format!(r#".tree-row[data-path="{path}"]"#);

    // Abrir tres archivos → tres pestañas, la última activa.
    app.click(&row("a.txt")).await;
    app.wait_text(".cm-content", "alpha").await;
    app.click(&row("b.txt")).await;
    app.wait_text(".cm-content", "beta").await;
    app.click_xpath("//div[contains(@class,'tree-row') and contains(@class,'dir')][span[@class='name' and text()='docs']]")
        .await;
    app.click(&row("docs/a.txt")).await;
    app.wait_text("#editor-path", "docs/a.txt").await;
    app.wait_css(&format!("{}.active", tab("docs/a.txt"))).await;
    let tabs = app
        .script(
            "return [...document.querySelectorAll('.editor-tab')].map(t => t.dataset.path);",
            vec![],
        )
        .await;
    assert_eq!(tabs, json!(["a.txt", "b.txt", "docs/a.txt"]));

    // Dos `a.txt`: cada pestaña muestra su carpeta; `b.txt` no.
    app.wait_dom_text(&format!("{} .editor-tab-hint", tab("docs/a.txt")), "docs")
        .await;
    app.wait_dom_text(&format!("{} .editor-tab-hint", tab("a.txt")), "./")
        .await;
    assert!(
        !app.exists(&format!("{} .editor-tab-hint", tab("b.txt")))
            .await
    );

    // Abrir un archivo que ya tiene pestaña no la duplica.
    app.click(&row("b.txt")).await;
    app.wait_css(&format!("{}.active", tab("b.txt"))).await;
    let count = app
        .script(
            "return document.querySelectorAll('.editor-tab').length;",
            vec![],
        )
        .await;
    assert_eq!(count, json!(3));

    // Editar a.txt → la pestaña queda marcada; cambiar de pestaña y volver conserva el texto.
    app.click(&tab("a.txt")).await;
    app.wait_text(".cm-content", "alpha").await;
    app.click(".cm-content").await;
    app.type_active(&format!("{CTRL}{END}{RELEASE} uno")).await;
    app.wait_css(&format!("{}.dirty", tab("a.txt"))).await;
    app.click(&tab("b.txt")).await;
    app.wait_text(".cm-content", "beta").await;
    assert!(!app.exists(&format!("{}.dirty", tab("b.txt"))).await);
    app.click(&tab("a.txt")).await;
    app.wait_text(".cm-content", "alpha uno").await;
    app.wait_css(&format!("{}.dirty", tab("a.txt"))).await;

    // Ctrl+Tab / Ctrl+Shift+Tab recorren las pestañas en orden circular.
    app.click(".cm-content").await;
    app.type_active(&format!("{CTRL}{TAB}{RELEASE}")).await;
    app.wait_css(&format!("{}.active", tab("b.txt"))).await;
    app.type_active(&format!("{CTRL}{SHIFT}{TAB}{RELEASE}"))
        .await;
    app.wait_css(&format!("{}.active", tab("a.txt"))).await;
    app.type_active(&format!("{CTRL}{SHIFT}{TAB}{RELEASE}"))
        .await;
    app.wait_css(&format!("{}.active", tab("docs/a.txt"))).await;

    // Una pestaña sin cambios que no está activa se recarga si cambia en disco.
    std::fs::write(root.join("b.txt"), "beta cambiado").unwrap();
    app.click(&tab("b.txt")).await;
    app.wait_text(".cm-content", "beta cambiado").await;

    // Cerrar con cambios → diálogo. Cancelar no cierra nada.
    app.click(&format!("{} .editor-tab-close", tab("a.txt")))
        .await;
    app.wait_text(".confirm-message", "a.txt").await;
    app.wait_text("#confirm-accept", "Guardar").await;
    app.wait_text("#confirm-choice-1", "No guardar").await;
    app.screenshot("11-guardar-cambios").await;
    app.click("#confirm-cancel").await;
    app.wait_gone(".confirm").await;
    app.wait_css(&format!("{}.dirty.active", tab("a.txt")))
        .await;

    // Ctrl+W → No guardar: se cierra y el disco queda igual.
    app.click(".cm-content").await;
    app.type_active(&format!("{CTRL}w{RELEASE}")).await;
    app.click("#confirm-choice-1").await;
    app.wait_gone(&tab("a.txt")).await;
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "alpha"
    );
    // Se activa la pestaña de su derecha.
    app.wait_css(&format!("{}.active", tab("b.txt"))).await;

    // Reabrir: se lee de disco de nuevo (sin el texto descartado). Editar → Guardar.
    app.click(&row("a.txt")).await;
    app.wait_css(&format!("{}.active", tab("a.txt"))).await;
    app.wait_text(".cm-content", "alpha").await;
    assert!(!app.text(".cm-content").await.contains("uno"));
    app.click(".cm-content").await;
    app.type_active(&format!("{CTRL}{END}{RELEASE} dos")).await;
    app.wait_css(&format!("{}.dirty", tab("a.txt"))).await;
    app.click(&format!("{} .editor-tab-close", tab("a.txt")))
        .await;
    app.click("#confirm-accept").await;
    app.wait_gone(&tab("a.txt")).await;
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "alpha dos"
    );

    // Cerrar sin cambios no pregunta; al cerrar la última no queda editor.
    app.click(&format!("{} .editor-tab-close", tab("b.txt")))
        .await;
    app.wait_gone(&tab("b.txt")).await;
    assert!(!app.exists(".confirm").await);
    app.click(&format!("{} .editor-tab-close", tab("docs/a.txt")))
        .await;
    app.wait_gone(".editor-tabs").await;
    app.wait_text("#editor-path", "Abre un archivo").await;

    app.quit().await;
}
