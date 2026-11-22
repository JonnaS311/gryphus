//! Casos propios de `LocalFs`: atomicidad, symlinks, `.git` en el watcher.

use std::sync::Arc;
use std::time::Duration;

use gryphus_core::ports::{FsPort, PortErrorKind};
use gryphus_core::RelPath;
use gryphus_fs::LocalFs;
use gryphus_testkit::contracts::assert_kind;
use gryphus_testkit::{wait_until, CollectingFsSink};

fn rel(s: &str) -> RelPath {
    RelPath::parse(s).unwrap()
}

#[test]
fn atomic_write_leaves_no_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFs::new();
    for i in 0..5 {
        fs.write_file(dir.path(), &rel("a.txt"), format!("{i}").as_bytes())
            .unwrap();
    }
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.txt"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "4"
    );
}

#[test]
fn cannot_write_a_file_where_a_directory_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("d")).unwrap();
    assert_kind(
        LocalFs::new().write_file(dir.path(), &rel("d"), b"x"),
        PortErrorKind::InvalidInput,
    );
}

#[test]
fn nested_gitignore_applies_to_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/.gitignore"), "*.log\n").unwrap();
    std::fs::write(dir.path().join("sub/a.log"), "").unwrap();
    std::fs::write(dir.path().join("sub/a.txt"), "").unwrap();
    let entries = LocalFs::new().list_dir(dir.path(), &rel("sub")).unwrap();
    let ignored: Vec<_> = entries
        .iter()
        .map(|e| (e.name.as_str(), e.ignored))
        .collect();
    assert_eq!(
        ignored,
        [(".gitignore", false), ("a.log", true), ("a.txt", false)]
    );
}

#[test]
fn symlinks_pointing_outside_the_worktree_are_rejected() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secreto.txt"), "x").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("enlace");
    #[cfg(unix)]
    let created = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
    // En Windows un symlink requiere privilegios; un junction no, y es igual de peligroso.
    #[cfg(windows)]
    let created = std::os::windows::fs::symlink_dir(outside.path(), &link).is_ok()
        || std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .is_ok_and(|o| o.status.success());
    if !created {
        eprintln!("omitido: este sistema no permite crear symlinks sin privilegios");
        return;
    }
    let fs = LocalFs::new();
    assert_kind(
        fs.read_file(dir.path(), &rel("enlace/secreto.txt")),
        PortErrorKind::PermissionDenied,
    );
    assert_kind(
        fs.list_dir(dir.path(), &rel("enlace")),
        PortErrorKind::PermissionDenied,
    );
    assert_kind(
        fs.write_file(dir.path(), &rel("enlace/nuevo.txt"), b"x"),
        PortErrorKind::PermissionDenied,
    );
}

#[test]
fn watcher_collapses_git_internals_into_dot_git() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let sink = Arc::new(CollectingFsSink::default());
    let _sub = LocalFs::new().watch(dir.path(), sink.clone()).unwrap();
    std::fs::write(dir.path().join(".git/index"), "x").unwrap();
    assert!(wait_until(Duration::from_secs(3), || sink.contains(".git")));
    assert!(
        sink.paths().iter().all(|p| p.as_str() == ".git"),
        "{:?}",
        sink.paths()
    );
}

/// Lo ignorado por `.gitignore` llega aparte (no altera el estado git), incluidos
/// los patrones anidados y las excepciones `!`.
#[test]
fn watcher_reports_gitignored_paths_separately() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join(".gitignore"), "target/\n*.log\n!keep.log\n").unwrap();
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/.gitignore"), "local.txt\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));

    let sink = Arc::new(CollectingFsSink::default());
    let _sub = LocalFs::new().watch(root, sink.clone()).unwrap();
    std::fs::write(root.join("target/debug/app.exe"), "x").unwrap();
    std::fs::write(root.join("a.log"), "x").unwrap();
    std::fs::write(root.join("keep.log"), "x").unwrap();
    std::fs::write(root.join("sub/local.txt"), "x").unwrap();
    std::fs::write(root.join("src/main.rs"), "x").unwrap();
    assert!(wait_until(Duration::from_secs(3), || {
        sink.contains("src/main.rs") && sink.contains("sub/local.txt") && sink.contains("keep.log")
    }));
    std::thread::sleep(Duration::from_millis(300));

    let ignored: Vec<_> = sink
        .ignored()
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    for path in ["target/debug/app.exe", "a.log", "sub/local.txt"] {
        assert!(
            ignored.iter().any(|p| p == path),
            "{path} ignorado: {ignored:?}"
        );
    }
    for path in ["src/main.rs", "keep.log"] {
        assert!(
            !ignored.iter().any(|p| p == path),
            "{path} no ignorado: {ignored:?}"
        );
    }
}

#[test]
fn watching_a_missing_directory_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(CollectingFsSink::default());
    assert_kind(
        LocalFs::new().watch(&dir.path().join("nada"), sink),
        PortErrorKind::NotFound,
    );
}

#[test]
fn copy_between_worktrees_skips_git_and_never_overwrites() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let fs = LocalFs::new();
    fs.write_file(a.path(), &rel("dir/x.txt"), b"x").unwrap();
    std::fs::create_dir(a.path().join("dir").join(".git")).unwrap();

    fs.copy(a.path(), &rel("dir"), b.path(), &rel("dir"))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(b.path().join("dir").join("x.txt")).unwrap(),
        "x"
    );
    assert!(
        !b.path().join("dir").join(".git").exists(),
        ".git no se copia"
    );
    // Mismo nombre relativo en otro worktree no es "dentro de sí mismo"; ya existe → error.
    assert_kind(
        fs.copy(a.path(), &rel("dir"), b.path(), &rel("dir")),
        PortErrorKind::AlreadyExists,
    );
}

