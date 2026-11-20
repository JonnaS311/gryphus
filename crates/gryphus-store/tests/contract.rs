//! `JsonStore` cumple el contrato de `StorePort` en un directorio temporal.

use gryphus_core::ports::{PortErrorKind, StorePort};
use gryphus_core::ProjectId;
use gryphus_store::JsonStore;
use gryphus_testkit::contracts::assert_kind;
use gryphus_testkit::StoreHarness;
use tempfile::TempDir;

struct Harness {
    _dir: TempDir,
    store: JsonStore,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonStore::new(dir.path(), dir.path().join("config.toml"));
        Self { _dir: dir, store }
    }
}

impl StoreHarness for Harness {
    fn port(&self) -> &dyn StorePort {
        &self.store
    }
    fn write_raw_project(&self, id: &ProjectId, contents: &str) {
        let path = self.store.project_file(id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}

gryphus_testkit::store_contract!(Harness::new);

#[test]
fn invalid_config_file_is_reported_as_invalid_input() {
    let h = Harness::new();
    std::fs::write(h.store.config_file(), "[agents.gpt]\ncommand = \"x\"\n").unwrap();
    assert_kind(h.store.load_config(), PortErrorKind::InvalidInput);
}

#[test]
fn save_leaves_no_temp_files() {
    let h = Harness::new();
    let p = gryphus_core::Project::new("/r".into());
    h.store.save_project(&p).unwrap();
    h.store.save_project(&p).unwrap();
    let dir = h.store.project_file(p.id()).parent().unwrap().to_path_buf();
    let names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [format!("{}.json", p.id())]);
}
