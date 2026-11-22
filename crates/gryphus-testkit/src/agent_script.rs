//! Lenguaje de `fake-agent`. Una línea normal se repite con eco; las que empiezan
//! por `::` son comandos:
//!
//! | Línea | Efecto |
//! |---|---|
//! | `hola` | imprime `echo: hola` |
//! | `::exit [N]` | termina con código `N` (0 por defecto) |
//! | `::flood N` | imprime `line 0` … `line N-1` |
//! | `::write RUTA CONTENIDO` | escribe el archivo (relativo al cwd) e imprime `wrote RUTA` |
//! | `::spin N` | redibuja en el sitio una línea `spin 00000` … `spin N-1` (como el indicador de un TUI) y termina con `spin done` |
//!
//! Lo comparten el binario `tools/fake-agent` y [`crate::FakeAgentRuntime`].

pub const BANNER: &str = "fake-agent ready";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Echo(String),
    Exit(i32),
    Flood(u32),
    Spin(u32),
    Write { path: String, content: String },
    Unknown(String),
}

pub fn parse(line: &str) -> Action {
    let line = line.trim_end_matches(['\r', '\n']);
    let Some(command) = line.strip_prefix("::") else {
        return Action::Echo(line.to_owned());
    };
    let unknown = || Action::Unknown(line.to_owned());
    let mut parts = command.splitn(3, ' ');
    match parts.next().unwrap_or_default() {
        "exit" => match parts.next() {
            None => Action::Exit(0),
            Some(code) => code
                .trim()
                .parse()
                .map(Action::Exit)
                .unwrap_or_else(|_| unknown()),
        },
        "flood" => parts
            .next()
            .and_then(|n| n.trim().parse().ok())
            .map(Action::Flood)
            .unwrap_or_else(unknown),
        "spin" => parts
            .next()
            .and_then(|n| n.trim().parse().ok())
            .map(Action::Spin)
            .unwrap_or_else(unknown),
        "write" => match parts.next() {
            Some(path) if !path.is_empty() => Action::Write {
                path: path.to_owned(),
                content: parts.next().unwrap_or_default().to_owned(),
            },
            _ => unknown(),
        },
        _ => unknown(),
    }
}

pub fn echo_line(text: &str) -> String {
    format!("echo: {text}")
}

pub fn flood_line(i: u32) -> String {
    format!("line {i}")
}

/// Cuadro `i` de `::spin`: se escribe tras `\r`, sobre el anterior.
pub fn spin_frame(i: u32) -> String {
    format!("spin {i:05}")
}

pub const SPIN_DONE: &str = "spin done ";

pub fn wrote_line(path: &str) -> String {
    format!("wrote {path}")
}

pub fn unknown_line(line: &str) -> String {
    format!("unknown command: {line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_command() {
        assert_eq!(parse("hola\r\n"), Action::Echo("hola".into()));
        assert_eq!(parse("::exit"), Action::Exit(0));
        assert_eq!(parse("::exit 3"), Action::Exit(3));
        assert_eq!(parse("::flood 10"), Action::Flood(10));
        assert_eq!(parse("::spin 5"), Action::Spin(5));
        assert_eq!(
            parse("::write sub/a.txt hola mundo"),
            Action::Write {
                path: "sub/a.txt".into(),
                content: "hola mundo".into()
            }
        );
    }

    #[test]
    fn malformed_commands_are_unknown() {
        for bad in [
            "::exit x",
            "::flood",
            "::flood -1",
            "::spin",
            "::write",
            "::nada",
        ] {
            assert_eq!(parse(bad), Action::Unknown(bad.into()), "{bad}");
        }
    }
}
