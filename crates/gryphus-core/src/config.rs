//! Configuración global: proveedores de agentes y ubicación de worktrees.
//! El adaptador `gryphus-store` la lee de `config.toml`; aquí solo están el
//! modelo, sus valores por defecto y las reglas puras que dependen de ella.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::ProviderId;
use crate::error::{DomainError, DomainResult};
use crate::names::FeatureName;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub agents: BTreeMap<ProviderId, ProviderConfig>,
    /// `None` = junto al repo: `<padre>/<repo>.worktrees/<feature>` (ADR-012).
    pub worktrees_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl ProviderConfig {
    pub fn with_command(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            ..Self::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agents: ProviderId::ALL
                .into_iter()
                .map(|p| (p, ProviderConfig::with_command(p.as_str())))
                .collect(),
            worktrees_dir: None,
        }
    }
}

impl Config {
    pub fn provider(&self, id: ProviderId) -> DomainResult<&ProviderConfig> {
        self.agents
            .get(&id)
            .filter(|p| !p.command.trim().is_empty())
            .ok_or(DomainError::ProviderNotConfigured(id))
    }

    /// Dónde crear el worktree de una feature nueva.
    pub fn worktree_path(&self, repo_root: &Path, feature: &FeatureName) -> PathBuf {
        let repo_name = repo_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repo".to_owned());
        match &self.worktrees_dir {
            Some(dir) => dir.join(repo_name).join(feature.as_str()),
            None => repo_root
                .parent()
                .unwrap_or(repo_root)
                .join(format!("{repo_name}.worktrees"))
                .join(feature.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_the_three_providers() {
        let cfg = Config::default();
        for p in ProviderId::ALL {
            assert_eq!(cfg.provider(p).unwrap().command, p.as_str());
        }
    }

    #[test]
    fn missing_or_blank_provider_is_not_configured() {
        let mut cfg = Config::default();
        cfg.agents.remove(&ProviderId::Codex);
        cfg.agents
            .insert(ProviderId::Claude, ProviderConfig::with_command("  "));
        assert_eq!(
            cfg.provider(ProviderId::Codex),
            Err(DomainError::ProviderNotConfigured(ProviderId::Codex))
        );
        assert_eq!(
            cfg.provider(ProviderId::Claude),
            Err(DomainError::ProviderNotConfigured(ProviderId::Claude))
        );
    }

    #[test]
    fn worktree_path_defaults_next_to_the_repo() {
        let cfg = Config::default();
        let name = FeatureName::parse("login").unwrap();
        let root = Path::new("work").join("gryphus");
        assert_eq!(
            cfg.worktree_path(&root, &name),
            Path::new("work").join("gryphus.worktrees").join("login")
        );
    }

    #[test]
    fn worktree_path_uses_configured_dir() {
        let cfg = Config {
            worktrees_dir: Some(PathBuf::from("wts")),
            ..Config::default()
        };
        let name = FeatureName::parse("login").unwrap();
        assert_eq!(
            cfg.worktree_path(Path::new("gryphus"), &name),
            Path::new("wts").join("gryphus").join("login")
        );
    }
}
