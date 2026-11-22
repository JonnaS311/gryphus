//! Conversaciones guardadas por los CLIs de agente, para retomarlas
//! (`AgentRuntimePort::find_session`; docs/architecture/agents.md#reanudar-conversaciones).
//!
//! - **Claude Code**: `<CLAUDE_CONFIG_DIR|~/.claude>/projects/<cwd codificada>/<id>.jsonl`.
//!   Gryphus elige el id (`--session-id`), solo hay que comprobar que existe: si no se
//!   escribió nada, `--resume` fallaría.
//! - **Codex**: `<CODEX_HOME|~/.codex>/sessions/AAAA/MM/DD/rollout-<fecha>-<id>.jsonl`.
//!   Su id no se puede elegir: se busca la conversación del worktree creada desde
//!   que se lanzó el agente. La primera línea (`session_meta`) trae id, cwd y fecha.
//! - **OpenCode**: `--session <id>` retoma o crea; no hace falta buscar nada.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use gryphus_core::ports::SessionQuery;
use gryphus_core::{is_valid_session_id, ProviderId};

/// Margen para relojes y fechas de archivo poco precisas.
const CLOCK_SLACK_MS: u64 = 2_000;
/// La primera línea de Codex puede traer instrucciones largas; más que esto no es suya.
const MAX_META_LINE: u64 = 1024 * 1024;

pub fn find_session(query: &SessionQuery) -> Option<String> {
    if query
        .known
        .as_deref()
        .is_some_and(|id| !is_valid_session_id(id))
    {
        return None;
    }
    match query.provider {
        ProviderId::Claude => claude_session(&claude_dir(&query.env)?, query),
        ProviderId::Codex => codex_session(&codex_home(&query.env)?, query),
        ProviderId::OpenCode => query.known.clone(),
    }
}

/// Variable del entorno del proveedor o, si no está, del proceso.
fn env_path(env: &[(String, String)], key: &str) -> Option<PathBuf> {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| PathBuf::from(v))
        .or_else(|| std::env::var_os(key).map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
}

fn home(env: &[(String, String)]) -> Option<PathBuf> {
    env_path(env, if cfg!(windows) { "USERPROFILE" } else { "HOME" })
}

fn claude_dir(env: &[(String, String)]) -> Option<PathBuf> {
    env_path(env, "CLAUDE_CONFIG_DIR").or_else(|| Some(home(env)?.join(".claude")))
}

fn codex_home(env: &[(String, String)]) -> Option<PathBuf> {
    env_path(env, "CODEX_HOME").or_else(|| Some(home(env)?.join(".codex")))
}

/// Se busca en todas las carpetas de `projects`: la codificación de la ruta del
/// worktree en el nombre de carpeta es interna de Claude (y cambia con rutas largas).
pub(crate) fn claude_session(dir: &Path, query: &SessionQuery) -> Option<String> {
    let id = query.known.as_deref()?;
    let file = format!("{id}.jsonl");
    fs::read_dir(dir.join("projects"))
        .ok()?
        .flatten()
        .any(|e| e.path().join(&file).is_file())
        .then(|| id.to_owned())
}

pub(crate) fn codex_session(home: &Path, query: &SessionQuery) -> Option<String> {
    let files = rollouts(&home.join("sessions"));
    if let Some(id) = query.known.as_deref() {
        let suffix = format!("-{id}.jsonl");
        return files
            .iter()
            .any(|f| {
                f.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
            })
            .then(|| id.to_owned());
    }
    let since = query.since_ms.saturating_sub(CLOCK_SLACK_MS);
    files
        .iter()
        // La fecha de modificación nunca es anterior a la de creación: descarta sin abrir.
        .filter(|f| modified_ms(f).is_some_and(|m| m >= since))
        .filter_map(|f| {
            let meta = codex_meta(f)?;
            let created = meta.created_ms.or_else(|| created_ms(f))?;
            (created >= since
                && same_dir(Path::new(&meta.cwd), &query.cwd)
                && is_valid_session_id(&meta.id)
                && !query.exclude.contains(&meta.id))
            .then_some((created, meta.id))
        })
        .min()
        .map(|(_, id)| id)
}

/// `rollout-*.jsonl` bajo `sessions/AAAA/MM/DD`.
fn rollouts(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < 4 {
                walk(&path, depth + 1, out);
            } else if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                    out.push(path);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 0, &mut out);
    out
}

struct CodexMeta {
    id: String,
    cwd: String,
    created_ms: Option<u64>,
}

fn codex_meta(file: &Path) -> Option<CodexMeta> {
    let mut line = String::new();
    BufReader::new(File::open(file).ok()?.take(MAX_META_LINE))
        .read_line(&mut line)
        .ok()?;
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let payload = if value["type"] == "session_meta" {
        &value["payload"]
    } else {
        &value
    };
    Some(CodexMeta {
        id: payload["id"].as_str()?.to_owned(),
        cwd: payload["cwd"].as_str()?.to_owned(),
        created_ms: payload["timestamp"]
            .as_str()
            .or_else(|| value["timestamp"].as_str())
            .and_then(parse_utc_ms),
    })
}

