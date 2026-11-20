//! Traducción dominio ⇄ DTO de `gryphus-ipc` (ADR-011).

use gryphus_app::{AppError, GitOverview, WorktreeChoice};
use gryphus_core::ports::{
    AgentChunk, AgentExit, Diff, DiffBase, DiffLine, DirEntry, EntryKind, FileContent, FileState,
    FileStatus,
};
use gryphus_core::{AgentStatus, BranchName, Config, Project, ProviderId, WorktreeId};
use gryphus_ipc::stream::{self, StreamMsg};
use gryphus_ipc::*;

pub fn error(e: AppError) -> IpcError {
    IpcError {
        code: e.code().to_owned(),
        message: e.to_string(),
    }
}

pub fn invalid(message: impl Into<String>) -> IpcError {
    IpcError {
        code: "INVALID_INPUT".into(),
        message: message.into(),
    }
}

pub fn project(p: &Project, config: &Config, available: impl Fn(&str) -> bool) -> ProjectDto {
    let root = p.root().to_string_lossy().into_owned();
    ProjectDto {
        id: p.id().as_str().to_owned(),
        name: p
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.clone()),
        root,
        features: p
            .features()
            .iter()
            .map(|f| FeatureDto {
                id: f.id().get(),
                name: f.name().as_str().to_owned(),
                worktrees: f.worktrees().iter().map(|w| w.get()).collect(),
            })
            .collect(),
        worktrees: p
            .worktrees()
            .iter()
            .map(|w| WorktreeDto {
                id: w.id().get(),
                path: w.path().to_string_lossy().into_owned(),
                branch: w.branch().map(|b| b.as_str().to_owned()),
                is_main: w.is_main(),
                missing: w.is_missing(),
            })
            .collect(),
        agents: p
            .agents()
            .iter()
            .map(|a| AgentDto {
                id: a.id().get(),
                feature: a.feature().get(),
                worktree: a.worktree().get(),
                provider: a.provider().as_str().to_owned(),
                status: status(a.status()),
            })
            .collect(),
        providers: ProviderId::ALL
            .into_iter()
            .map(|id| {
                let command = config
                    .agents
                    .get(&id)
                    .map(|c| c.command.clone())
                    .unwrap_or_default();
                ProviderDto {
                    id: id.as_str().to_owned(),
                    available: !command.is_empty() && available(&command),
                    command,
                }
            })
            .collect(),
    }
}

pub fn status(s: &AgentStatus) -> AgentStatusDto {
    match s {
        AgentStatus::Starting => AgentStatusDto::Starting,
        AgentStatus::Running => AgentStatusDto::Running,
        AgentStatus::Exited { code } => AgentStatusDto::Exited { code: *code },
        AgentStatus::Failed { reason } => AgentStatusDto::Failed {
            reason: reason.clone(),
        },
    }
}

pub fn provider(id: &str) -> Result<ProviderId, IpcError> {
    id.parse()
        .map_err(|e: gryphus_core::agent::UnknownProvider| invalid(e.to_string()))
}

pub fn worktree_choice(c: WorktreeChoiceDto) -> WorktreeChoice {
    match c {
        WorktreeChoiceDto::New { base } => WorktreeChoice::New {
            base: base.filter(|b| !b.trim().is_empty()),
        },
        WorktreeChoiceDto::Existing { id } => WorktreeChoice::Existing(WorktreeId::from_raw(id)),
    }
}

pub fn diff_base(base: Option<String>) -> Result<DiffBase, IpcError> {
    match base {
        None => Ok(DiffBase::Head),
        Some(b) => BranchName::parse(&b)
            .map(DiffBase::MergeBase)
            .map_err(|e| invalid(e.to_string())),
    }
}

pub fn dir_entry(e: DirEntry) -> DirEntryDto {
    DirEntryDto {
        name: e.name,
        kind: match e.kind {
            EntryKind::File => EntryKindDto::File,
            EntryKind::Dir => EntryKindDto::Dir,
            EntryKind::Symlink => EntryKindDto::Symlink,
        },
        ignored: e.ignored,
    }
}

pub fn file_content(c: FileContent) -> FileContentDto {
    match c {
        FileContent::Text(text) => FileContentDto::Text { text },
        FileContent::Binary { size } => FileContentDto::Binary { size },
        FileContent::TooLarge { size, limit } => FileContentDto::TooLarge { size, limit },
    }
}

