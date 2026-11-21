//! Reglas de presentación puras (sin Leptos ni JS): se prueban con `cargo test`.

use gryphus_ipc::{
    AgentDto, AgentStatusDto, BranchDto, FeatureDto, FileDiffDto, FileStateDto, FileStatusDto,
    LineKindDto, ProjectDto, WorktreeDto,
};

pub fn main_worktree(p: &ProjectDto) -> Option<&WorktreeDto> {
    p.worktrees.iter().find(|w| w.is_main)
}

pub fn worktree(p: &ProjectDto, id: u64) -> Option<&WorktreeDto> {
    p.worktrees.iter().find(|w| w.id == id)
}

/// `feat/login`, `main (principal)`, `HEAD desacoplado`, `… (falta)`.
pub fn worktree_label(w: &WorktreeDto) -> String {
    let mut label = w
        .branch
        .clone()
        .unwrap_or_else(|| "HEAD desacoplado".to_owned());
    if w.is_main {
        label.push_str(" (principal)");
    }
    if w.missing {
        label.push_str(" (falta)");
    }
    label
}

pub fn agents_of(p: &ProjectDto, feature: u64) -> Vec<&AgentDto> {
    p.agents.iter().filter(|a| a.feature == feature).collect()
}

pub fn feature(p: &ProjectDto, id: u64) -> Option<&FeatureDto> {
    p.features.iter().find(|f| f.id == id)
}

/// Worktrees presentes que ninguna feature usa (para "usar existente").
pub fn unassigned_worktrees(p: &ProjectDto) -> Vec<&WorktreeDto> {
    p.worktrees
        .iter()
        .filter(|w| !w.missing && !p.features.iter().any(|f| f.worktrees.contains(&w.id)))
        .collect()
}

/// Clase CSS y texto del indicador de estado de un agente.
pub fn status_badge(s: &AgentStatusDto) -> (&'static str, String) {
    match s {
        AgentStatusDto::Starting => ("starting", "iniciando".into()),
        AgentStatusDto::Running => ("running", "activo".into()),
        AgentStatusDto::Exited { code: Some(0) } => ("exited", "terminado".into()),
        AgentStatusDto::Exited { code: Some(c) } => ("exited-error", format!("terminado ({c})")),
        AgentStatusDto::Exited { code: None } => ("exited", "detenido".into()),
        AgentStatusDto::Failed { reason } => ("failed", format!("error: {reason}")),
    }
}

pub fn join_path(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

pub fn file_mark(statuses: &[FileStatusDto], path: &str) -> Option<FileStateDto> {
    statuses.iter().find(|s| s.path == path).map(|s| s.state)
}

/// Un directorio "tiene cambios" si algún archivo cambiado está dentro.
pub fn dir_changed(statuses: &[FileStatusDto], dir: &str) -> bool {
    let prefix = format!("{dir}/");
    statuses.iter().any(|s| s.path.starts_with(&prefix))
}

pub fn diff_counts(file: &FileDiffDto) -> (usize, usize) {
    file.hunks
        .iter()
        .flat_map(|h| &h.lines)
        .fold((0, 0), |(a, r), l| match l.kind {
            LineKindDto::Added => (a + 1, r),
            LineKindDto::Removed => (a, r + 1),
            LineKindDto::Context => (a, r),
        })
}

/// Índice de la pestaña que queda activa al cerrar la activa en `closed`, sobre la
/// lista ya sin ella (`remaining` pestañas): la de su derecha, o la última.
pub fn tab_after_close(remaining: usize, closed: usize) -> Option<usize> {
    if remaining == 0 {
        None
    } else {
        Some(closed.min(remaining - 1))
    }
}

/// Carpeta a mostrar junto al nombre de una pestaña cuando otra pestaña tiene el
/// mismo nombre de archivo (como VS Code). `None` si el nombre es único.
pub fn tab_hint<'a>(paths: impl IntoIterator<Item = &'a str>, path: &str) -> Option<String> {
    let split = |p: &'a str| p.rsplit_once('/').unwrap_or(("", p));
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let duplicated = paths
        .into_iter()
        .any(|p| p != path && split(p).1 == name);
    duplicated.then(|| if dir.is_empty() { "./".into() } else { dir.to_owned() })
}

