//! [`FakeGit`]: modelo en memoria de un repo con commits, branches y worktrees.
//!
//! Simplificaciones (no afectan al contrato): sin índice (todo cambio es
//! `staged: false`), sin remotos, y el diff de cada archivo es un único hunk.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gryphus_core::ports::{
    Branch, Diff, DiffBase, DiffLine, FileDiff, FileState, FileStatus, GitPort, Hunk, NewWorktree,
    PortError, PortErrorKind, PortResult, RepoInfo, WorktreeInfo,
};
use gryphus_core::{BranchName, RelPath};

use super::{err, lock, FailSwitch};

type Tree = BTreeMap<RelPath, String>;

struct Commit {
    parent: Option<usize>,
    tree: Tree,
}

struct Wt {
    path: PathBuf,
    branch: Option<BranchName>,
    head: usize,
    /// Contenido del directorio de trabajo.
    files: Tree,
    is_main: bool,
    /// Su directorio se borró a mano: git aún lo tiene registrado (`prunable`).
    vanished: bool,
}

struct State {
    root: PathBuf,
    commits: Vec<Commit>,
    branches: BTreeMap<BranchName, usize>,
    worktrees: Vec<Wt>,
}

pub struct FakeGit {
    state: Mutex<State>,
    fail: FailSwitch,
}

impl FakeGit {
    /// Repo vacío: commit raíz sin archivos en `main`, worktree principal en `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let main = branch("main");
        Self {
            state: Mutex::new(State {
                root: root.clone(),
                commits: vec![Commit {
                    parent: None,
                    tree: Tree::new(),
                }],
                branches: BTreeMap::from([(main.clone(), 0)]),
                worktrees: vec![Wt {
                    path: root,
                    branch: Some(main),
                    head: 0,
                    files: Tree::new(),
                    is_main: true,
                    vanished: false,
                }],
            }),
            fail: FailSwitch::default(),
        }
    }

    pub fn root(&self) -> PathBuf {
        lock(&self.state).root.clone()
    }

    pub fn fail_next(&self, error: PortError) {
        self.fail.set(error);
    }

    /// Simula que alguien (usuario o agente) escribe un archivo en el worktree.
    pub fn write_file(&self, worktree: &Path, rel: &str, content: &str) {
        let mut s = lock(&self.state);
        let wt = s.wt_mut(worktree).expect("worktree desconocido");
        wt.files.insert(rel_path(rel), content.to_owned());
    }

    pub fn delete_file(&self, worktree: &Path, rel: &str) {
        let mut s = lock(&self.state);
        let wt = s.wt_mut(worktree).expect("worktree desconocido");
        wt.files.remove(&rel_path(rel));
    }

    /// Equivale a `git add -A && git commit`.
    pub fn commit_all(&self, worktree: &Path, _message: &str) {
        let mut s = lock(&self.state);
        let idx = s.wt_index(worktree).expect("worktree desconocido");
        let parent = s.worktrees[idx].head;
        let tree = s.worktrees[idx].files.clone();
        s.commits.push(Commit {
            parent: Some(parent),
            tree,
        });
        let id = s.commits.len() - 1;
        s.worktrees[idx].head = id;
        if let Some(b) = s.worktrees[idx].branch.clone() {
            s.branches.insert(b, id);
        }
    }

    /// Crea una branch en el HEAD del worktree principal.
    pub fn create_branch(&self, name: &str) {
        let mut s = lock(&self.state);
        let head = s.worktrees[0].head;
        s.branches.insert(branch(name), head);
    }

    /// Simula que el usuario borra a mano el directorio de un worktree.
    pub fn delete_worktree_dir(&self, worktree: &Path) {
        let mut s = lock(&self.state);
        let wt = s.wt_mut(worktree).expect("worktree desconocido");
        wt.vanished = true;
        wt.files.clear();
    }
}

fn branch(name: &str) -> BranchName {
    BranchName::parse(name).expect("nombre de branch válido")
}

fn rel_path(rel: &str) -> RelPath {
    RelPath::parse(rel).expect("ruta relativa válida")
}

fn oid(commit: usize) -> String {
    format!("{commit:040x}")
}

impl State {
    fn check_root(&self, root: &Path) -> PortResult<()> {
        if root == self.root {
            Ok(())
        } else {
            err(
                PortErrorKind::NotFound,
                format!("no es un repo: {}", root.display()),
            )
        }
    }

