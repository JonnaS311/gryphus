//! [`Project`]: raíz del agregado. Toda mutación pasa por aquí y cada método
//! deja el proyecto en un estado que cumple [`Project::check_invariants`].
//!
//! Invariantes (docs/architecture/domain.md#invariantes):
//! 1. Una feature tiene al menos un worktree.
//! 2. Un agente solo trabaja en un worktree de su feature.
//! 3. Un worktree pertenece a lo sumo a una feature.
//! 4. Una feature con agentes activos no se puede eliminar.
//! 5. El worktree principal nunca se olvida ni elimina desde Gryphus.
//! 6. El nombre de feature es único (sin distinguir mayúsculas).
//! 7. No se puede desasociar un worktree que usa un agente.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{Agent, AgentStatus, ProviderId};
use crate::error::{DomainError, DomainResult};
use crate::feature::Feature;
use crate::ids::{AgentId, FeatureId, ProjectId, WorktreeId};
use crate::names::{AgentName, FeatureName};
use crate::ports::git::WorktreeInfo;
use crate::worktree::Worktree;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    id: ProjectId,
    root: PathBuf,
    features: Vec<Feature>,
    worktrees: Vec<Worktree>,
    agents: Vec<Agent>,
    /// Siguiente id secuencial (compartido por features, worktrees y agentes).
    next_id: u64,
}

