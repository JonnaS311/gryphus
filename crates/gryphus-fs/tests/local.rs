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