    fn wt_index(&self, path: &Path) -> PortResult<usize> {
        match self.worktrees.iter().position(|w| w.path == path) {
            Some(i) => Ok(i),
            None => err(
                PortErrorKind::NotFound,
                format!("worktree desconocido: {}", path.display()),
            ),
        }
    }

    /// Un worktree cuyo directorio ya no existe no admite status/diff.
    fn wt(&self, path: &Path) -> PortResult<&Wt> {
        let wt = &self.worktrees[self.wt_index(path)?];
        if wt.vanished {
            return err(
                PortErrorKind::NotFound,
                format!("no existe: {}", path.display()),
            );
        }
        Ok(wt)
    }

    fn wt_mut(&mut self, path: &Path) -> PortResult<&mut Wt> {
        self.wt_index(path).map(|i| &mut self.worktrees[i])
    }

    fn info(&self, wt: &Wt) -> WorktreeInfo {
        WorktreeInfo {
            path: wt.path.clone(),
            branch: wt.branch.clone(),
            head: Some(oid(wt.head)),
            is_main: wt.is_main,
            locked: false,
        }
    }

    fn ancestors(&self, mut commit: usize) -> Vec<usize> {
        let mut out = vec![commit];
        while let Some(parent) = self.commits[commit].parent {
            out.push(parent);
            commit = parent;
        }
        out
    }

    fn merge_base(&self, a: usize, b: usize) -> usize {
        let of_a: HashSet<usize> = self.ancestors(a).into_iter().collect();
        self.ancestors(b)
            .into_iter()
            .find(|c| of_a.contains(c))
            .unwrap_or(0)
    }
}

impl GitPort for FakeGit {
    fn repo_info(&self, root: &Path) -> PortResult<RepoInfo> {
        self.fail.check()?;
        let s = lock(&self.state);
        s.check_root(root)?;
        Ok(RepoInfo {
            root: s.root.clone(),
            head: s.worktrees[0].branch.clone(),
        })
    }

    fn list_branches(&self, root: &Path) -> PortResult<Vec<Branch>> {
        self.fail.check()?;
        let s = lock(&self.state);
        s.check_root(root)?;
        Ok(s.branches
            .keys()
            .map(|name| Branch {
                name: name.clone(),
                is_remote: false,
                upstream: None,
                ahead: 0,
                behind: 0,
                checked_out: s.worktrees.iter().any(|w| w.branch.as_ref() == Some(name)),
            })
            .collect())
    }

    fn list_worktrees(&self, root: &Path) -> PortResult<Vec<WorktreeInfo>> {
        self.fail.check()?;
        let s = lock(&self.state);
        s.check_root(root)?;
        Ok(s.worktrees
            .iter()
            .filter(|w| !w.vanished)
            .map(|w| s.info(w))
            .collect())
    }

    fn prune_worktrees(&self, root: &Path) -> PortResult<()> {
        self.fail.check()?;
        let mut s = lock(&self.state);
        s.check_root(root)?;
        s.worktrees.retain(|w| !w.vanished);
        Ok(())
    }

    fn add_worktree(&self, root: &Path, req: &NewWorktree) -> PortResult<WorktreeInfo> {
        self.fail.check()?;
        let mut s = lock(&self.state);
        s.check_root(root)?;
        if s.worktrees.iter().any(|w| w.path == req.path) {
            return err(
                PortErrorKind::AlreadyExists,
                "ya existe un worktree en esa ruta",
            );
        }
        let head = if req.create_branch {
            if s.branches.contains_key(&req.branch) {
                return err(
                    PortErrorKind::AlreadyExists,
                    format!("la branch {} ya existe", req.branch),
                );
            }
            let base = match &req.base {
                None => s.worktrees[0].head,
                Some(reference) => match BranchName::parse(reference)
                    .ok()
                    .and_then(|b| s.branches.get(&b).copied())
                {
                    Some(commit) => commit,
                    None => {
                        return err(
                            PortErrorKind::NotFound,
                            format!("base desconocida: {reference}"),
                        )
                    }
                },
            };
            s.branches.insert(req.branch.clone(), base);
            base
        } else {
            let Some(&head) = s.branches.get(&req.branch) else {
                return err(
                    PortErrorKind::NotFound,
                    format!("la branch {} no existe", req.branch),
                );
            };
            if s.worktrees
                .iter()
                .any(|w| w.branch.as_ref() == Some(&req.branch))
            {
                return err(
                    PortErrorKind::Conflict,
                    format!("la branch {} ya está en otro worktree", req.branch),
                );
            }
            head
        };
        let files = s.commits[head].tree.clone();
        s.worktrees.push(Wt {
            path: req.path.clone(),
            branch: Some(req.branch.clone()),
            head,
            files,
            is_main: false,
            vanished: false,
        });
        Ok(s.info(s.worktrees.last().expect("recién añadido")))
    }

