//! Adaptador `GitPort` sobre la CLI de `git`.
//!
//! - Solo salidas estables para máquinas: `--porcelain`, `-z`, `for-each-ref --format`.
//! - Lecturas con `GIT_OPTIONAL_LOCKS=0` para no competir por `index.lock` con
//!   los agentes que también ejecutan git en el mismo worktree.
//! - Mensajes en inglés (`LC_ALL=C`) para clasificar errores de forma fiable.
//! - En Windows no abre ventanas de consola.
//!
//! Ver docs/architecture/git.md (y ADR-005 sobre CLI frente a gix).
#![forbid(unsafe_code)]

mod cli;
mod parse;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gryphus_core::ports::{
    Branch, Diff, DiffBase, FileDiff, FileStatus, GitPort, Hunk, NewWorktree, PortError,
    PortErrorKind, PortResult, RepoInfo, WorktreeInfo,
};
use gryphus_core::{BranchName, RelPath};

pub use cli::classify_failure;

/// Tamaño máximo de un archivo sin seguimiento que se incluye como texto en el diff.
pub const UNTRACKED_DIFF_LIMIT: u64 = 1024 * 1024;

/// Implementación de [`GitPort`] que ejecuta el binario `git`.
#[derive(Debug, Clone)]
pub struct CliGit {
    program: OsString,
    env: Vec<(OsString, OsString)>,
}

impl Default for CliGit {
    fn default() -> Self {
        Self::new()
    }
}

impl CliGit {
    /// Usa `git` del `PATH`.
    pub fn new() -> Self {
        Self {
            program: "git".into(),
            env: Vec::new(),
        }
    }

    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            env: Vec::new(),
        }
    }

    /// Variable de entorno extra para cada invocación (p. ej. aislar la config en pruebas).
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

impl GitPort for CliGit {
    fn repo_info(&self, root: &Path) -> PortResult<RepoInfo> {
        let top = self.read(root, ["rev-parse", "--show-toplevel"])?;
        // Falla (exit 1) si HEAD está desacoplado: eso es `None`, no un error.
        let head = self
            .try_read(root, ["symbolic-ref", "--quiet", "--short", "HEAD"])?
            .and_then(|out| BranchName::parse(out.trim()).ok());
        Ok(RepoInfo {
            root: PathBuf::from(top.trim()),
            head,
        })
    }

    fn list_branches(&self, root: &Path) -> PortResult<Vec<Branch>> {
        let out = self.read_bytes(
            root,
            [
                "for-each-ref",
                "--format=%(refname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(worktreepath)",
                "refs/heads",
                "refs/remotes",
            ],
        )?;
        Ok(parse::branches(&out))
    }

    fn list_worktrees(&self, root: &Path) -> PortResult<Vec<WorktreeInfo>> {
        let out = self.read_bytes(root, ["worktree", "list", "--porcelain", "-z"])?;
        Ok(parse::worktrees(&out))
    }

    fn add_worktree(&self, root: &Path, req: &NewWorktree) -> PortResult<WorktreeInfo> {
        if req
            .base
            .as_deref()
            .is_some_and(|b| b.is_empty() || b.starts_with('-'))
        {
            return Err(PortError::new(
                PortErrorKind::InvalidInput,
                "referencia base inválida",
            ));
        }
        let mut args: Vec<OsString> = vec!["worktree".into(), "add".into()];
        if req.create_branch {
            args.extend([
                "-b".into(),
                req.branch.as_str().into(),
                req.path.clone().into(),
            ]);
            args.extend(req.base.clone().map(OsString::from));
        } else {
            args.extend([req.path.clone().into(), req.branch.as_str().into()]);
        }
        self.write(root, args)?;
        self.list_worktrees(root)?
            .into_iter()
            .find(|w| cli::same_path(&w.path, &req.path))
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::Io,
                    "git creó el worktree pero no aparece en la lista",
                )
            })
    }

    fn remove_worktree(&self, root: &Path, worktree: &Path, force: bool) -> PortResult<()> {
        let mut args: Vec<OsString> = vec!["worktree".into(), "remove".into()];
        if force {
            args.push("--force".into());
        }
        args.push(worktree.into());
        self.write(root, args)?;
        Ok(())
    }

    fn status(&self, worktree: &Path) -> PortResult<Vec<FileStatus>> {
        let out = self.read_bytes(
            worktree,
            ["status", "--porcelain=v2", "-z", "--untracked-files=all"],
        )?;
        let mut status = parse::status_v2(&out);
        status.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(status)
    }

    fn diff(&self, worktree: &Path, base: &DiffBase) -> PortResult<Diff> {
        let commit = match base {
            DiffBase::Head => "HEAD".to_owned(),
            DiffBase::MergeBase(branch) => self
                .try_read(worktree, ["merge-base", "HEAD", branch.as_str()])?
                .map(|sha| sha.trim().to_owned())
                .ok_or_else(|| {
                    PortError::new(
                        PortErrorKind::NotFound,
                        format!("sin merge-base con {branch}"),
                    )
                })?,
        };
        let text = self.read(
            worktree,
            [
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-renames",
                "-U3",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                &commit,
                "--",
            ],
        )?;
        let mut files = parse::unified_diff(&text);

        let untracked = self.read_bytes(
            worktree,
            ["ls-files", "--others", "--exclude-standard", "-z"],
        )?;
        for rel in parse::nul_fields(&untracked) {
            if let Ok(path) = RelPath::parse(&rel) {
                files.push(untracked_file_diff(worktree, path));
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Diff { files })
    }
}

/// Un archivo sin seguimiento se muestra como añadido por completo.
fn untracked_file_diff(worktree: &Path, path: RelPath) -> FileDiff {
    let full = path.to_path(worktree);
    let text = std::fs::metadata(&full)
        .ok()
        .filter(|m| m.len() <= UNTRACKED_DIFF_LIMIT)
        .and_then(|_| std::fs::read(&full).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .filter(|t| !t.contains('\0'));
    match text {
        Some(text) => {
            let lines: Vec<_> = text
                .lines()
                .map(|l| gryphus_core::ports::DiffLine::Added(l.trim_end_matches('\r').to_owned()))
                .collect();
            FileDiff {
                path,
                old_path: None,
                binary: false,
                hunks: vec![Hunk {
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: lines.len() as u32,
                    lines,
                }],
            }
        }
        None => FileDiff {
            path,
            old_path: None,
            binary: true,
            hunks: Vec::new(),
        },
    }
}
