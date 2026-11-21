//! Adaptador `StorePort` (ADR-009).
//!
//! - Proyectos: `<data_dir>/projects/<project-id>.json`, escritura atómica.
//!   Al cargar se verifican las invariantes; si fallan → `Corrupted`.
//! - Configuración: `config.toml` del usuario. Los proveedores que el usuario no
//!   declara se completan con los valores por defecto (no se pierden).
//!
//! Nada se escribe dentro del repositorio del usuario.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gryphus_core::ports::{PortError, PortErrorKind, PortResult, StorePort};
use gryphus_core::{Config, Project, ProjectId, ProviderConfig, ProviderId};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct JsonStore {
    projects_dir: PathBuf,
    config_file: PathBuf,
    /// Serializa las escrituras: en Windows `rename` sobre un destino que otro hilo
    /// está reemplazando falla con "acceso denegado".
    write_lock: Arc<Mutex<()>>,
}

impl JsonStore {
    /// `data_dir`: datos de la app (p. ej. `%APPDATA%\gryphus`).
    /// `config_file`: ruta de `config.toml`.
    pub fn new(data_dir: impl AsRef<Path>, config_file: impl Into<PathBuf>) -> Self {
        Self {
            projects_dir: data_dir.as_ref().join("projects"),
            config_file: config_file.into(),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn project_file(&self, id: &ProjectId) -> PathBuf {
        self.projects_dir.join(format!("{id}.json"))
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }
}

fn io_err(e: io::Error, path: &Path) -> PortError {
    let kind = match e.kind() {
        io::ErrorKind::NotFound => PortErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => PortErrorKind::PermissionDenied,
        _ => PortErrorKind::Io,
    };
    PortError::new(kind, format!("{}: {e}", path.display()))
}

fn corrupted(path: &Path, e: impl std::fmt::Display) -> PortError {
    PortError::new(PortErrorKind::Corrupted, format!("{}: {e}", path.display()))
}

/// Sufijo único por escritura: dos guardados simultáneos no pueden compartir
/// temporal (antes solo se usaba el PID y se pisaban entre hilos).
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Escribe en un temporal del mismo directorio y lo renombra encima del destino.
fn write_atomic(path: &Path, bytes: &[u8]) -> PortResult<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| io_err(e, dir))?;
    let tmp = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    result.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_err(e, path)
    })
}

/// Forma del archivo `config.toml` (claves de proveedor como texto para dar
/// errores claros con nombres desconocidos).
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    agents: BTreeMap<String, ProviderConfig>,
    worktrees_dir: Option<PathBuf>,
}

/// Parsea `config.toml` y completa los proveedores no declarados con sus valores por defecto.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut config = Config::default();
    for (name, provider) in raw.agents {
        let id: ProviderId = name
            .parse()
            .map_err(|e: gryphus_core::agent::UnknownProvider| e.to_string())?;
        config.agents.insert(id, provider);
    }
    config.worktrees_dir = raw.worktrees_dir;
    Ok(config)
}

impl StorePort for JsonStore {
    fn load_project(&self, id: &ProjectId) -> PortResult<Option<Project>> {
        let path = self.project_file(id);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io_err(e, &path)),
        };
        let project: Project = serde_json::from_slice(&bytes).map_err(|e| corrupted(&path, e))?;
        project
            .check_invariants()
            .map_err(|e| corrupted(&path, e))?;
        Ok(Some(project))
    }

    fn save_project(&self, project: &Project) -> PortResult<()> {
        let json = serde_json::to_vec_pretty(project)
            .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;
        let _guard = self.write_lock.lock().unwrap_or_else(|p| p.into_inner());
        write_atomic(&self.project_file(project.id()), &json)
    }

    fn load_config(&self) -> PortResult<Config> {
        match fs::read_to_string(&self.config_file) {
            Ok(text) => parse_config(&text).map_err(|e| {
                PortError::new(
                    PortErrorKind::InvalidInput,
                    format!("{} inválido: {e}", self.config_file.display()),
                )
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(io_err(e, &self.config_file)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_example_parses() {
        let cfg = parse_config(
            r#"
            [agents.claude]
            command = "claude"
            args = ["--verbose"]
            env = { CLAUDE_X = "1" }

            [agents.codex]
            command = "codex"
            "#,
        )
        .unwrap();
        let claude = cfg.provider(ProviderId::Claude).unwrap();
        assert_eq!(claude.args, ["--verbose"]);
        assert_eq!(claude.env["CLAUDE_X"], "1");
    }

    #[test]
    fn undeclared_providers_keep_their_defaults() {
        let cfg = parse_config("[agents.claude]\ncommand = \"/opt/claude\"\n").unwrap();
        assert_eq!(
            cfg.provider(ProviderId::Claude).unwrap().command,
            "/opt/claude"
        );
        assert_eq!(cfg.provider(ProviderId::Codex).unwrap().command, "codex");
        assert_eq!(
            cfg.provider(ProviderId::OpenCode).unwrap().command,
            "opencode"
        );
    }

    #[test]
    fn worktrees_dir_is_read() {
        let cfg = parse_config("worktrees_dir = \"/wts\"\n").unwrap();
        assert_eq!(cfg.worktrees_dir, Some(PathBuf::from("/wts")));
    }

    #[test]
    fn unknown_provider_and_fields_are_errors() {
        assert!(parse_config("[agents.gpt]\ncommand = \"x\"\n")
            .unwrap_err()
            .contains("proveedor desconocido"));
        assert!(parse_config("tema = \"oscuro\"\n").is_err());
        assert!(parse_config("[agents.claude\n").is_err());
    }
}
