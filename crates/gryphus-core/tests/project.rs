//! Comportamiento del agregado `Project` usando solo su API pública.

use std::path::PathBuf;

use gryphus_core::ports::WorktreeInfo;
use gryphus_core::{
    AgentStatus, BranchName, DomainError, FeatureId, FeatureName, Project, ProviderId, WorktreeId,
};

fn info(path: &str, branch: &str, is_main: bool) -> WorktreeInfo {
    WorktreeInfo {
        path: PathBuf::from(path),
        branch: Some(BranchName::parse(branch).unwrap()),
        head: None,
        is_main,
        locked: false,
    }
}

fn name(s: &str) -> FeatureName {
    FeatureName::parse(s).unwrap()
}

/// Proyecto con el worktree principal y dos worktrees libres.
fn project() -> (Project, WorktreeId, WorktreeId, WorktreeId) {
    let mut p = Project::new(PathBuf::from("/repo"));
    p.sync_worktrees(&[
        info("/repo", "main", true),
        info("/wt/login", "feat/login", false),
        info("/wt/pagos", "feat/pagos", false),
    ]);
    let id = |path: &str| p.worktree_by_path(&PathBuf::from(path)).unwrap().id();
    let (main, a, b) = (id("/repo"), id("/wt/login"), id("/wt/pagos"));
    (p, main, a, b)
}

fn feature_with_agent(p: &mut Project, wt: WorktreeId) -> (FeatureId, gryphus_core::AgentId) {
    let f = p.create_feature(name("login"), wt).unwrap();
    let a = p.add_agent(f, wt, ProviderId::Claude).unwrap();
    (f, a)
}

// ── Worktrees ────────────────────────────────────────────────────────────────

#[test]
fn sync_registers_new_worktrees_and_marks_missing_ones() {
    let (mut p, main, a, b) = project();
    assert_eq!(p.worktrees().len(), 3);
    assert!(p.worktree(main).unwrap().is_main());

    let changed = p.sync_worktrees(&[
        info("/repo", "main", true),
        info("/wt/login", "otra", false),
    ]);
    assert!(changed);
    assert!(p.worktree(b).unwrap().is_missing());
    assert_eq!(p.worktree(a).unwrap().branch().unwrap().as_str(), "otra");

    // Reaparece: se desmarca y conserva su id.
    p.sync_worktrees(&[info("/wt/pagos", "feat/pagos", false)]);
    assert!(!p.worktree(b).unwrap().is_missing());

    assert!(
        !p.sync_worktrees(&[info("/wt/pagos", "feat/pagos", false)]),
        "sin cambios"
    );
    p.check_invariants().unwrap();
}

#[test]
fn forget_worktree_rules() {
    let (mut p, main, a, b) = project();
    p.create_feature(name("login"), a).unwrap();
    assert_eq!(
        p.forget_worktree(main),
        Err(DomainError::MainWorktree(main))
    );
    assert!(matches!(
        p.forget_worktree(a),
        Err(DomainError::WorktreeAlreadyAssigned { .. })
    ));
    assert_eq!(p.forget_worktree(b).unwrap().id(), b);
    assert_eq!(p.worktree(b), Err(DomainError::WorktreeNotFound(b)));
}

// ── Features ─────────────────────────────────────────────────────────────────

#[test]
fn create_feature_requires_a_free_existing_worktree() {
    let (mut p, _, a, b) = project();
    let f = p.create_feature(name("login"), a).unwrap();
    assert_eq!(p.feature(f).unwrap().worktrees(), [a]);
    assert_eq!(p.feature_of_worktree(a), Some(f));

    assert_eq!(
        p.create_feature(name("otra"), a),
        Err(DomainError::WorktreeAlreadyAssigned {
            worktree: a,
            feature: f
        })
    );
    let ghost = WorktreeId::from_raw(999);
    assert_eq!(
        p.create_feature(name("otra"), ghost),
        Err(DomainError::WorktreeNotFound(ghost))
    );
    p.sync_worktrees(&[
        info("/repo", "main", true),
        info("/wt/login", "feat/login", false),
    ]);
    assert_eq!(
        p.create_feature(name("otra"), b),
        Err(DomainError::WorktreeMissing(b))
    );
}

