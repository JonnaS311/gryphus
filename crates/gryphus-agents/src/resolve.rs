//! Resolución del ejecutable de un agente en el `PATH`.
//!
//! En Windows los CLIs instalados con npm (`claude`, `codex`, `opencode`) son
//! scripts `.cmd`; `CreateProcess` no los ejecuta directamente, así que se lanzan
//! con `cmd.exe /C`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Devuelve `(programa, argumentos previos)` o `None` si no existe.
pub fn resolve_program(
    program: &str,
    path_override: Option<&str>,
) -> Option<(PathBuf, Vec<OsString>)> {
    let found = find(program, path_override)?;
    if cfg!(windows) && is_batch(&found) {
        let cmd = std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("cmd.exe"));
        return Some((cmd, vec!["/C".into(), found.into_os_string()]));
    }
    Some((found, Vec::new()))
}

fn is_batch(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

/// Extensiones a probar para `base`, en orden.
///
/// En Windows el nombre sin extensión solo se acepta si ya trae una extensión
/// ejecutable: npm instala junto a `opencode.cmd` un script `opencode` (sh) sin
/// extensión que `CreateProcess` rechaza con "no es una aplicación Win32 válida".
fn extensions(base: &Path) -> Vec<String> {
    if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        // El FS de Windows no distingue mayúsculas: se devuelve la forma habitual.
        let exts: Vec<String> = pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_lowercase)
            .collect();
        let has_exec_ext = base.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            exts.iter()
                .any(|x| x.trim_start_matches('.').eq_ignore_ascii_case(e))
        });
        if has_exec_ext {
            vec![String::new()]
        } else {
            exts
        }
    } else {
        vec![String::new()]
    }
}

fn candidate(base: &Path) -> Option<PathBuf> {
    extensions(base).into_iter().find_map(|ext| {
        let mut p = base.as_os_str().to_owned();
        p.push(&ext);
        let p = PathBuf::from(p);
        p.is_file().then_some(p)
    })
}

fn find(program: &str, path_override: Option<&str>) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    let as_path = Path::new(program);
    if as_path.components().count() > 1 || as_path.is_absolute() {
        return candidate(as_path);
    }
    let path_var: OsString = match path_override {
        Some(p) => p.into(),
        None => std::env::var_os("PATH")?,
    };
    std::env::split_paths(&path_var).find_map(|dir| candidate(&dir.join(program)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_programs_in_the_given_path() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "herramienta.exe"
        } else {
            "herramienta"
        };
        std::fs::write(dir.path().join(name), "").unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let (found, args) = resolve_program("herramienta", Some(&path)).unwrap();
        assert_eq!(found, dir.path().join(name));
        assert!(args.is_empty());
        assert!(resolve_program("no-existe", Some(&path)).is_none());
        assert!(resolve_program("", Some(&path)).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn batch_shims_run_through_cmd() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("claude.cmd"), "@echo off").unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let (program, args) = resolve_program("claude", Some(&path)).unwrap();
        assert!(program
            .to_string_lossy()
            .to_lowercase()
            .ends_with("cmd.exe"));
        assert_eq!(args[0], "/C");
        assert_eq!(PathBuf::from(&args[1]), dir.path().join("claude.cmd"));
    }

    /// npm deja un script sh sin extensión junto al `.cmd`; no es ejecutable en Windows.
    #[cfg(windows)]
    #[test]
    fn extensionless_npm_shims_are_skipped_in_favor_of_cmd() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("opencode"), "#!/bin/sh").unwrap();
        std::fs::write(dir.path().join("opencode.cmd"), "@echo off").unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let (_, args) = resolve_program("opencode", Some(&path)).unwrap();
        assert_eq!(PathBuf::from(&args[1]), dir.path().join("opencode.cmd"));
        // Un nombre explícito con extensión se respeta tal cual.
        let (_, args) = resolve_program("opencode.cmd", Some(&path)).unwrap();
        assert_eq!(PathBuf::from(&args[1]), dir.path().join("opencode.cmd"));
        // Solo el script sin extensión: no hay nada ejecutable.
        std::fs::remove_file(dir.path().join("opencode.cmd")).unwrap();
        assert!(resolve_program("opencode", Some(&path)).is_none());
    }
}
