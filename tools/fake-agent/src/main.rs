//! Agente falso: imprime un banner y hace eco de cada línea.
//! Los comandos guionizados (`::exit N`, `::flood N`, `::write archivo`) llegan en el hito 2.
//! Ver docs/architecture/agents.md#agente-falso-para-pruebas.

use std::io::{self, BufRead, Write};

pub const BANNER: &str = "fake-agent ready";

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{BANNER}")?;
    stdout.flush()?;
    for line in stdin.lock().lines() {
        writeln!(stdout, "{}", echo(&line?))?;
        stdout.flush()?;
    }
    Ok(())
}

fn echo(line: &str) -> String {
    format!("echo: {line}")
}

#[cfg(test)]
mod tests {
    use super::echo;

    #[test]
    fn echoes_input_with_prefix() {
        assert_eq!(echo("hola"), "echo: hola");
    }
}
