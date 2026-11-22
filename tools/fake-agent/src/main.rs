//! Agente falso para pruebas de integración y e2e. Interpreta el lenguaje de
//! `gryphus_testkit::agent_script` (el mismo que usa `FakeAgentRuntime`).
//! Ver docs/architecture/agents.md#agente-falso-para-pruebas.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use gryphus_testkit::agent_script::{self, Action};

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    writeln!(out, "{}", agent_script::BANNER)?;
    out.flush()?;
    for line in stdin.lock().lines() {
        match agent_script::parse(&line?) {
            Action::Echo(text) => writeln!(out, "{}", agent_script::echo_line(&text))?,
            Action::Flood(n) => {
                for i in 0..n {
                    writeln!(out, "{}", agent_script::flood_line(i))?;
                }
            }
            Action::Spin(n) => {
                // Con pausas, para que ConPTY emita cada cuadro y no solo el último.
                for i in 0..n {
                    write!(out, "\r{}", agent_script::spin_frame(i))?;
                    out.flush()?;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                writeln!(out, "\r{}", agent_script::SPIN_DONE)?;
            }
            Action::Write { path, content } => {
                if let Some(parent) = Path::new(&path).parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&path, content)?;
                writeln!(out, "{}", agent_script::wrote_line(&path))?;
            }
            Action::Unknown(text) => writeln!(out, "{}", agent_script::unknown_line(&text))?,
            Action::Exit(code) => {
                out.flush()?;
                std::process::exit(code);
            }
        }
        out.flush()?;
    }
    Ok(())
}
