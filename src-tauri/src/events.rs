//! Reenvía los `DomainEvent` de una sesión a su ventana (y solo a ella).

use gryphus_core::{DomainEvent, EventSink};
use gryphus_ipc::{event, AgentStatusEvent, FeatureEvent, FilesChangedEvent, WorktreeEvent};
use tauri::{AppHandle, Emitter};

use crate::dto;

pub struct TauriEvents {
    pub app: AppHandle,
    pub label: String,
}

impl EventSink for TauriEvents {
    fn publish(&self, e: DomainEvent) {
        let target = self.label.as_str();
        let app = &self.app;
        let _ = match e {
            DomainEvent::FeatureChanged(f) => app.emit_to(
                target,
                event::FEATURE_CHANGED,
                FeatureEvent { feature: f.get() },
            ),
            DomainEvent::FeatureRemoved(f) => app.emit_to(
                target,
                event::FEATURE_REMOVED,
                FeatureEvent { feature: f.get() },
            ),
            DomainEvent::AgentStatusChanged { agent, status } => app.emit_to(
                target,
                event::AGENT_STATUS,
                AgentStatusEvent {
                    agent: agent.get(),
                    status: dto::status(&status),
                },
            ),
            DomainEvent::FilesChanged { worktree, paths } => app.emit_to(
                target,
                event::FILES_CHANGED,
                FilesChangedEvent {
                    worktree: worktree.get(),
                    paths: paths.iter().map(|p| p.as_str().to_owned()).collect(),
                },
            ),
            DomainEvent::GitChanged { worktree } => app.emit_to(
                target,
                event::GIT_CHANGED,
                WorktreeEvent {
                    worktree: worktree.get(),
                },
            ),
            DomainEvent::WorktreesChanged => app.emit_to(target, event::WORKTREES_CHANGED, ()),
        };
    }
}
