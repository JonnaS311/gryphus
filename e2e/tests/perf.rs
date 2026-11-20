//! Rendimiento de la app real (build de release) contra los presupuestos de
//! docs/architecture/overview.md. Imprime una tabla y guarda target/perf.json.
//! Ejecutar: `GRYPHUS_E2E=1 cargo test -p gryphus-e2e --test perf -- --nocapture`.
//!
//! Solo afirma límites holgados (2× el presupuesto) para no ser frágil en CI;
//! los valores reales quedan en el informe.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{json, Value};

/// Mediana y p95 (ms) de `n` invocaciones de un comando, medidas en la WebView.
async fn ipc_timing(app: &App, cmd: &str, req: Value, n: u32) -> Value {
    app.script(
        r#"
        const [cmd, req, n] = arguments;
        return (async () => {
            const t = [];
            for (let i = 0; i < n; i++) {
                const s = performance.now();
                await window.__TAURI__.core.invoke(cmd, { req });
                t.push(performance.now() - s);
            }
            t.sort((a, b) => a - b);
            return { median: t[Math.floor(n / 2)], p95: t[Math.floor(n * 0.95)] };
        })();
        "#,
        vec![json!(cmd), req, json!(n)],
    )
    .await
}

/// Memoria (MB) de la app y de todos sus procesos hijos (WebView2).
#[cfg(windows)]
fn memory_mb(exe: &std::path::Path) -> (f64, f64, f64) {
    let script = format!(
        r#"
        $all = Get-CimInstance Win32_Process
        $root = $all | Where-Object {{ $_.ExecutablePath -eq '{}' }} | Select-Object -First 1
        $ids = @($root.ProcessId); $i = 0
        while ($i -lt $ids.Count) {{ $ids += @($all | Where-Object {{ $_.ParentProcessId -eq $ids[$i] }} | ForEach-Object {{ $_.ProcessId }}); $i++ }}
        $procs = $all | Where-Object {{ $ids -contains $_.ProcessId }}
        $app = [math]::Round($root.PrivatePageCount / 1MB, 1)
        $priv = [math]::Round(($procs | Measure-Object PrivatePageCount -Sum).Sum / 1MB, 1)
        $ws = [math]::Round(($procs | Measure-Object WorkingSetSize -Sum).Sum / 1MB, 1)
        "$app;$priv;$ws"
        "#,
        // Windows devuelve ExecutablePath con `\` y resuelto: se normaliza igual.
        exe.canonicalize()
            .unwrap_or_else(|_| exe.to_path_buf())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', r"\")
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .expect("powershell");
    let text = String::from_utf8_lossy(&out.stdout);
    let nums: Vec<f64> = text
        .trim()
        .split(';')
        .map(|n| n.replace(',', ".").parse().unwrap_or(f64::NAN))
        .collect();
    (nums[0], nums[1], nums[2])
}

#[tokio::test(flavor = "multi_thread")]
async fn performance_budgets() {
    if !enabled() {
        eprintln!("omitido: define GRYPHUS_E2E=1 para ejecutar las pruebas e2e");
        return;
    }
    let _serial = SERIAL.lock().await;
    let fx = Fixture::new();
    // Directorio con 1000 archivos para medir el listado del explorador.
    for i in 0..1000 {
        fx.repo
            .write_file(fx.repo.root(), &format!("muchos/archivo{i:04}.txt"), "x\n");
    }
    fx.repo.commit_all(fx.repo.root(), "1000 archivos");

    let app = App::start_with(&fx, true).await;
    app.wait_css("#project-path").await;
    let startup_ms = app.invoke("ui_ready", Value::Null).await;
    // Desglose dentro de la WebView (ms desde el inicio de la navegación).
    let startup_breakdown = app
        .script(
            r#"
            const nav = performance.getEntriesByType('navigation')[0];
            const mark = performance.getEntriesByName('ui-ready')[0];
            const res = performance.getEntriesByType('resource').map((r) => ({
                name: r.name.split('/').pop(),
                start: Math.round(r.startTime),
                end: Math.round(r.responseEnd),
                kb: Math.round((r.decodedBodySize || 0) / 1024),
            }));
            return {
                ui_ready_in_webview: mark ? Math.round(mark.startTime) : null,
                dom_content_loaded: Math.round(nav.domContentLoadedEventEnd),
                load_event: Math.round(nav.loadEventEnd),
                resources: res,
            };
            "#,
            vec![],
        )
        .await;

    app.open_project(&fx.root()).await;
    app.wait_css(r#".tree-row[data-path="README.md"]"#).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    #[cfg(windows)]
    let (app_mb, total_private_mb, total_ws_mb) = memory_mb(&app_binary(true));
    #[cfg(not(windows))]
    let (app_mb, total_private_mb, total_ws_mb) = (f64::NAN, f64::NAN, f64::NAN);

    let project = app.invoke("get_project", Value::Null).await;
    let main_wt = project["worktrees"][0]["id"].clone();
    let list_dir = ipc_timing(
        &app,
        "list_dir",
        json!({ "worktree": main_wt, "path": "muchos" }),
        30,
    )
    .await;
    let list_root = ipc_timing(
        &app,
        "list_dir",
        json!({ "worktree": main_wt, "path": "" }),
        30,
    )
    .await;
    let status = ipc_timing(&app, "git_status", json!({ "worktree": main_wt }), 20).await;

    // Latencia tecla → eco con la UI completa.
    app.click("#new-feature").await;
    app.type_into("#feature-name", "perf").await;
    app.click("#create-feature").await;
    app.click(r#".feature[data-feature="perf"] .launch"#).await;
    app.wait_terminal("fake-agent ready").await;
    let agent = app.invoke("get_project", Value::Null).await["agents"][0]["id"].clone();
    let echo = app
        .script(
            r#"
            const [agent] = arguments;
            return (async () => {
                const t = [];
                for (let i = 0; i < 30; i++) {
                    await new Promise((r) => setTimeout(r, 40));
                    t.push(await window.GryphusJs.debugEchoLatency(agent, 'k' + i));
                }
                t.sort((a, b) => a - b);
                return { median: t[15], p95: t[28] };
            })();
            "#,
            vec![agent],
        )
        .await;
    app.quit().await;

    let report = json!({
        "startup_ms": startup_ms,
        "startup_breakdown": startup_breakdown,
        "memory_mb": { "app_private": app_mb, "total_private": total_private_mb, "total_working_set": total_ws_mb },
        "echo_ms": echo,
        "list_dir_1000_ms": list_dir,
        "list_dir_root_ms": list_root,
        "git_status_ms": status,
    });
    println!(
        "\n── Rendimiento (release) ──\n{}",
        serde_json::to_string_pretty(&report).unwrap()
    );
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target")
        .join("perf.json");
    std::fs::write(out, serde_json::to_string_pretty(&report).unwrap()).unwrap();

    let ms = |v: &Value| v.as_f64().unwrap_or(f64::MAX);
    assert!(ms(&startup_ms) < 800.0, "arranque {startup_ms}");
    assert!(ms(&echo["median"]) < 50.0, "eco {echo}");
    assert!(ms(&list_dir["median"]) < 60.0, "list_dir {list_dir}");
}
