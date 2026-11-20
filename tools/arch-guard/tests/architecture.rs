//! Guardias de arquitectura. Ver docs/architecture/overview.md#regla-de-dependencias
//! y docs/testing.md#guardias-de-arquitectura.
//!
//! Si añades un crate al workspace, esta prueba fallará hasta que declares
//! aquí qué crates internos puede usar. Es intencional.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Dependencias internas (normales, no dev ni build) permitidas por crate.
fn allowed() -> BTreeMap<&'static str, &'static [&'static str]> {
    const ADAPTER: &[&str] = &["gryphus-core"];
    BTreeMap::from([
        ("gryphus-core", &[] as &[&str]),
        ("gryphus-ipc", &[]),
        ("gryphus-app", &["gryphus-core"]),
        ("gryphus-git", ADAPTER),
        ("gryphus-agents", ADAPTER),
        ("gryphus-fs", ADAPTER),
        ("gryphus-store", ADAPTER),
        ("gryphus-testkit", &["gryphus-core"]),
        ("gryphus-ui", &["gryphus-ipc"]),
        (
            "gryphus-desktop",
            &[
                "gryphus-core",
                "gryphus-app",
                "gryphus-ipc",
                "gryphus-git",
                "gryphus-agents",
                "gryphus-fs",
                "gryphus-store",
            ],
        ),
        // Comparte el lenguaje de comandos con FakeAgentRuntime (agent_script).
        ("fake-agent", &["gryphus-testkit"]),
        ("arch-guard", &[]),
    ])
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("tools/arch-guard está dos niveles bajo la raíz")
        .to_path_buf()
}

fn metadata() -> serde_json::Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("no se pudo ejecutar cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata falló: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("JSON de cargo metadata inválido")
}

#[test]
fn crates_only_depend_on_allowed_internal_crates() {
    let meta = metadata();
    let packages = meta["packages"].as_array().expect("packages");
    let members: BTreeSet<&str> = packages
        .iter()
        .map(|p| p["name"].as_str().expect("name"))
        .collect();
    let allowed = allowed();
    let mut violations = Vec::new();

    for pkg in packages {
        let name = pkg["name"].as_str().expect("name");
        let Some(permitted) = allowed.get(name) else {
            violations.push(format!(
                "{name}: crate nuevo sin regla en tools/arch-guard/tests/architecture.rs"
            ));
            continue;
        };
        for dep in pkg["dependencies"].as_array().expect("dependencies") {
            let dep_name = dep["name"].as_str().expect("dep name");
            let is_normal = dep["kind"].is_null();
            if is_normal && members.contains(dep_name) && !permitted.contains(&dep_name) {
                violations.push(format!("{name} no puede depender de {dep_name}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Violaciones de la regla de dependencias:\n  {}",
        violations.join("\n  ")
    );
}

/// Solo `ui/src/bridge/` puede tocar JS (wasm-bindgen, js-sys, web-sys).
#[test]
fn only_ui_bridge_touches_js() {
    const FORBIDDEN: [&str; 3] = ["wasm_bindgen", "js_sys", "web_sys"];
    let ui_src = workspace_root().join("ui").join("src");
    let bridge = ui_src.join("bridge");
    let mut violations = Vec::new();

    for file in rust_files(&ui_src) {
        if file.starts_with(&bridge) {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("leer archivo de la UI");
        for word in FORBIDDEN {
            if text.contains(word) {
                violations.push(format!("{} usa {word}", file.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Uso de JS fuera de ui/src/bridge/:\n  {}",
        violations.join("\n  ")
    );
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
    files
}