    fn remove_worktree(&self, root: &Path, worktree: &Path, force: bool) -> PortResult<()> {
        self.fail.check()?;
        let mut s = lock(&self.state);
        s.check_root(root)?;
        let idx = s.wt_index(worktree)?;
        let wt = &s.worktrees[idx];
        if wt.vanished {
            return err(
                PortErrorKind::NotFound,
                format!("no existe: {}", worktree.display()),
            );
        }
        if wt.is_main {
            return err(
                PortErrorKind::InvalidInput,
                "no se puede eliminar el worktree principal",
            );
        }
        if !force && wt.files != s.commits[wt.head].tree {
            return err(
                PortErrorKind::Conflict,
                "el worktree tiene cambios sin commit",
            );
        }
        s.worktrees.remove(idx);
        Ok(())
    }

    fn status(&self, worktree: &Path) -> PortResult<Vec<FileStatus>> {
        self.fail.check()?;
        let s = lock(&self.state);
        let wt = s.wt(worktree)?;
        let head = &s.commits[wt.head].tree;
        let mut out: Vec<FileStatus> = Vec::new();
        for (path, content) in &wt.files {
            let state = match head.get(path) {
                None => FileState::Untracked,
                Some(old) if old != content => FileState::Modified,
                Some(_) => continue,
            };
            out.push(FileStatus {
                path: path.clone(),
                state,
                staged: false,
            });
        }
        for path in head.keys().filter(|p| !wt.files.contains_key(*p)) {
            out.push(FileStatus {
                path: path.clone(),
                state: FileState::Deleted,
                staged: false,
            });
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn diff(&self, worktree: &Path, base: &DiffBase) -> PortResult<Diff> {
        self.fail.check()?;
        let s = lock(&self.state);
        let wt = s.wt(worktree)?;
        let base_commit = match base {
            DiffBase::Head => wt.head,
            DiffBase::MergeBase(b) => match s.branches.get(b) {
                Some(&tip) => s.merge_base(wt.head, tip),
                None => return err(PortErrorKind::NotFound, format!("la branch {b} no existe")),
            },
        };
        let old_tree = &s.commits[base_commit].tree;
        let paths: BTreeSet<&RelPath> = old_tree.keys().chain(wt.files.keys()).collect();
        let files = paths
            .into_iter()
            .filter_map(|path| {
                let (old, new) = (old_tree.get(path), wt.files.get(path));
                if old == new {
                    return None;
                }
                let (old, new) = (
                    old.map_or("", String::as_str),
                    new.map_or("", String::as_str),
                );
                Some(FileDiff {
                    path: path.clone(),
                    old_path: None,
                    binary: false,
                    hunks: vec![Hunk {
                        old_start: 1,
                        old_lines: old.lines().count() as u32,
                        new_start: 1,
                        new_lines: new.lines().count() as u32,
                        lines: line_diff(old, new),
                    }],
                })
            })
            .collect();
        Ok(Diff { files })
    }
}

/// Diff por líneas basado en LCS (suficiente para archivos de prueba).
fn line_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push(DiffLine::Context(a[i].to_owned()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(DiffLine::Removed(a[i].to_owned()));
            i += 1;
        } else {
            out.push(DiffLine::Added(b[j].to_owned()));
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|l| DiffLine::Removed((*l).to_owned())));
    out.extend(b[j..].iter().map(|l| DiffLine::Added((*l).to_owned())));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_diff_keeps_common_lines_as_context() {
        use DiffLine::*;
        assert_eq!(
            line_diff("a\nb\nc\n", "a\nx\nc\nd\n"),
            [
                Context("a".into()),
                Removed("b".into()),
                Added("x".into()),
                Context("c".into()),
                Added("d".into()),
            ]
        );
    }
}