#[test]
fn rename_changing_only_case_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFs::new();
    fs.write_file(dir.path(), &rel("a.txt"), b"a").unwrap();
    fs.rename(dir.path(), &rel("a.txt"), &rel("A.txt")).unwrap();
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["A.txt"]);
}

#[test]
fn the_root_is_not_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFs::new();
    let root = RelPath::default();
    assert_kind(fs.read_file(dir.path(), &root), PortErrorKind::InvalidInput);
    assert_kind(
        fs.write_file(dir.path(), &root, b"x"),
        PortErrorKind::InvalidInput,
    );
    fs.write_file(dir.path(), &rel("a.txt"), b"a").unwrap();
    assert_kind(
        fs.copy(dir.path(), &root, dir.path(), &rel("b")),
        PortErrorKind::InvalidInput,
    );
    assert_kind(
        fs.copy(dir.path(), &rel("a.txt"), dir.path(), &root),
        PortErrorKind::InvalidInput,
    );
}

#[test]
fn utf8_text_with_nul_bytes_is_binary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), b"hola\0mundo").unwrap();
    assert_eq!(
        LocalFs::new().read_file(dir.path(), &rel("a.bin")).unwrap(),
        gryphus_core::ports::FileContent::Binary { size: 10 }
    );
}

#[test]
fn target_parent_must_be_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFs::new();
    fs.write_file(dir.path(), &rel("a.txt"), b"a").unwrap();
    fs.write_file(dir.path(), &rel("b.txt"), b"b").unwrap();
    assert_kind(
        fs.rename(dir.path(), &rel("a.txt"), &rel("b.txt/a.txt")),
        PortErrorKind::InvalidInput,
    );
    assert_kind(
        fs.copy(dir.path(), &rel("a.txt"), dir.path(), &rel("b.txt/a.txt")),
        PortErrorKind::InvalidInput,
    );
}

#[test]
fn copying_a_directory_into_itself_within_one_root_fails() {
    let dir = tempfile::tempdir().unwrap();
    let fs = LocalFs::new();
    fs.write_file(dir.path(), &rel("d/x.txt"), b"x").unwrap();
    assert_kind(
        fs.copy(dir.path(), &rel("d"), dir.path(), &rel("d/copia")),
        PortErrorKind::InvalidInput,
    );
    assert!(!dir.path().join("d/copia").exists());
}

#[test]
fn list_dir_marks_symlinks_and_rejects_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    let fs = LocalFs::new();
    assert_kind(
        fs.list_dir(dir.path(), &rel("a.txt")),
        PortErrorKind::InvalidInput,
    );
    if !make_dir_link(&dir.path().join("real"), &dir.path().join("enlace")) {
        eprintln!("omitido: este sistema no permite crear symlinks sin privilegios");
        return;
    }
    let entries = fs.list_dir(dir.path(), &RelPath::default()).unwrap();
    let link = entries.iter().find(|e| e.name == "enlace").unwrap();
    assert_eq!(link.kind, gryphus_core::ports::EntryKind::Symlink);
}

/// Al editar `.gitignore` el watcher recarga sus reglas.
#[test]
fn watcher_reloads_rules_when_gitignore_changes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sink = Arc::new(CollectingFsSink::default());
    let _sub = LocalFs::new().watch(root, sink.clone()).unwrap();
    std::fs::write(root.join("a.log"), "x").unwrap();
    assert!(wait_until(Duration::from_secs(3), || sink.contains("a.log")));
    assert!(!sink.ignored().iter().any(|p| p.as_str() == "a.log"));

    std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
    assert!(wait_until(Duration::from_secs(3), || sink.contains(".gitignore")));
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(root.join("b.log"), "x").unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || sink
            .ignored()
            .iter()
            .any(|p| p.as_str() == "b.log")),
        "b.log debía llegar como ignorado: {:?}",
        sink.ignored()
    );
}

/// Lo que se crea dentro de un directorio nuevo se reporta aunque el SO no lo avise.
#[test]
fn watcher_reports_contents_of_new_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let sink = Arc::new(CollectingFsSink::default());
    let _sub = LocalFs::new().watch(root, sink.clone()).unwrap();
    std::fs::create_dir_all(root.join("nuevo/sub")).unwrap();
    std::fs::write(root.join("nuevo/sub/x.txt"), "x").unwrap();
    assert!(
        wait_until(Duration::from_secs(3), || sink.contains("nuevo/sub/x.txt")),
        "{:?}",
        sink.paths()
    );
}

/// Symlink de directorio; en Windows, si no hay privilegios, un junction.
fn make_dir_link(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_dir(target, link).is_ok()
        || std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success());
}

/// Copiar desde otra raíz (archivos soltados o pegados desde fuera del worktree).
#[test]
fn copies_from_outside_the_worktree_but_never_into_itself() {
    let outside = tempfile::tempdir().unwrap();
    let wt = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("docs/sub")).unwrap();
    std::fs::write(outside.path().join("docs/sub/x.txt"), "x").unwrap();
    let fs = LocalFs::new();
    fs.copy(outside.path(), &rel("docs"), wt.path(), &rel("docs"))
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(wt.path().join("docs/sub/x.txt")).unwrap(),
        "x"
    );

    // Una carpeta del worktree tomada "desde fuera" (su ruta absoluta) no puede
    // copiarse dentro de sí misma: antes solo se detectaba con la misma raíz.
    let parent = wt.path().join("docs");
    assert_kind(
        fs.copy(&parent, &rel("sub"), wt.path(), &rel("docs/sub/sub")),
        PortErrorKind::InvalidInput,
    );
    assert!(!wt.path().join("docs/sub/sub").exists());
}
