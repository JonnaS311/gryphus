//! Los fakes cumplen los mismos contratos que cumplirán los adaptadores reales.

use std::path::{Path, PathBuf};

use gryphus_core::ports::{AgentRuntimePort, FsPort, GitPort, SpawnSpec, StorePort, TermSize};
use gryphus_core::{AgentId, ProjectId, ProviderConfig};
use gryphus_testkit::fakes::FAKE_AGENT_PROGRAM;
use gryphus_testkit::{
    AgentHarness, FakeAgentRuntime, FakeFs, FakeGit, FakeStore, FsHarness, GitHarness, StoreHarness,
};

mod git {
    use super::*;

    struct Harness(FakeGit);

    impl Harness {
        fn new() -> Self {
            let git = FakeGit::new("/fake/repo");
            let root = git.root();
            git.write_file(&root, "README.md", "hola\n");
            git.commit_all(&root, "init");
            Self(git)
        }
    }

    impl GitHarness for Harness {
        fn port(&self) -> &dyn GitPort {
            &self.0
        }
        fn root(&self) -> PathBuf {
            self.0.root()
        }
        fn new_worktree_path(&self, name: &str) -> PathBuf {
            PathBuf::from("/fake/repo.worktrees").join(name)
        }
        fn write_file(&self, worktree: &Path, rel: &str, content: &str) {
            self.0.write_file(worktree, rel, content);
        }
        fn delete_file(&self, worktree: &Path, rel: &str) {
            self.0.delete_file(worktree, rel);
        }
        fn commit_all(&self, worktree: &Path, message: &str) {
            self.0.commit_all(worktree, message);
        }
        fn create_branch(&self, name: &str) {
            self.0.create_branch(name);
        }
    }

    gryphus_testkit::git_contract!(Harness::new);
}

mod fs {
    use super::*;

    struct Harness(FakeFs);

    impl Harness {
        fn new() -> Self {
            Self(FakeFs::with_limit("/fake/wt", 64))
        }
    }

    impl FsHarness for Harness {
        fn port(&self) -> &dyn FsPort {
            &self.0
        }
        fn root(&self) -> PathBuf {
            self.0.root()
        }
        fn write_raw(&self, rel: &str, bytes: &[u8]) {
            self.0.write_raw(rel, bytes);
        }
        fn create_dir(&self, rel: &str) {
            self.0.create_dir(rel);
        }
        fn size_limit(&self) -> u64 {
            self.0.size_limit()
        }
    }

    gryphus_testkit::fs_contract!(Harness::new);
}

mod store {
    use super::*;

    struct Harness(FakeStore);

    impl Harness {
        fn new() -> Self {
            Self(FakeStore::default())
        }
    }

    impl StoreHarness for Harness {
        fn port(&self) -> &dyn StorePort {
            &self.0
        }
        fn write_raw_project(&self, id: &ProjectId, contents: &str) {
            self.0.write_raw_project(id, contents);
        }
    }

    gryphus_testkit::store_contract!(Harness::new);
}

mod agents {
    use super::*;

    struct Harness(FakeAgentRuntime);

    impl Harness {
        fn new() -> Self {
            Self(FakeAgentRuntime::new())
        }
    }

    fn spec(agent: AgentId, program: &str) -> SpawnSpec {
        SpawnSpec::new(
            agent,
            &ProviderConfig::with_command(program),
            PathBuf::from("/fake/wt"),
            TermSize::default(),
        )
    }

    impl AgentHarness for Harness {
        fn port(&self) -> &dyn AgentRuntimePort {
            &self.0
        }
        fn spec(&self, agent: AgentId) -> SpawnSpec {
            spec(agent, FAKE_AGENT_PROGRAM)
        }
        fn missing_program_spec(&self, agent: AgentId) -> SpawnSpec {
            spec(agent, "gryphus-no-such-program")
        }
    }

    gryphus_testkit::agents_contract!(Harness::new);
}
