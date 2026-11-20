//! Contrato de `FsPort`.
//!
//! Semántica acordada:
//! - `list_dir`: un nivel; directorios primero y luego archivos, por nombre sin
//!   distinguir mayúsculas; nunca muestra `.git`; marca lo ignorado por `.gitignore`.
//! - `read_file`: `Text` si es UTF-8 sin NUL; `Binary` si no; `TooLarge` si supera el límite.
//! - `write_file`: crea los directorios padre que falten.
//! - `watch`: reporta cambios externos en ≤ 3 s; soltar la suscripción los detiene.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gryphus_core::ports::{EntryKind, FileContent, FsPort, PortErrorKind};
use gryphus_core::RelPath;

use super::assert_kind;
use crate::sinks::CollectingFsSink;
use crate::wait::wait_until;

pub trait FsHarness {
    fn port(&self) -> &dyn FsPort;
    fn root(&self) -> PathBuf;
    /// Escritura externa al puerto (simula a un agente); crea padres.
    fn write_raw(&self, rel: &str, bytes: &[u8]);
    fn create_dir(&self, rel: &str);
    /// Límite de tamaño de lectura configurado en el puerto.
    fn size_limit(&self) -> u64;
}

fn rel(s: &str) -> RelPath {
    RelPath::parse(s).unwrap()
}

fn names<H: FsHarness>(h: &H, dir: &str) -> Vec<(String, EntryKind)> {
    h.port()
        .list_dir(&h.root(), &rel(dir))
        .unwrap()
        .into_iter()
        .map(|e| (e.name, e.kind))
        .collect()
}

pub fn lists_dirs_first_then_files_case_insensitively<H: FsHarness>(h: &H) {
    h.create_dir("src");
    h.create_dir("Docs");
    h.write_raw("b.txt", b"b");
    h.write_raw("A.md", b"a");
    let names: Vec<_> = names(h, "").into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Docs", "src", "A.md", "b.txt"]);
}

pub fn lists_nested_directories<H: FsHarness>(h: &H) {
    h.write_raw("src/main.rs", b"fn main() {}");
    h.write_raw("src/bin/tool.rs", b"");
    assert_eq!(
        names(h, "src"),
        [
            ("bin".to_owned(), EntryKind::Dir),
            ("main.rs".to_owned(), EntryKind::File)
        ]
    );
}

pub fn hides_the_git_directory<H: FsHarness>(h: &H) {
    h.create_dir(".git");
    h.write_raw("a.txt", b"a");
    let names: Vec<_> = names(h, "").into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["a.txt"]);
}

pub fn marks_gitignored_entries<H: FsHarness>(h: &H) {
    h.write_raw(".gitignore", b"target/\nsecret.txt\n");
    h.create_dir("target");
    h.write_raw("secret.txt", b"x");
    h.write_raw("ok.txt", b"x");
    let entries = h.port().list_dir(&h.root(), &RelPath::root()).unwrap();
    let ignored = |name: &str| {
        entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} no listado"))
            .ignored
    };
    assert!(ignored("target"));
    assert!(ignored("secret.txt"));
    assert!(!ignored("ok.txt"));
    assert!(!ignored(".gitignore"));
}

pub fn list_dir_errors<H: FsHarness>(h: &H) {
    h.write_raw("file.txt", b"x");
    assert_kind(
        h.port().list_dir(&h.root(), &rel("nada")),
        PortErrorKind::NotFound,
    );
    assert_kind(
        h.port().list_dir(&h.root(), &rel("file.txt")),
        PortErrorKind::InvalidInput,
    );
}

pub fn reads_text_binary_and_too_large<H: FsHarness>(h: &H) {
    h.write_raw("text.txt", "hola ñ\n".as_bytes());
    h.write_raw("bin.dat", &[0x89, b'P', 0, 1]);
    let limit = h.size_limit();
    h.write_raw("big.txt", &vec![b'a'; limit as usize + 1]);

    let read = |p: &str| h.port().read_file(&h.root(), &rel(p)).unwrap();
    assert_eq!(read("text.txt"), FileContent::Text("hola ñ\n".into()));
    assert_eq!(read("bin.dat"), FileContent::Binary { size: 4 });
    assert_eq!(
        read("big.txt"),
        FileContent::TooLarge {
            size: limit + 1,
            limit
        }
    );
}

pub fn read_file_errors<H: FsHarness>(h: &H) {
    h.create_dir("dir");
    assert_kind(
        h.port().read_file(&h.root(), &rel("nada.txt")),
        PortErrorKind::NotFound,
    );
    assert_kind(
        h.port().read_file(&h.root(), &rel("dir")),
        PortErrorKind::InvalidInput,
    );
}

pub fn write_file_round_trips_and_creates_parents<H: FsHarness>(h: &H) {
    let path = rel("nuevo/dir/f.txt");
    h.port().write_file(&h.root(), &path, b"contenido").unwrap();
    assert_eq!(
        h.port().read_file(&h.root(), &path).unwrap(),
        FileContent::Text("contenido".into())
    );
}

pub fn write_file_overwrites<H: FsHarness>(h: &H) {
    let path = rel("f.txt");
    h.port()
        .write_file(&h.root(), &path, b"uno dos tres")
        .unwrap();
    h.port().write_file(&h.root(), &path, b"dos").unwrap();
    assert_eq!(
        h.port().read_file(&h.root(), &path).unwrap(),
        FileContent::Text("dos".into())
    );
}

pub fn watch_reports_external_changes<H: FsHarness>(h: &H) {
    let sink = Arc::new(CollectingFsSink::default());
    let _sub = h.port().watch(&h.root(), sink.clone()).unwrap();
    h.write_raw("sub/w.txt", b"x");
    assert!(
        wait_until(Duration::from_secs(3), || sink.contains("sub/w.txt")),
        "no se reportó el cambio; recibido: {:?}",
        sink.paths()
    );
}

pub fn dropping_the_subscription_stops_events<H: FsHarness>(h: &H) {
    let sink = Arc::new(CollectingFsSink::default());
    let sub = h.port().watch(&h.root(), sink.clone()).unwrap();
    drop(sub);
    h.write_raw("w.txt", b"x");
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        sink.paths().is_empty(),
        "recibido tras cancelar: {:?}",
        sink.paths()
    );
}