pub fn file_status(s: FileStatus) -> FileStatusDto {
    let (state, from) = match s.state {
        FileState::Added => (FileStateDto::Added, None),
        FileState::Modified => (FileStateDto::Modified, None),
        FileState::Deleted => (FileStateDto::Deleted, None),
        FileState::Renamed { from } => (FileStateDto::Renamed, Some(from.as_str().to_owned())),
        FileState::Untracked => (FileStateDto::Untracked, None),
        FileState::Conflicted => (FileStateDto::Conflicted, None),
    };
    FileStatusDto {
        path: s.path.as_str().to_owned(),
        state,
        from,
        staged: s.staged,
    }
}

pub fn diff(d: Diff) -> DiffDto {
    DiffDto {
        files: d
            .files
            .into_iter()
            .map(|f| FileDiffDto {
                path: f.path.as_str().to_owned(),
                binary: f.binary,
                hunks: f
                    .hunks
                    .into_iter()
                    .map(|h| HunkDto {
                        old_start: h.old_start,
                        old_lines: h.old_lines,
                        new_start: h.new_start,
                        new_lines: h.new_lines,
                        lines: h
                            .lines
                            .into_iter()
                            .map(|l| match l {
                                DiffLine::Context(text) => DiffLineDto {
                                    kind: LineKindDto::Context,
                                    text,
                                },
                                DiffLine::Added(text) => DiffLineDto {
                                    kind: LineKindDto::Added,
                                    text,
                                },
                                DiffLine::Removed(text) => DiffLineDto {
                                    kind: LineKindDto::Removed,
                                    text,
                                },
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

pub fn overview(o: GitOverview) -> GitOverviewDto {
    GitOverviewDto {
        head: o.head.map(|b| b.as_str().to_owned()),
        branches: o
            .branches
            .into_iter()
            .map(|b| BranchDto {
                name: b.name.as_str().to_owned(),
                is_remote: b.is_remote,
                upstream: b.upstream,
                ahead: b.ahead,
                behind: b.behind,
                checked_out: b.checked_out,
            })
            .collect(),
    }
}

/// Mensaje binario del Channel para un chunk de salida.
pub fn chunk(c: AgentChunk) -> Vec<u8> {
    match c {
        AgentChunk::Scrollback(b) => stream::encode_bytes(false, &b),
        AgentChunk::Data(b) => stream::encode_bytes(true, &b),
        AgentChunk::Exited(AgentExit::Code(code)) => stream::encode(&StreamMsg::Exited(code)),
        AgentChunk::Exited(AgentExit::Failed(reason)) => stream::encode(&StreamMsg::Failed(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gryphus_core::ports::WorktreeInfo;
    use gryphus_core::FeatureName;
    use std::path::PathBuf;

    #[test]
    fn project_dto_reflects_domain_and_provider_availability() {
        let mut p = Project::new(PathBuf::from("/repos/demo"));
        p.sync_worktrees(&[WorktreeInfo {
            path: PathBuf::from("/repos/demo"),
            branch: Some(BranchName::parse("main").unwrap()),
            head: None,
            is_main: true,
            locked: false,
        }]);
        let wt = p.worktrees()[0].id();
        let f = p
            .create_feature(FeatureName::parse("login").unwrap(), wt)
            .unwrap();
        p.add_agent(f, wt, ProviderId::Codex).unwrap();

        let dto = project(&p, &Config::default(), |cmd| cmd == "claude");
        assert_eq!(dto.name, "demo");
        assert_eq!(dto.features[0].name, "login");
        assert_eq!(dto.worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(dto.agents[0].provider, "codex");
        assert_eq!(dto.agents[0].status, AgentStatusDto::Starting);
        let available: Vec<_> = dto
            .providers
            .iter()
            .map(|p| (p.id.as_str(), p.available))
            .collect();
        assert_eq!(
            available,
            [("opencode", false), ("claude", true), ("codex", false)]
        );
    }

    #[test]
    fn requests_are_validated() {
        assert!(provider("gpt").is_err());
        assert_eq!(provider("claude").unwrap(), ProviderId::Claude);
        assert_eq!(diff_base(None).unwrap(), DiffBase::Head);
        assert!(diff_base(Some("a..b".into())).is_err());
        assert_eq!(
            worktree_choice(WorktreeChoiceDto::New {
                base: Some("  ".into())
            }),
            WorktreeChoice::New { base: None }
        );
    }

    #[test]
    fn chunks_encode_to_the_stream_format() {
        assert_eq!(
            stream::decode(&chunk(AgentChunk::Data(b"x".to_vec()))),
            Some(StreamMsg::Data(b"x".to_vec()))
        );
        assert_eq!(
            stream::decode(&chunk(AgentChunk::Exited(AgentExit::Code(None)))),
            Some(StreamMsg::Exited(None))
        );
    }
}