#[test]
fn feature_names_are_unique_case_insensitively() {
    let (mut p, _, a, b) = project();
    p.create_feature(name("Login"), a).unwrap();
    assert_eq!(
        p.create_feature(name("login"), b),
        Err(DomainError::DuplicateFeatureName("login".into()))
    );
}

#[test]
fn the_main_worktree_can_back_a_feature() {
    let (mut p, main, ..) = project();
    let f = p.create_feature(name("hotfix"), main).unwrap();
    assert_eq!(p.feature(f).unwrap().worktrees(), [main]);
}

#[test]
fn attach_and_detach_worktrees() {
    let (mut p, _, a, b) = project();
    let f = p.create_feature(name("login"), a).unwrap();

    p.attach_worktree(f, b).unwrap();
    p.attach_worktree(f, b).unwrap(); // idempotente
    assert_eq!(p.feature(f).unwrap().worktrees(), [a, b]);
    assert_eq!(
        p.unassigned_worktrees().count(),
        1,
        "solo queda el principal"
    );

    p.detach_worktree(f, b).unwrap();
    assert_eq!(p.feature(f).unwrap().worktrees(), [a]);
    assert_eq!(p.detach_worktree(f, a), Err(DomainError::LastWorktree(f)));
    assert_eq!(
        p.detach_worktree(f, b),
        Err(DomainError::WorktreeNotInFeature {
            worktree: b,
            feature: f
        })
    );
}

#[test]
fn cannot_detach_a_worktree_used_by_an_agent() {
    let (mut p, _, a, b) = project();
    let (f, agent) = feature_with_agent(&mut p, a);
    p.attach_worktree(f, b).unwrap();
    assert_eq!(
        p.detach_worktree(f, a),
        Err(DomainError::WorktreeInUse { worktree: a, agent })
    );
}

#[test]
fn delete_feature_requires_inactive_agents_and_removes_them() {
    let (mut p, _, a, _) = project();
    let (f, agent) = feature_with_agent(&mut p, a);
    assert_eq!(p.delete_feature(f), Err(DomainError::AgentsStillRunning(f)));

    p.set_agent_status(agent, AgentStatus::Exited { code: Some(0) })
        .unwrap();
    let removed = p.delete_feature(f).unwrap();
    assert_eq!(removed.worktrees(), [a]);
    assert!(p.agents().is_empty());
    assert_eq!(p.feature_of_worktree(a), None, "el worktree queda libre");
    p.check_invariants().unwrap();
}

// ── Agentes ──────────────────────────────────────────────────────────────────

#[test]
fn agents_only_work_in_worktrees_of_their_feature() {
    let (mut p, _, a, b) = project();
    let f = p.create_feature(name("login"), a).unwrap();
    assert_eq!(
        p.add_agent(f, b, ProviderId::Codex),
        Err(DomainError::WorktreeNotInFeature {
            worktree: b,
            feature: f
        })
    );
    let agent = p.add_agent(f, a, ProviderId::Codex).unwrap();
    let agent = p.agent(agent).unwrap();
    assert_eq!(agent.status(), &AgentStatus::Starting);
    assert_eq!(agent.provider(), ProviderId::Codex);
    assert_eq!(p.agents_of(f).count(), 1);
}

#[test]
fn cannot_add_agent_on_a_missing_worktree() {
    let (mut p, _, a, _) = project();
    let f = p.create_feature(name("login"), a).unwrap();
    p.sync_worktrees(&[info("/repo", "main", true)]);
    assert_eq!(
        p.add_agent(f, a, ProviderId::Claude),
        Err(DomainError::WorktreeMissing(a))
    );
}

