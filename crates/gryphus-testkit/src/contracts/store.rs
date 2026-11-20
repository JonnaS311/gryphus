//! Contrato de `StorePort`.

use std::path::PathBuf;

use gryphus_core::ports::{PortErrorKind, StorePort, WorktreeInfo};
use gryphus_core::{BranchName, Config, FeatureName, Project, ProjectId, ProviderId};

use super::assert_kind;

pub trait StoreHarness {
    fn port(&self) -> &dyn StorePort;
    /// Escribe el estado persistido de un proyecto tal cual (para simular corrupción).
    fn write_raw_project(&self, id: &ProjectId, contents: &str);
}

/// Proyecto con una feature, un worktree extra y un agente.
fn sample_project() -> Project {
    let mut p = Project::new(PathBuf::from("/repos/demo"));
    let info = |path: &str, branch: &str, is_main| WorktreeInfo {
        path: PathBuf::from(path),
        branch: Some(BranchName::parse(branch).unwrap()),
        head: None,
        is_main,
        locked: false,
    };
    p.sync_worktrees(&[
        info("/repos/demo", "main", true),
        info("/wt/login", "feat/login", false),
    ]);
    let wt = p
        .worktree_by_path(&PathBuf::from("/wt/login"))
        .unwrap()
        .id();
    let f = p
        .create_feature(FeatureName::parse("login").unwrap(), wt)
        .unwrap();
    p.add_agent(f, wt, ProviderId::Claude).unwrap();
    p
}

pub fn missing_project_is_none<H: StoreHarness>(h: &H) {
    let id = ProjectId::from_canonical_root(&PathBuf::from("/no/existe"));
    assert_eq!(h.port().load_project(&id).unwrap(), None);
}

pub fn saved_project_round_trips<H: StoreHarness>(h: &H) {
    let p = sample_project();
    h.port().save_project(&p).unwrap();
    assert_eq!(h.port().load_project(p.id()).unwrap(), Some(p));
}

pub fn saving_again_overwrites<H: StoreHarness>(h: &H) {
    let mut p = sample_project();
    h.port().save_project(&p).unwrap();
    p.reset_after_restart();
    h.port().save_project(&p).unwrap();
    assert_eq!(h.port().load_project(p.id()).unwrap(), Some(p));
}

pub fn config_defaults_when_absent<H: StoreHarness>(h: &H) {
    assert_eq!(h.port().load_config().unwrap(), Config::default());
}

pub fn unreadable_project_is_corrupted<H: StoreHarness>(h: &H) {
    let p = sample_project();
    h.write_raw_project(p.id(), "{ esto no es json");
    assert_kind(h.port().load_project(p.id()), PortErrorKind::Corrupted);
}

pub fn project_violating_invariants_is_corrupted<H: StoreHarness>(h: &H) {
    let p = sample_project();
    let mut json = serde_json::to_value(&p).unwrap();
    json["features"][0]["worktrees"] = serde_json::json!([]);
    h.write_raw_project(p.id(), &json.to_string());
    assert_kind(h.port().load_project(p.id()), PortErrorKind::Corrupted);
}