impl Project {
    /// `root` debe venir canonicalizada por el adaptador.
    pub fn new(root: PathBuf) -> Self {
        Self {
            id: ProjectId::from_canonical_root(&root),
            root,
            features: Vec::new(),
            worktrees: Vec::new(),
            agents: Vec::new(),
            next_id: 1,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // ── Consultas ────────────────────────────────────────────────────────────

    pub fn id(&self) -> &ProjectId {
        &self.id
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn features(&self) -> &[Feature] {
        &self.features
    }
    pub fn worktrees(&self) -> &[Worktree] {
        &self.worktrees
    }
    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    pub fn feature(&self, id: FeatureId) -> DomainResult<&Feature> {
        self.features
            .iter()
            .find(|f| f.id == id)
            .ok_or(DomainError::FeatureNotFound(id))
    }

    pub fn worktree(&self, id: WorktreeId) -> DomainResult<&Worktree> {
        self.worktrees
            .iter()
            .find(|w| w.id == id)
            .ok_or(DomainError::WorktreeNotFound(id))
    }

    pub fn agent(&self, id: AgentId) -> DomainResult<&Agent> {
        self.agents
            .iter()
            .find(|a| a.id == id)
            .ok_or(DomainError::AgentNotFound(id))
    }

    pub fn worktree_by_path(&self, path: &Path) -> Option<&Worktree> {
        self.worktrees.iter().find(|w| w.path == path)
    }

    pub fn agents_of(&self, feature: FeatureId) -> impl Iterator<Item = &Agent> {
        self.agents.iter().filter(move |a| a.feature == feature)
    }

    pub fn feature_of_worktree(&self, worktree: WorktreeId) -> Option<FeatureId> {
        self.features
            .iter()
            .find(|f| f.worktrees.contains(&worktree))
            .map(|f| f.id)
    }

    /// Worktrees disponibles para asignar a una feature.
    pub fn unassigned_worktrees(&self) -> impl Iterator<Item = &Worktree> {
        self.worktrees
            .iter()
            .filter(|w| !w.missing && self.feature_of_worktree(w.id).is_none())
    }

    // ── Worktrees ────────────────────────────────────────────────────────────

    /// Reconcilia el registro con `git worktree list`: registra los nuevos,
    /// actualiza branch/principal y marca como `missing` los que ya no están.
    /// Devuelve `true` si algo cambió.
    pub fn sync_worktrees(&mut self, infos: &[WorktreeInfo]) -> bool {
        let before = self.worktrees.clone();
        for wt in &mut self.worktrees {
            match infos.iter().find(|i| i.path == wt.path) {
                Some(info) => {
                    wt.branch = info.branch.clone();
                    wt.is_main = info.is_main;
                    wt.missing = false;
                }
                None => wt.missing = true,
            }
        }
        for info in infos {
            if self.worktree_by_path(&info.path).is_none() {
                let id = WorktreeId(self.next_id());
                self.worktrees.push(Worktree {
                    id,
                    path: info.path.clone(),
                    branch: info.branch.clone(),
                    is_main: info.is_main,
                    missing: false,
                });
            }
        }
        self.worktrees != before
    }

    /// Quita del registro un worktree ya eliminado en git. No aplica al principal
    /// ni a uno asignado a una feature.
    pub fn forget_worktree(&mut self, worktree: WorktreeId) -> DomainResult<Worktree> {
        let wt = self.worktree(worktree)?;
        if wt.is_main {
            return Err(DomainError::MainWorktree(worktree));
        }
        if let Some(feature) = self.feature_of_worktree(worktree) {
            return Err(DomainError::WorktreeAlreadyAssigned { worktree, feature });
        }
        let idx = self.worktree_index(worktree)?;
        Ok(self.worktrees.remove(idx))
    }

    // ── Features ─────────────────────────────────────────────────────────────

    pub fn create_feature(
        &mut self,
        name: FeatureName,
        worktree: WorktreeId,
    ) -> DomainResult<FeatureId> {
        if self.features.iter().any(|f| f.name.key() == name.key()) {
            return Err(DomainError::DuplicateFeatureName(name.as_str().to_owned()));
        }
        self.ensure_assignable(worktree)?;
        let id = FeatureId(self.next_id());
        self.features.push(Feature {
            id,
            name,
            worktrees: vec![worktree],
        });
        Ok(id)
    }

    /// Idempotente si el worktree ya es de esta feature.
    pub fn attach_worktree(
        &mut self,
        feature: FeatureId,
        worktree: WorktreeId,
    ) -> DomainResult<()> {
        self.feature(feature)?;
        if self.feature_of_worktree(worktree) == Some(feature) {
            return Ok(());
        }
        self.ensure_assignable(worktree)?;
        self.feature_mut(feature)?.worktrees.push(worktree);
        Ok(())
    }

    pub fn detach_worktree(
        &mut self,
        feature: FeatureId,
        worktree: WorktreeId,
    ) -> DomainResult<()> {
        let f = self.feature(feature)?;
        if !f.worktrees.contains(&worktree) {
            return Err(DomainError::WorktreeNotInFeature { worktree, feature });
        }
        if f.worktrees.len() == 1 {
            return Err(DomainError::LastWorktree(feature));
        }
        if let Some(agent) = self.agents.iter().find(|a| a.worktree == worktree) {
            return Err(DomainError::WorktreeInUse {
                worktree,
                agent: agent.id,
            });
        }
        self.feature_mut(feature)?
            .worktrees
            .retain(|w| *w != worktree);
        Ok(())
    }

    /// Elimina la feature y sus agentes inactivos. Devuelve la feature para que la
    /// app decida si elimina también sus worktrees en git.
    pub fn delete_feature(&mut self, feature: FeatureId) -> DomainResult<Feature> {
        let idx = self
            .features
            .iter()
            .position(|f| f.id == feature)
            .ok_or(DomainError::FeatureNotFound(feature))?;
        if self.agents_of(feature).any(|a| a.status.is_active()) {
            return Err(DomainError::AgentsStillRunning(feature));
        }
        self.agents.retain(|a| a.feature != feature);
        Ok(self.features.remove(idx))
    }

    // ── Agentes ──────────────────────────────────────────────────────────────

    /// Registra un agente en estado `Starting`. Lanzar el proceso es tarea de la app.
    pub fn add_agent(
        &mut self,
        feature: FeatureId,
        worktree: WorktreeId,
        provider: ProviderId,
    ) -> DomainResult<AgentId> {
        if !self.feature(feature)?.has_worktree(worktree) {
            return Err(DomainError::WorktreeNotInFeature { worktree, feature });
        }
        if self.worktree(worktree)?.missing {
            return Err(DomainError::WorktreeMissing(worktree));
        }
        let id = AgentId(self.next_id());
        let name = self.default_agent_name(id);
        self.agents.push(Agent {
            id,
            feature,
            worktree,
            provider,
            status: AgentStatus::Starting,
            name: Some(name),
        });
        Ok(id)
    }

    /// Cambia el nombre visible del agente. Los nombres pueden repetirse.
    pub fn rename_agent(&mut self, agent: AgentId, name: AgentName) -> DomainResult<()> {
        self.agents
            .iter_mut()
            .find(|a| a.id == agent)
            .ok_or(DomainError::AgentNotFound(agent))?
            .name = Some(name);
        Ok(())
    }

    /// Da un nombre por defecto a los agentes guardados sin nombre. Devuelve si cambió algo.
    pub fn name_unnamed_agents(&mut self) -> bool {
        let unnamed: Vec<AgentId> = self
            .agents
            .iter()
            .filter(|a| a.name.is_none())
            .map(|a| a.id)
            .collect();
        for id in &unnamed {
            let name = self.default_agent_name(*id);
            if let Some(a) = self.agents.iter_mut().find(|a| a.id == *id) {
                a.name = Some(name);
            }
        }
        !unnamed.is_empty()
    }

    fn default_agent_name(&self, id: AgentId) -> AgentName {
        AgentName::pick_default(id.0, |candidate| {
            self.agents
                .iter()
                .any(|a| a.name.as_ref().is_some_and(|n| n.as_str() == candidate))
        })
    }

    pub fn set_agent_status(&mut self, agent: AgentId, status: AgentStatus) -> DomainResult<()> {
        let a = self
            .agents
            .iter_mut()
            .find(|a| a.id == agent)
            .ok_or(DomainError::AgentNotFound(agent))?;
        if !a.status.can_transition_to(&status) {
            return Err(DomainError::InvalidTransition {
                agent,
                from: a.status.name(),
                to: status.name(),
            });
        }
        a.status = status;
        Ok(())
    }

    pub fn remove_agent(&mut self, agent: AgentId) -> DomainResult<Agent> {
        let idx = self
            .agents
            .iter()
            .position(|a| a.id == agent)
            .ok_or(DomainError::AgentNotFound(agent))?;
        if self.agents[idx].status.is_active() {
            return Err(DomainError::AgentStillRunning(agent));
        }
        Ok(self.agents.remove(idx))
    }

    /// Tras reabrir la app ningún proceso sigue vivo (ADR-013): los agentes activos
    /// pasan a `Exited { code: None }`.
    pub fn reset_after_restart(&mut self) {
        for agent in &mut self.agents {
            if agent.status.is_active() {
                agent.status = AgentStatus::Exited { code: None };
            }
        }
    }

    // ── Invariantes ──────────────────────────────────────────────────────────

    /// Verifica todas las invariantes. Los métodos públicos nunca la rompen; se usa
    /// tras deserializar (datos externos) y en las pruebas de propiedades.
    pub fn check_invariants(&self) -> DomainResult<()> {
        let fail = |msg: String| Err(DomainError::Invariant(msg));

        let all_ids = self
            .features
            .iter()
            .map(|f| f.id.0)
            .chain(self.worktrees.iter().map(|w| w.id.0))
            .chain(self.agents.iter().map(|a| a.id.0));
        let mut seen = HashSet::new();
        for id in all_ids {
            if id == 0 || id >= self.next_id || !seen.insert(id) {
                return fail(format!("id {id} duplicado o fuera de rango"));
            }
        }

        let mut paths = HashSet::new();
        for wt in &self.worktrees {
            if !paths.insert(&wt.path) {
                return fail(format!("ruta de worktree duplicada: {}", wt.path.display()));
            }
        }

        let mut names = HashSet::new();
        let mut owner: HashMap<WorktreeId, FeatureId> = HashMap::new();
        for f in &self.features {
            if f.worktrees.is_empty() {
                return fail(format!("la feature {} no tiene worktrees", f.id));
            }
            if !names.insert(f.name.key()) {
                return fail(format!("nombre de feature duplicado: {}", f.name));
            }
            for wt in &f.worktrees {
                if self.worktree(*wt).is_err() {
                    return fail(format!("la feature {} referencia {wt} inexistente", f.id));
                }
                if let Some(other) = owner.insert(*wt, f.id) {
                    return fail(format!("{wt} pertenece a {other} y a {}", f.id));
                }
            }
        }

        for a in &self.agents {
            match self.feature(a.feature) {
                Err(_) => {
                    return fail(format!(
                        "el agente {} referencia {} inexistente",
                        a.id, a.feature
                    ))
                }
                Ok(f) if !f.has_worktree(a.worktree) => {
                    return fail(format!(
                        "el agente {} usa {} fuera de su feature {}",
                        a.id, a.worktree, a.feature
                    ))
                }
                Ok(_) => {}
            }
        }
        Ok(())
    }

    // ── Privado ──────────────────────────────────────────────────────────────

    fn feature_mut(&mut self, id: FeatureId) -> DomainResult<&mut Feature> {
        self.features
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or(DomainError::FeatureNotFound(id))
    }

    fn worktree_index(&self, id: WorktreeId) -> DomainResult<usize> {
        self.worktrees
            .iter()
            .position(|w| w.id == id)
            .ok_or(DomainError::WorktreeNotFound(id))
    }

    fn ensure_assignable(&self, worktree: WorktreeId) -> DomainResult<()> {
        if self.worktree(worktree)?.missing {
            return Err(DomainError::WorktreeMissing(worktree));
        }
        if let Some(feature) = self.feature_of_worktree(worktree) {
            return Err(DomainError::WorktreeAlreadyAssigned { worktree, feature });
        }
        Ok(())
    }
}
