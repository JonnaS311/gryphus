//! Contrato de `GitPort`.
//!
//! Precondición del harness: repo con branch `main` y un commit que contiene
//! `README.md` = `"hola\n"`, sin cambios pendientes.
//!
//! Semántica acordada:
//! - `status` y `diff` comparan con el directorio de trabajo.
//! - `diff` incluye los archivos sin seguimiento como añadidos (la vista de diff
//!   debe mostrar archivos nuevos creados por un agente).
//! - Las líneas de `DiffLine` no incluyen el salto de línea final.

use std::path::{Path, PathBuf};

use gryphus_core::ports::{DiffBase, DiffLine, FileState, GitPort, NewWorktree, PortErrorKind};
use gryphus_core::BranchName;

use super::{assert_kind, same_path};

pub trait GitHarness {
    fn port(&self) -> &dyn GitPort;
    /// Raíz del worktree principal.
    fn root(&self) -> PathBuf;
    /// Ruta aún inexistente donde crear un worktree.
    fn new_worktree_path(&self, name: &str) -> PathBuf;
    fn write_file(&self, worktree: &Path, rel: &str, content: &str);
    fn delete_file(&self, worktree: &Path, rel: &str);
    /// `git add -A && git commit`.
    fn commit_all(&self, worktree: &Path, message: &str);
    /// Crea una branch en el HEAD de `main` sin hacer checkout.
    fn create_branch(&self, name: &str);
}

fn branch(name: &str) -> BranchName {
    BranchName::parse(name).unwrap()
}

fn request(path: PathBuf, name: &str, create_branch: bool, base: Option<&str>) -> NewWorktree {
    NewWorktree {
        path,
        branch: branch(name),
        create_branch,
        base: base.map(str::to_owned),
    }
}

/// Crea `feat/<name>` en un worktree nuevo y devuelve su ruta.
fn add_feature_worktree<H: GitHarness>(h: &H, name: &str) -> PathBuf {
    let path = h.new_worktree_path(name);
    let branch_name = format!("feat/{name}");
    h.port()
        .add_worktree(&h.root(), &request(path.clone(), &branch_name, true, None))
        .unwrap();
    path
}

fn diff_lines<H: GitHarness>(h: &H, worktree: &Path, file: &str) -> Vec<DiffLine> {
    let diff = h.port().diff(worktree, &DiffBase::Head).unwrap();
    let file = diff
        .files
        .iter()
        .find(|f| f.path.as_str() == file)
        .unwrap_or_else(|| panic!("{file} no aparece en el diff: {diff:?}"));
    file.hunks.iter().flat_map(|h| h.lines.clone()).collect()
}

pub fn repo_info_reports_main_branch<H: GitHarness>(h: &H) {
    let info = h.port().repo_info(&h.root()).unwrap();
    assert_eq!(info.head, Some(branch("main")));
    assert!(same_path(&info.root, &h.root()));
}

pub fn lists_only_the_main_worktree_initially<H: GitHarness>(h: &H) {
    let worktrees = h.port().list_worktrees(&h.root()).unwrap();
    assert_eq!(worktrees.len(), 1, "{worktrees:?}");
    let main = &worktrees[0];
    assert!(main.is_main);
    assert_eq!(main.branch, Some(branch("main")));
    assert!(main.head.is_some());
    assert!(same_path(&main.path, &h.root()));
}

pub fn adds_worktree_on_new_branch<H: GitHarness>(h: &H) {
    let path = h.new_worktree_path("login");
    let info = h
        .port()
        .add_worktree(&h.root(), &request(path.clone(), "feat/login", true, None))
        .unwrap();
    assert!(same_path(&info.path, &path));
    assert_eq!(info.branch, Some(branch("feat/login")));
    assert!(!info.is_main);

    let worktrees = h.port().list_worktrees(&h.root()).unwrap();
    assert_eq!(worktrees.len(), 2);
    assert!(worktrees[0].is_main, "el principal va primero");
    assert!(worktrees.iter().any(|w| same_path(&w.path, &path)));

    let branches = h.port().list_branches(&h.root()).unwrap();
    let created = branches
        .iter()
        .find(|b| b.name == branch("feat/login"))
        .expect("branch creada");
    assert!(created.checked_out);
}

pub fn new_worktree_is_clean<H: GitHarness>(h: &H) {
    let path = add_feature_worktree(h, "limpio");
    assert_eq!(h.port().status(&path).unwrap(), []);
}

pub fn adds_worktree_for_existing_branch<H: GitHarness>(h: &H) {
    h.create_branch("dev");
    let path = h.new_worktree_path("dev");
    let info = h
        .port()
        .add_worktree(&h.root(), &request(path, "dev", false, None))
        .unwrap();
    assert_eq!(info.branch, Some(branch("dev")));
}

