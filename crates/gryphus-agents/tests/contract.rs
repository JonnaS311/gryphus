//! `PtyRuntime` cumple el contrato de `AgentRuntimePort` con el binario real
//! `fake-agent` dentro de una PTY (ConPTY en Windows).

use std::path::PathBuf;

use gryphus_agents::PtyRuntime;
use gryphus_core::ports::{AgentRuntimePort, SpawnSpec, TermSize};
use gryphus_core::{AgentId, ProviderConfig};
use gryphus_testkit::{fake_agent_bin, AgentHarness};
use tempfile::TempDir;

struct Harness {
    dir: TempDir,
    runtime: PtyRuntime,
}

impl Harness {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            runtime: PtyRuntime::new(),
        }
    }

    fn spec_for(&self, agent: AgentId, program: PathBuf) -> SpawnSpec {
        SpawnSpec::new(
            agent,
            &ProviderConfig::with_command(program.to_string_lossy()),
            self.dir.path().to_path_buf(),
            TermSize::default(),
        )
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.runtime.kill_all();
    }
}

impl AgentHarness for Harness {
    fn port(&self) -> &dyn AgentRuntimePort {
        &self.runtime
    }
    fn spec(&self, agent: AgentId) -> SpawnSpec {
        self.spec_for(agent, fake_agent_bin())
    }
    fn missing_program_spec(&self, agent: AgentId) -> SpawnSpec {
        self.spec_for(agent, PathBuf::from("gryphus-no-such-program"))
    }
}

gryphus_testkit::agents_contract!(Harness::new);
