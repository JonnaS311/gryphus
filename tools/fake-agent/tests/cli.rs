//! El binario real (sin PTY, por tuberías) interpreta el lenguaje de comandos.

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn runs_a_full_script() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fake-agent"))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("lanzar fake-agent");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hola\r\n::flood 3\n::write sub/a.txt contenido\n::nada\n::exit 3\nignorado\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert_eq!(out.status.code(), Some(3));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(
        lines,
        [
            "fake-agent ready",
            "echo: hola",
            "line 0",
            "line 1",
            "line 2",
            "wrote sub/a.txt",
            "unknown command: ::nada",
        ]
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("sub").join("a.txt")).unwrap(),
        "contenido"
    );
}

#[test]
fn exits_cleanly_when_stdin_closes() {
    let out = Command::new(env!("CARGO_BIN_EXE_fake-agent"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "fake-agent ready"
    );
}