pub fn rejects_creating_an_existing_branch<H: GitHarness>(h: &H) {
    let path = h.new_worktree_path("x");
    assert_kind(
        h.port()
            .add_worktree(&h.root(), &request(path, "main", true, None)),
        PortErrorKind::AlreadyExists,
    );
}

pub fn rejects_branch_checked_out_elsewhere<H: GitHarness>(h: &H) {
    let path = h.new_worktree_path("x");
    assert_kind(
        h.port()
            .add_worktree(&h.root(), &request(path, "main", false, None)),
        PortErrorKind::Conflict,
    );
}

pub fn rejects_unknown_base<H: GitHarness>(h: &H) {
    let path = h.new_worktree_path("x");
    assert_kind(
        h.port()
            .add_worktree(&h.root(), &request(path, "feat/x", true, Some("no-existe"))),
        PortErrorKind::NotFound,
    );
}

pub fn status_reports_untracked_modified_and_deleted<H: GitHarness>(h: &H) {
    let root = h.root();
    h.write_file(&root, "old.txt", "x\n");
    h.commit_all(&root, "old");
    h.delete_file(&root, "old.txt");
    h.write_file(&root, "README.md", "cambiado\n");
    h.write_file(&root, "new.txt", "n\n");

    let mut status = h.port().status(&root).unwrap();
    status.sort_by(|a, b| a.path.cmp(&b.path));
    let summary: Vec<_> = status
        .iter()
        .map(|s| (s.path.as_str(), s.state.clone(), s.staged))
        .collect();
    assert_eq!(
        summary,
        [
            ("README.md", FileState::Modified, false),
            ("new.txt", FileState::Untracked, false),
            ("old.txt", FileState::Deleted, false),
        ]
    );
}

pub fn status_is_clean_after_commit<H: GitHarness>(h: &H) {
    let root = h.root();
    h.write_file(&root, "a.txt", "a\n");
    h.commit_all(&root, "a");
    assert_eq!(h.port().status(&root).unwrap(), []);
}

pub fn diff_against_head_shows_line_changes<H: GitHarness>(h: &H) {
    let root = h.root();
    h.write_file(&root, "README.md", "hola\nmundo\n");
    let lines = diff_lines(h, &root, "README.md");
    assert!(
        lines.contains(&DiffLine::Added("mundo".into())),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| matches!(l, DiffLine::Removed(_))),
        "{lines:?}"
    );
    assert!(
        !lines.contains(&DiffLine::Added("hola".into())),
        "hola no cambió"
    );
}

pub fn diff_includes_untracked_files_as_added<H: GitHarness>(h: &H) {
    let root = h.root();
    h.write_file(&root, "nuevo.txt", "contenido\n");
    let lines = diff_lines(h, &root, "nuevo.txt");
    assert_eq!(lines, [DiffLine::Added("contenido".into())]);
}

pub fn diff_against_merge_base_only_includes_feature_work<H: GitHarness>(h: &H) {
    let wt = add_feature_worktree(h, "x");
    h.write_file(&wt, "a.txt", "a\n");
    h.commit_all(&wt, "trabajo de la feature");
    let root = h.root();
    h.write_file(&root, "b.txt", "b\n");
    h.commit_all(&root, "trabajo en main");

    let diff = h
        .port()
        .diff(&wt, &DiffBase::MergeBase(branch("main")))
        .unwrap();
    let paths: Vec<_> = diff.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.txt"]);
}

pub fn removes_clean_worktree_but_keeps_branch<H: GitHarness>(h: &H) {
    let wt = add_feature_worktree(h, "x");
    h.port().remove_worktree(&h.root(), &wt, false).unwrap();
    assert_eq!(h.port().list_worktrees(&h.root()).unwrap().len(), 1);
    let branches = h.port().list_branches(&h.root()).unwrap();
    let kept = branches
        .iter()
        .find(|b| b.name == branch("feat/x"))
        .expect("la branch se conserva");
    assert!(!kept.checked_out);
}

pub fn refuses_to_remove_dirty_worktree_without_force<H: GitHarness>(h: &H) {
    let wt = add_feature_worktree(h, "x");
    h.write_file(&wt, "sucio.txt", "x\n");
    assert_kind(
        h.port().remove_worktree(&h.root(), &wt, false),
        PortErrorKind::Conflict,
    );
    h.port().remove_worktree(&h.root(), &wt, true).unwrap();
    assert_eq!(h.port().list_worktrees(&h.root()).unwrap().len(), 1);
}

pub fn unknown_worktree_is_not_found<H: GitHarness>(h: &H) {
    assert_kind(
        h.port().status(&h.new_worktree_path("nada")),
        PortErrorKind::NotFound,
    );
}
