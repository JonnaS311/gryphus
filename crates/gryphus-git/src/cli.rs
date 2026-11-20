//! Ejecución del binario `git` y clasificación de sus errores.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use gryphus_core::ports::{PortError, PortErrorKind, PortResult};

use crate::CliGit;

impl CliGit {
    fn command(&self, cwd: &Path, read_only: bool) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.current_dir(cwd)
            .args(["-c", "core.quotepath=false", "-c", "color.ui=false"])
            .env("LC_ALL", "C")
            .env("LANGUAGE", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        if read_only {
            cmd.env("GIT_OPTIONAL_LOCKS", "0");
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    fn output<I, S>(&self, cwd: &Path, args: I, read_only: bool) -> PortResult<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if !cwd.is_dir() {
            return Err(PortError::new(
                PortErrorKind::NotFound,
                format!("no existe el directorio {}", cwd.display()),
            ));
        }
        self.command(cwd, read_only)
            .args(args)
            .output()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => PortError::new(
                    PortErrorKind::Unavailable,
                    format!("no se encontró git ({:?})", self.program),
                ),
                _ => PortError::new(PortErrorKind::Io, e.to_string()),
            })
    }

    fn checked(&self, out: Output) -> PortResult<Vec<u8>> {
        if out.status.success() {
            Ok(out.stdout)
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            Err(PortError::new(classify_failure(&stderr), stderr))
        }
    }

    /// Lectura: falla si git falla.
    pub(crate) fn read_bytes<I, S>(&self, cwd: &Path, args: I) -> PortResult<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let out = self.output(cwd, args, true)?;
        self.checked(out)
    }

    pub(crate) fn read<I, S>(&self, cwd: &Path, args: I) -> PortResult<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.read_bytes(cwd, args)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Lectura donde un código de salida distinto de 0 significa "no hay" (`None`).
    /// Un repo inexistente sigue siendo error.
    pub(crate) fn try_read<I, S>(&self, cwd: &Path, args: I) -> PortResult<Option<String>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let out = self.output(cwd, args, true)?;
        if out.status.success() {
            return Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()));
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("not a git repository") {
            return Err(PortError::new(PortErrorKind::NotFound, stderr.trim()));
        }
        Ok(None)
    }

    /// Mutación (toma locks normalmente).
    pub(crate) fn write<I, S>(&self, cwd: &Path, args: I) -> PortResult<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let out = self.output(cwd, args, false)?;
        self.checked(out)
    }
}

/// Clasifica el stderr de git (con `LC_ALL=C`) en un [`PortErrorKind`].
pub fn classify_failure(stderr: &str) -> PortErrorKind {
    let s = stderr.to_ascii_lowercase();
    let has = |needle: &str| s.contains(needle);
    if has("is already checked out")
        || has("is already used by worktree")
        || has("contains modified or untracked files")
        || has("is locked")
        || has("index.lock")
    {
        PortErrorKind::Conflict
    } else if has("already exists") {
        PortErrorKind::AlreadyExists
    } else if has("is a main working tree") {
        PortErrorKind::InvalidInput
    } else if has("not a git repository")
        || has("is not a working tree")
        || has("invalid reference")
        || has("not a valid object name")
        || has("unknown revision")
        || has("cannot change to")
    {
        PortErrorKind::NotFound
    } else if has("permission denied") {
        PortErrorKind::PermissionDenied
    } else {
        PortErrorKind::Io
    }
}

/// Igualdad de rutas tolerante a separadores, mayúsculas de unidad y symlinks.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use PortErrorKind::*;

    #[test]
    fn classifies_real_git_messages() {
        let cases = [
            ("fatal: a branch named 'main' already exists", AlreadyExists),
            ("fatal: '/tmp/x' already exists", AlreadyExists),
            ("fatal: 'main' is already checked out at '/repo'", Conflict),
            (
                "fatal: 'main' is already used by worktree at '/repo'",
                Conflict,
            ),
            (
                "fatal: '/wt' contains modified or untracked files, use --force to delete it",
                Conflict,
            ),
            (
                "fatal: Unable to create '/r/.git/index.lock': File exists.",
                Conflict,
            ),
            ("fatal: '/repo' is a main working tree", InvalidInput),
            (
                "fatal: not a git repository (or any of the parent directories): .git",
                NotFound,
            ),
            ("fatal: '/x' is not a working tree", NotFound),
            ("fatal: invalid reference: no-existe", NotFound),
            ("fatal: Not a valid object name nada", NotFound),
            ("error: something unexpected", Io),
        ];
        for (stderr, kind) in cases {
            assert_eq!(classify_failure(stderr), kind, "{stderr}");
        }
    }
}
