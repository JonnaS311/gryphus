//! Ruta del binario `fake-agent` para pruebas de otros paquetes
//! (`CARGO_BIN_EXE_*` solo existe dentro del propio paquete).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Compila `fake-agent` (una vez por proceso de pruebas) y devuelve la ruta del ejecutable.
pub fn fake_agent_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(build_fake_agent).clone()
}

fn build_fake_agent() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/gryphus-testkit está dos niveles bajo la raíz");
    let out = Command::new(cargo)
        .args(["build", "-p", "fake-agent", "--message-format=json"])
        .current_dir(workspace)
        .output()
        .expect("ejecutar cargo build -p fake-agent");
    assert!(
        out.status.success(),
        "cargo build -p fake-agent falló:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|msg| msg["reason"] == "compiler-artifact" && msg["target"]["name"] == "fake-agent")
        .and_then(|msg| msg["executable"].as_str().map(PathBuf::from))
        .expect("cargo no informó el ejecutable de fake-agent")
}
