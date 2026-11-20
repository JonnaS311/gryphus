//! `CliGit` cumple el contrato de `GitPort` contra repos git reales.

use std::path::{Path, PathBuf};

use gryphus_core::ports::GitPort;
use gryphus_git::CliGit;
use gryphus_testkit::{GitHarness, TempRepo};

struct Harness {
    repo: TempRepo,
    git: CliGit,
}

impl Harness {
    fn new() -> Self {
        let repo = TempRepo::new();
        let git = repo
            .git_env()
            .into_iter()
            .fold(CliGit::new(), |git, (k, v)| git.env(k, v));
        Self { repo, git }
    }
}

impl GitHarness for Harness {
    fn port(&self) -> &dyn GitPort {
        &self.git
    }
    fn root(&self) -> PathBuf {
        self.repo.root().to_path_buf()
    }
    fn new_worktree_path(&self, name: &str) -> PathBuf {
        self.repo.worktree_path(name)
    }
    fn write_file(&self, worktree: &Path, rel: &str, content: &str) {
        self.repo.write_file(worktree, rel, content);
    }
    fn delete_file(&self, worktree: &Path, rel: &str) {
        self.repo.delete_file(worktree, rel);
    }
    fn commit_all(&self, worktree: &Path, message: &str) {
        self.repo.commit_all(worktree, message);
    }
    fn create_branch(&self, name: &str) {
        self.repo.create_branch(name);
    }
}

gryphus_testkit::git_contract!(Harness::new);
