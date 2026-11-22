//! Resolución del ejecutable de un agente en el `PATH`.
//!
//! En Windows los CLIs instalados con npm (`claude`, `codex`, `opencode`) son
//! scripts `.cmd`; `CreateProcess` no los ejecuta directamente, así que se lanzan
//! con `cmd.exe /C`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Devuelve `(programa, argumentos previos)` o `None` si no existe.
///
/// Un `.cmd`/`.bat` se lanza como `cmd.exe /D /V:OFF /C @ "<script>" args…`. El `@`
/// hace que la línea no empiece por comillas: si empieza (y hay más de dos
/// comillas o paréntesis, p. ej. `Program Files (x86)` o un argumento con
/// espacios), `cmd /C` quita la primera y la última comilla y la orden se rompe.
pub fn resolve_program(
    program: &str,
    path_override: Option<&str>,
) -> Option<(PathBuf, Vec<OsString>)> {
    let found = find(program, path_override)?;
    if cfg!(windows) && is_batch(&found) {
        let cmd = std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("cmd.exe"));
        return Some((
            cmd,
            vec![
                "/D".into(),
                "/V:OFF".into(),
                "/C".into(),
                "@".into(),
                found.into_os_string(),
            ],
        ));
    }
    Some((found, Vec::new()))
}

/// Si `arg` (o la ruta del script) no puede pasar intacto por `cmd.exe`, explica
/// por qué. El PTY solo entrecomilla argumentos con espacios o comillas, y cmd
/// interpreta `& | < > ^` fuera de comillas y expande `%VAR%` incluso dentro:
/// un argumento así ejecutaría otra orden o llegaría alterado al agente.
pub fn batch_arg_problem(arg: &str) -> Option<&'static str> {
    let quoted = arg.is_empty() || arg.contains([' ', '\t']);
    if arg.contains(['"', '\r', '\n']) {
        Some("contiene comillas o saltos de línea")
    } else if arg.contains('%') {
        Some("contiene '%' (cmd.exe expandiría variables)")
    } else if !quoted && arg.contains(['&', '|', '<', '>', '^']) {
        Some("contiene caracteres especiales de cmd.exe (& | < > ^)")
    } else {
        None
    }
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
        is_executable(&p).then_some(p)
    })
}

/// En Unix (macOS, Linux) un archivo sin permiso de ejecución no sirve: se sigue
/// buscando en el resto del `PATH`, como hace el shell.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
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
        make_executable(&dir.path().join(name));
        let path = dir.path().to_string_lossy().into_owned();
        let (found, args) = resolve_program("herramienta", Some(&path)).unwrap();
        assert_eq!(found, dir.path().join(name));
        assert!(args.is_empty());
        assert!(resolve_program("no-existe", Some(&path)).is_none());
        assert!(resolve_program("", Some(&path)).is_none());
    }

    #[cfg(unix)]
    fn make_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(not(unix))]
    fn make_executable(_: &Path) {}

    /// Un archivo sin bit de ejecución antes en el `PATH` no tapa al ejecutable.
    #[cfg(unix)]
    #[test]
    fn non_executable_files_are_skipped() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        std::fs::write(first.path().join("claude"), "").unwrap();
        std::fs::write(second.path().join("claude"), "#!/bin/sh").unwrap();
        make_executable(&second.path().join("claude"));
        let path = std::env::join_paths([first.path(), second.path()]).unwrap();
        let (found, _) = resolve_program("claude", path.to_str()).unwrap();
        assert_eq!(found, second.path().join("claude"));
        assert!(resolve_program("claude", first.path().to_str()).is_none());
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
        assert_eq!(args[..4], ["/D", "/V:OFF", "/C", "@"]);
        assert_eq!(PathBuf::from(&args[4]), dir.path().join("claude.cmd"));
    }

    /// Script en una ruta con espacios y paréntesis y argumentos con espacios y
    /// `&`: antes `cmd /C` quitaba comillas y fallaba con "no se reconoce…".
    #[cfg(windows)]
    #[test]
    fn batch_shims_survive_spaces_parentheses_and_quoted_args() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("Program Files (x86)");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(bin.join("eco.cmd"), "@echo off\r\necho ARGS=[%*]\r\n").unwrap();
        let path = bin.to_string_lossy().into_owned();
        let (program, prefix) = resolve_program("eco", Some(&path)).unwrap();
        let out = std::process::Command::new(program)
            .args(prefix)
            .args(["a b", "x&y z", "plano"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.contains(r#"ARGS=["a b" "x&y z" plano]"#),
            "salida: {text} {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn unsafe_batch_args_are_detected() {
        for ok in [
            "--verbose",
            "a b",
            "x&y z",
            "",
            "C:\\ruta\\sin espacios",
            "(x)",
        ] {
            assert_eq!(batch_arg_problem(ok), None, "{ok:?}");
        }
        for bad in [
            "a&b",
            "x|y",
            "a>b",
            "^x",
            "%PATH%",
            "con \"comillas\"",
            "a\nb",
        ] {
            assert!(batch_arg_problem(bad).is_some(), "{bad:?}");
        }
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
        assert_eq!(PathBuf::from(&args[4]), dir.path().join("opencode.cmd"));
        // Un nombre explícito con extensión se respeta tal cual.
        let (_, args) = resolve_program("opencode.cmd", Some(&path)).unwrap();
        assert_eq!(PathBuf::from(&args[4]), dir.path().join("opencode.cmd"));
        // Solo el script sin extensión: no hay nada ejecutable.
        std::fs::remove_file(dir.path().join("opencode.cmd")).unwrap();
        assert!(resolve_program("opencode", Some(&path)).is_none());
    }
}
