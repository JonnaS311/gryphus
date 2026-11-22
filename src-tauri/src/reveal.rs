//! Mostrar una ruta en el gestor de archivos del sistema.
//!
//! - Windows: `explorer /select,` (selecciona el elemento en su carpeta).
//! - macOS: `open -R` (Finder, con el elemento seleccionado).
//! - Linux/BSD: D-Bus `org.freedesktop.FileManager1.ShowItems` (Nautilus, Dolphin,
//!   Nemo, Caja, Thunar…); si no hay quien lo atienda, `xdg-open` de la carpeta.
//!
//! Con `select = false` se abre la carpeta misma (raíz del worktree).

use std::io;
use std::path::Path;
use std::process::Command;

pub fn reveal(path: &Path, select: bool) -> io::Result<()> {
    imp::reveal(path, select)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::process::CommandExt;

    pub fn reveal(path: &Path, select: bool) -> io::Result<()> {
        // git da las raíces de los worktrees con `/` y `explorer` no las entiende.
        let path = path.to_string_lossy().replace('/', "\\");
        let mut cmd = Command::new("explorer");
        if select {
            // `explorer` no sigue las reglas de comillas de `CommandLineToArgvW`:
            // el argumento debe ir tal cual `/select,"C:\ruta"`.
            cmd.raw_arg(format!("/select,\"{path}\""));
        } else {
            cmd.arg(path);
        }
        // `explorer` devuelve 1 aunque funcione: no se espera su código de salida.
        cmd.spawn().map(drop)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    pub fn reveal(path: &Path, select: bool) -> io::Result<()> {
        let mut cmd = Command::new("open");
        if select {
            cmd.arg("-R");
        }
        run(cmd.arg(path))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::*;

    pub fn reveal(path: &Path, select: bool) -> io::Result<()> {
        if select && show_items(path) {
            return Ok(());
        }
        let dir = if select && !path.is_dir() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        run(Command::new("xdg-open").arg(dir))
    }

    /// Interfaz estándar de freedesktop; `false` si no hay gestor que la implemente.
    fn show_items(path: &Path) -> bool {
        Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{}", super::file_uri(path)))
            .arg("string:")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[cfg(unix)]
fn run(cmd: &mut Command) -> io::Result<()> {
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{cmd:?} terminó con {status}")))
    }
}

/// `file://` con los bytes fuera de lo no reservado codificados (`%20`…).
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut uri = String::from("file://");
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_escapes_reserved_and_non_ascii() {
        assert_eq!(
            file_uri(Path::new("/home/a b/ñ,x.txt")),
            "file:///home/a%20b/%C3%B1%2Cx.txt"
        );
    }
}
