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

/// La sesión guarda desde varios hilos a la vez (comandos IPC, watcher, salida de
/// agentes): guardados concurrentes no deben fallar ni dejar un JSON ilegible.
#[test]
fn concurrent_saves_of_the_same_project_all_succeed() {
    let h = std::sync::Arc::new(Harness::new());
    let p = gryphus_core::Project::new("/r".into());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let h = h.clone();
            let p = p.clone();
            std::thread::spawn(move || {
                for _ in 0..25 {
                    h.store.save_project(&p).expect("guardado concurrente");
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(h.store.load_project(p.id()).unwrap(), Some(p));
}

/// Un error tipográfico en un proveedor no debe ignorarse en silencio (dejaría el
/// proveedor "no configurado" sin explicación).
#[test]
fn unknown_provider_fields_are_reported() {
    let h = Harness::new();
    std::fs::write(h.store.config_file(), "[agents.claude]\ncomand = \"x\"\n").unwrap();
    assert_kind(h.store.load_config(), PortErrorKind::InvalidInput);
}
