//! `PATH` del shell de login (macOS).
//!
//! Una app abierta desde Finder, el Dock o Spotlight hereda de `launchd` un `PATH`
//! mínimo (`/usr/bin:/bin:/usr/sbin:/sbin`): sin Homebrew, ni los binarios
//! globales de npm, ni `~/.local/bin`. Así no se encontrarían `claude`, `codex`
//! ni `opencode` (y quizá tampoco el `git` de Homebrew). Al arrancar se le pide el
//! `PATH` al shell del usuario, igual que lo vería en su terminal.
//!
//! Se hace en `main`, antes de que existan otros hilos: `setenv` no es seguro
//! si otro hilo lee el entorno a la vez.

use std::ffi::OsString;
use std::path::PathBuf;

/// Separa el `PATH` del resto de lo que imprima el shell (avisos de `.zshrc`, etc.).
const MARKER: &str = "__GRYPHUS_PATH__";

/// Si el shell no responde en este tiempo (un `.zshrc` que espera entrada, una
/// red lenta), se sigue con el `PATH` heredado más las rutas habituales.
#[cfg(target_os = "macos")]
const SHELL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Completa el `PATH` del proceso con el del shell de login. No hace nada fuera
/// de macOS ni si la app se lanzó desde un terminal (ya trae su `PATH`).
pub fn import_login_path() {
    #[cfg(target_os = "macos")]
    {
        if std::env::var_os("TERM").is_some() {
            return;
        }
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let login = login_shell_path().unwrap_or_default();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let merged = merge_paths(&[login, inherited], &fallback_dirs(home.as_deref()));
        std::env::set_var("PATH", merged);
    }
}

/// Ejecuta `$SHELL -l -i -c …` (interactivo para que zsh lea también `.zshrc`,
/// donde suelen estar nvm, pnpm o bun) y extrae el `PATH` entre marcadores.
/// `printenv` en vez de `echo $PATH` para que fish lo dé separado por `:`.
#[cfg(target_os = "macos")]
fn login_shell_path() -> Option<OsString> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let script = format!("echo {MARKER}; /usr/bin/printenv PATH; echo {MARKER}");
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let deadline = Instant::now() + SHELL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    parse_marked(&reader.join().ok()?).map(OsString::from)
}

/// Texto entre la primera y la segunda línea `MARKER`, sin espacios alrededor.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_marked(output: &str) -> Option<String> {
    let mut lines = output.lines().skip_while(|l| l.trim() != MARKER).skip(1);
    let mut value = String::new();
    for line in lines.by_ref() {
        if line.trim() == MARKER {
            let value = value.trim();
            return (!value.is_empty()).then(|| value.to_owned());
        }
        value.push_str(line);
    }
    None
}

/// Rutas donde suelen instalarse los CLIs de agentes, por si el shell no responde.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn fallback_dirs(home: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = home {
        dirs.extend([
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".bun/bin"),
            home.join(".opencode/bin"),
            home.join(".cargo/bin"),
        ]);
    }
    dirs
}

/// Une varios `PATH` en orden, sin duplicados, y añade al final las rutas de
/// `extra` que existan y falten.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn merge_paths(paths: &[OsString], extra: &[PathBuf]) -> OsString {
    let mut out: Vec<PathBuf> = Vec::new();
    let candidates = paths
        .iter()
        .flat_map(std::env::split_paths)
        .chain(extra.iter().filter(|d| d.is_dir()).cloned());
    for dir in candidates {
        if !dir.as_os_str().is_empty() && !out.contains(&dir) {
            out.push(dir);
        }
    }
    std::env::join_paths(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_path_between_markers_ignoring_shell_noise() {
        let out = format!("Bienvenido\n{MARKER}\n/opt/homebrew/bin:/usr/bin\n{MARKER}\nadiós\n");
        assert_eq!(
            parse_marked(&out).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(parse_marked("sin marcadores"), None);
        assert_eq!(parse_marked(&format!("{MARKER}\n/usr/bin\n")), None);
        assert_eq!(parse_marked(&format!("{MARKER}\n\n{MARKER}\n")), None);
    }

    #[test]
    fn merges_in_order_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir(&a).unwrap();
        let first = std::env::join_paths([&a, &b]).unwrap();
        let second = std::env::join_paths([&b, &a]).unwrap();
        let missing = dir.path().join("no-existe");
        let merged = merge_paths(&[first, second], &[a.clone(), missing]);
        let parts: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(parts, [a, b]);
    }

    #[test]
    fn fallback_includes_user_bins_when_home_is_known() {
        let home = std::path::Path::new("/Users/alguien");
        let dirs = fallback_dirs(Some(home));
        assert!(dirs.contains(&home.join(".local/bin")));
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert_eq!(fallback_dirs(None).len(), 2);
    }
}
