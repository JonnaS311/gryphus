//! `LocalFs` cumple el contrato de `FsPort` sobre un directorio temporal real.

use std::path::PathBuf;

use gryphus_core::ports::FsPort;
use gryphus_fs::LocalFs;
use gryphus_testkit::FsHarness;
use tempfile::TempDir;

struct Harness {
    dir: TempDir,
    fs: LocalFs,
}

impl Harness {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            fs: LocalFs::with_limit(64),
        }
    }
}

impl FsHarness for Harness {
    fn port(&self) -> &dyn FsPort {
        &self.fs
    }
    fn root(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
    fn write_raw(&self, rel: &str, bytes: &[u8]) {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    fn create_dir(&self, rel: &str) {
        std::fs::create_dir_all(self.dir.path().join(rel)).unwrap();
    }
    fn size_limit(&self) -> u64 {
        self.fs.size_limit()
    }
}

gryphus_testkit::fs_contract!(Harness::new);