/// `↑2 ↓1`, o vacío si está al día o no tiene upstream.
pub fn tracking_text(b: &BranchDto) -> String {
    let mut parts = Vec::new();
    if b.ahead > 0 {
        parts.push(format!("↑{}", b.ahead));
    }
    if b.behind > 0 {
        parts.push(format!("↓{}", b.behind));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gryphus_ipc::{DiffLineDto, HunkDto};

    fn wt(id: u64, branch: Option<&str>, is_main: bool) -> WorktreeDto {
        WorktreeDto {
            id,
            path: format!("/wt/{id}"),
            branch: branch.map(str::to_owned),
            is_main,
            missing: false,
        }
    }

    fn project() -> ProjectDto {
        ProjectDto {
            id: "x".into(),
            root: "/r".into(),
            name: "r".into(),
            features: vec![FeatureDto {
                id: 10,
                name: "login".into(),
                worktrees: vec![2],
            }],
            worktrees: vec![
                wt(1, Some("main"), true),
                wt(2, Some("feat/login"), false),
                wt(3, None, false),
            ],
            agents: vec![AgentDto {
                id: 20,
                feature: 10,
                worktree: 2,
                provider: "claude".into(),
                status: AgentStatusDto::Running,
            }],
            providers: vec![],
        }
    }

    fn status(path: &str, state: FileStateDto) -> FileStatusDto {
        FileStatusDto {
            path: path.into(),
            state,
            from: None,
            staged: false,
        }
    }

    #[test]
    fn worktree_queries() {
        let p = project();
        assert_eq!(main_worktree(&p).unwrap().id, 1);
        assert_eq!(worktree_label(&p.worktrees[0]), "main (principal)");
        assert_eq!(worktree_label(&p.worktrees[2]), "HEAD desacoplado");
        let free: Vec<_> = unassigned_worktrees(&p).iter().map(|w| w.id).collect();
        assert_eq!(free, [1, 3]);
        assert_eq!(agents_of(&p, 10).len(), 1);
        assert_eq!(feature(&p, 10).unwrap().name, "login");
    }

    #[test]
    fn badges_describe_agent_state() {
        assert_eq!(status_badge(&AgentStatusDto::Running).0, "running");
        assert_eq!(
            status_badge(&AgentStatusDto::Exited { code: Some(3) }).1,
            "terminado (3)"
        );
        assert_eq!(
            status_badge(&AgentStatusDto::Exited { code: None }).1,
            "detenido"
        );
    }

    #[test]
    fn paths_and_marks() {
        assert_eq!(join_path("", "a.rs"), "a.rs");
        assert_eq!(join_path("src", "a.rs"), "src/a.rs");
        let st = [status("src/a.rs", FileStateDto::Modified)];
        assert_eq!(file_mark(&st, "src/a.rs"), Some(FileStateDto::Modified));
        assert!(dir_changed(&st, "src"));
        assert!(!dir_changed(&st, "sr"), "no confunde prefijos");
    }

    #[test]
    fn diff_counts_added_and_removed() {
        let line = |kind| DiffLineDto {
            kind,
            text: String::new(),
        };
        let file = FileDiffDto {
            path: "a".into(),
            binary: false,
            hunks: vec![HunkDto {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 2,
                lines: vec![
                    line(LineKindDto::Context),
                    line(LineKindDto::Added),
                    line(LineKindDto::Added),
                    line(LineKindDto::Removed),
                ],
            }],
        };
        assert_eq!(diff_counts(&file), (2, 1));
    }

    #[test]
    fn tracking() {
        let b = |ahead, behind| BranchDto {
            name: "x".into(),
            is_remote: false,
            upstream: None,
            ahead,
            behind,
            checked_out: true,
        };
        assert_eq!(tracking_text(&b(2, 1)), "↑2 ↓1");
        assert_eq!(tracking_text(&b(0, 0)), "");
    }

    #[test]
    fn closing_a_tab_activates_its_right_neighbour_or_the_last() {
        assert_eq!(tab_after_close(0, 0), None, "no quedan pestañas");
        assert_eq!(tab_after_close(2, 0), Some(0), "la de la derecha ocupa su lugar");
        assert_eq!(tab_after_close(2, 1), Some(1));
        assert_eq!(tab_after_close(2, 2), Some(1), "era la última: la anterior");
    }

    #[test]
    fn tab_hint_only_for_repeated_names() {
        let tabs = ["src/main.rs", "ui/src/main.rs", "Cargo.toml", "main.rs"];
        assert_eq!(tab_hint(tabs, "src/main.rs").as_deref(), Some("src"));
        assert_eq!(tab_hint(tabs, "ui/src/main.rs").as_deref(), Some("ui/src"));
        assert_eq!(tab_hint(tabs, "main.rs").as_deref(), Some("./"));
        assert_eq!(tab_hint(tabs, "Cargo.toml"), None);
    }
}
