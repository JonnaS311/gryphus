//! Prueba de propiedades: cualquier secuencia de operaciones (válidas o no)
//! deja el proyecto cumpliendo todas sus invariantes, y un error nunca
//! modifica el estado.

use std::path::PathBuf;

use gryphus_core::ports::WorktreeInfo;
use gryphus_core::{AgentStatus, BranchName, FeatureName, Project, ProviderId};
use proptest::prelude::*;

const PATHS: [&str; 5] = ["/repo", "/wt/a", "/wt/b", "/wt/c", "/wt/d"];
const NAMES: [&str; 4] = ["login", "LOGIN", "pagos", "search"];

#[derive(Debug, Clone)]
enum Op {
    /// Qué rutas existen ahora en git (máscara de bits sobre PATHS).
    Sync(u8),
    Forget(u8),
    CreateFeature(u8, u8),
    Attach(u8, u8),
    Detach(u8, u8),
    DeleteFeature(u8),
    AddAgent(u8, u8, u8),
    SetStatus(u8, u8),
    RemoveAgent(u8),
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    let i = || any::<u8>();
    prop_oneof![
        i().prop_map(Op::Sync),
        i().prop_map(Op::Forget),
        (i(), i()).prop_map(|(a, b)| Op::CreateFeature(a, b)),
        (i(), i()).prop_map(|(a, b)| Op::Attach(a, b)),
        (i(), i()).prop_map(|(a, b)| Op::Detach(a, b)),
        i().prop_map(Op::DeleteFeature),
        (i(), i(), i()).prop_map(|(a, b, c)| Op::AddAgent(a, b, c)),
        (i(), i()).prop_map(|(a, b)| Op::SetStatus(a, b)),
        i().prop_map(Op::RemoveAgent),
        Just(Op::Restart),
    ]
}

/// Elige un elemento por índice; si la lista está vacía devuelve `None`.
fn pick<T: Copy>(items: &[T], i: u8) -> Option<T> {
    (!items.is_empty()).then(|| items[i as usize % items.len()])
}

fn apply(p: &mut Project, op: &Op) {
    let features: Vec<_> = p.features().iter().map(|f| f.id()).collect();
    let worktrees: Vec<_> = p.worktrees().iter().map(|w| w.id()).collect();
    let agents: Vec<_> = p.agents().iter().map(|a| a.id()).collect();

    // Un error debe dejar el proyecto intacto.
    let before = p.clone();
    let failed = match *op {
        Op::Sync(mask) => {
            let infos: Vec<_> = PATHS
                .iter()
                .enumerate()
                .filter(|(i, _)| i == &0 || mask & (1 << i) != 0)
                .map(|(i, path)| WorktreeInfo {
                    path: PathBuf::from(path),
                    branch: Some(BranchName::parse(&format!("b{i}")).unwrap()),
                    head: None,
                    is_main: i == 0,
                    locked: false,
                })
                .collect();
            p.sync_worktrees(&infos);
            false
        }
        Op::Forget(w) => pick(&worktrees, w).is_some_and(|w| p.forget_worktree(w).is_err()),
        Op::CreateFeature(n, w) => pick(&worktrees, w).is_some_and(|w| {
            let name = FeatureName::parse(NAMES[n as usize % NAMES.len()]).unwrap();
            p.create_feature(name, w).is_err()
        }),
        Op::Attach(f, w) => match (pick(&features, f), pick(&worktrees, w)) {
            (Some(f), Some(w)) => p.attach_worktree(f, w).is_err(),
            _ => false,
        },
        Op::Detach(f, w) => match (pick(&features, f), pick(&worktrees, w)) {
            (Some(f), Some(w)) => p.detach_worktree(f, w).is_err(),
            _ => false,
        },
        Op::DeleteFeature(f) => pick(&features, f).is_some_and(|f| p.delete_feature(f).is_err()),
        Op::AddAgent(f, w, prov) => match (pick(&features, f), pick(&worktrees, w)) {
            (Some(f), Some(w)) => {
                let provider = ProviderId::ALL[prov as usize % 3];
                p.add_agent(f, w, provider).is_err()
            }
            _ => false,
        },
        Op::SetStatus(a, s) => pick(&agents, a).is_some_and(|a| {
            let status = match s % 4 {
                0 => AgentStatus::Starting,
                1 => AgentStatus::Running,
                2 => AgentStatus::Exited { code: Some(0) },
                _ => AgentStatus::Failed { reason: "x".into() },
            };
            p.set_agent_status(a, status).is_err()
        }),
        Op::RemoveAgent(a) => pick(&agents, a).is_some_and(|a| p.remove_agent(a).is_err()),
        Op::Restart => {
            p.reset_after_restart();
            false
        }
    };
    if failed {
        assert_eq!(
            *p, before,
            "una operación fallida modificó el estado: {op:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn any_operation_sequence_preserves_invariants(ops in prop::collection::vec(op(), 1..60)) {
        let mut p = Project::new(PathBuf::from("/repo"));
        for op in &ops {
            apply(&mut p, op);
            if let Err(e) = p.check_invariants() {
                return Err(TestCaseError::fail(format!("tras {op:?}: {e}")));
            }
        }
        // El estado final sobrevive a la persistencia.
        let back: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        prop_assert_eq!(back, p);
    }
}