fn modified_ms(file: &Path) -> Option<u64> {
    let t = fs::metadata(file).ok()?.modified().ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

fn created_ms(file: &Path) -> Option<u64> {
    let meta = fs::metadata(file).ok()?;
    let t = meta.created().or_else(|_| meta.modified()).ok()?;
    Some(t.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// `AAAA-MM-DDTHH:MM:SS[.fff…]Z` → ms Unix. Otros formatos: `None`.
pub(crate) fn parse_utc_ms(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, ""));
    let mut t = hms.splitn(3, ':').map(|p| p.parse::<u64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let millis = frac
        .chars()
        .chain("000".chars())
        .take(3)
        .collect::<String>()
        .parse::<u64>()
        .ok()?;
    // Días desde 1970-01-01 (algoritmo civil de Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = u64::try_from(era * 146_097 + doe - 719_468).ok()?;
    Some(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + millis)
}

/// Misma carpeta aunque difieran en separadores, mayúsculas (Windows), prefijo
/// `\\?\` o nombres cortos 8.3.
fn same_dir(a: &Path, b: &Path) -> bool {
    fn norm(p: &Path) -> String {
        let p = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = p.to_string_lossy();
        let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
        let s = if cfg!(windows) {
            s.replace('/', "\\").to_lowercase()
        } else {
            s.to_owned()
        };
        s.trim_end_matches(['/', '\\']).to_owned()
    }
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(provider: ProviderId, cwd: &Path) -> SessionQuery {
        SessionQuery {
            provider,
            env: Vec::new(),
            cwd: cwd.to_path_buf(),
            known: None,
            since_ms: 0,
            exclude: Vec::new(),
        }
    }

    fn rollout(home: &Path, name: &str, id: &str, cwd: &Path, timestamp: &str) {
        let dir = home.join("sessions").join("2026").join("11").join("22");
        fs::create_dir_all(&dir).unwrap();
        let meta = serde_json::json!({
            "timestamp": timestamp,
            "type": "session_meta",
            "payload": { "id": id, "timestamp": timestamp, "cwd": cwd, "instructions": "…" },
        });
        fs::write(
            dir.join(format!("rollout-{name}-{id}.jsonl")),
            format!("{meta}\n{{\"type\":\"turn\"}}\n"),
        )
        .unwrap();
    }

    #[test]
    fn utc_timestamps_parse_to_unix_millis() {
        assert_eq!(parse_utc_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_utc_ms("2026-11-22T10:30:00.123Z"),
            Some(1_795_343_400_123)
        );
        assert_eq!(
            parse_utc_ms("2000-02-29T00:00:01.5Z"),
            Some(951_782_401_500)
        );
        assert_eq!(parse_utc_ms("2026-11-22T10:30:00+02:00"), None);
        assert_eq!(parse_utc_ms("basura"), None);
    }

    #[test]
    fn claude_session_exists_only_once_written() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("projects").join("C--repo");
        fs::create_dir_all(&dir).unwrap();
        let mut q = query(ProviderId::Claude, tmp.path());
        assert_eq!(claude_session(tmp.path(), &q), None, "sin id no hay nada");

        q.known = Some("1234".into());
        assert_eq!(claude_session(tmp.path(), &q), None);
        fs::write(dir.join("1234.jsonl"), "{}\n").unwrap();
        assert_eq!(claude_session(tmp.path(), &q).as_deref(), Some("1234"));
    }

    #[test]
    fn codex_session_is_the_oldest_unclaimed_one_of_the_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("codex");
        let wt = tmp.path().join("wt");
        let other = tmp.path().join("otro");
        fs::create_dir_all(&wt).unwrap();
        fs::create_dir_all(&other).unwrap();
        rollout(&home, "a", "antes", &wt, "2026-11-22T09:00:00.000Z");
        rollout(&home, "b", "ajena", &other, "2026-11-22T10:00:01.000Z");
        rollout(&home, "c", "mia", &wt, "2026-11-22T10:00:02.000Z");
        rollout(
            &home,
            "d",
            "de-otro-agente",
            &wt,
            "2026-11-22T10:00:01.500Z",
        );
        rollout(&home, "e", "despues", &wt, "2026-11-22T10:00:03.000Z");

        let mut q = query(ProviderId::Codex, &wt);
        q.since_ms = parse_utc_ms("2026-11-22T10:00:00.000Z").unwrap();
        q.exclude = vec!["de-otro-agente".into()];
        assert_eq!(codex_session(&home, &q).as_deref(), Some("mia"));

        q.known = Some("antes".into());
        assert_eq!(codex_session(&home, &q).as_deref(), Some("antes"));
        q.known = Some("borrada".into());
        assert_eq!(codex_session(&home, &q), None);
    }

    #[test]
    fn opencode_resumes_any_known_id_and_bad_ids_are_ignored() {
        let mut q = query(ProviderId::OpenCode, Path::new("wt"));
        assert_eq!(find_session(&q), None);
        q.known = Some("ses_abc".into());
        assert_eq!(find_session(&q).as_deref(), Some("ses_abc"));
        q.known = Some("../../etc".into());
        assert_eq!(find_session(&q), None);
    }

    #[test]
    fn storage_dirs_follow_the_provider_env() {
        let env = vec![
            ("CLAUDE_CONFIG_DIR".to_owned(), "/c".to_owned()),
            ("CODEX_HOME".to_owned(), "/x".to_owned()),
        ];
        assert_eq!(claude_dir(&env), Some(PathBuf::from("/c")));
        assert_eq!(codex_home(&env), Some(PathBuf::from("/x")));
    }
}