#[test]
fn agent_status_follows_the_state_machine() {
    let (mut p, _, a, _) = project();
    let (_, agent) = feature_with_agent(&mut p, a);
    p.set_agent_status(agent, AgentStatus::Running).unwrap();
    assert_eq!(
        p.set_agent_status(agent, AgentStatus::Starting),
        Err(DomainError::InvalidTransition {
            agent,
            from: "running",
            to: "starting"
        })
    );
    p.set_agent_status(agent, AgentStatus::Exited { code: Some(1) })
        .unwrap();
    p.set_agent_status(agent, AgentStatus::Starting).unwrap(); // reinicio
}

#[test]
fn remove_agent_requires_it_to_be_inactive() {
    let (mut p, _, a, _) = project();
    let (_, agent) = feature_with_agent(&mut p, a);
    assert_eq!(
        p.remove_agent(agent),
        Err(DomainError::AgentStillRunning(agent))
    );
    p.set_agent_status(agent, AgentStatus::Failed { reason: "x".into() })
        .unwrap();
    assert_eq!(p.remove_agent(agent).unwrap().id(), agent);
    assert_eq!(p.agent(agent), Err(DomainError::AgentNotFound(agent)));
}

#[test]
fn reset_after_restart_marks_active_agents_as_exited() {
    let (mut p, _, a, _) = project();
    let (f, running) = feature_with_agent(&mut p, a);
    p.set_agent_status(running, AgentStatus::Running).unwrap();
    let failed = p.add_agent(f, a, ProviderId::OpenCode).unwrap();
    p.set_agent_status(failed, AgentStatus::Failed { reason: "x".into() })
        .unwrap();

    p.reset_after_restart();
    assert_eq!(
        p.agent(running).unwrap().status(),
        &AgentStatus::Exited { code: None }
    );
    assert!(matches!(
        p.agent(failed).unwrap().status(),
        AgentStatus::Failed { .. }
    ));
}

// ── Persistencia ─────────────────────────────────────────────────────────────

#[test]
fn project_round_trips_through_json() {
    let (mut p, _, a, _) = project();
    feature_with_agent(&mut p, a);
    let json = serde_json::to_string(&p).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
    back.check_invariants().unwrap();
}

#[test]
fn check_invariants_detects_corrupted_state() {
    let (mut p, _, a, _) = project();
    p.create_feature(name("login"), a).unwrap();
    let mut json: serde_json::Value = serde_json::to_value(&p).unwrap();
    json["features"][0]["worktrees"] = serde_json::json!([]);
    let corrupted: Project = serde_json::from_value(json).unwrap();
    assert!(matches!(
        corrupted.check_invariants(),
        Err(DomainError::Invariant(_))
    ));
}

// ── Nombres de agente ────────────────────────────────────────────────────────

#[test]
fn agents_get_distinct_default_names_and_can_be_renamed() {
    let (mut p, _, a, _) = project();
    let (f, first) = feature_with_agent(&mut p, a);
    let second = p.add_agent(f, a, ProviderId::Codex).unwrap();
    let name_of = |p: &Project, id| p.agent(id).unwrap().name().unwrap().as_str().to_owned();
    assert_ne!(name_of(&p, first), name_of(&p, second));

    p.rename_agent(first, gryphus_core::AgentName::parse("Revisor").unwrap())
        .unwrap();
    assert_eq!(name_of(&p, first), "Revisor");
    assert!(matches!(
        p.rename_agent(
            gryphus_core::AgentId::from_raw(999),
            gryphus_core::AgentName::parse("x").unwrap()
        ),
        Err(DomainError::AgentNotFound(_))
    ));
    assert!(!p.name_unnamed_agents(), "todos tienen nombre");
}

#[test]
fn legacy_agents_without_name_get_one() {
    let (mut p, _, a, _) = project();
    let (_, agent) = feature_with_agent(&mut p, a);
    let mut json: serde_json::Value = serde_json::to_value(&p).unwrap();
    json["agents"][0].as_object_mut().unwrap().remove("name");
    let mut legacy: Project = serde_json::from_value(json).unwrap();
    assert!(legacy.agent(agent).unwrap().name().is_none());
    assert!(legacy.name_unnamed_agents());
    assert!(legacy.agent(agent).unwrap().name().is_some());
}
