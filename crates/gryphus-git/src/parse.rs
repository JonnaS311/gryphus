//! Parsers puros de las salidas de git. Sin IO: se prueban con texto fijo.

use std::path::PathBuf;

use gryphus_core::ports::{Branch, DiffLine, FileDiff, FileState, FileStatus, Hunk, WorktreeInfo};
use gryphus_core::{BranchName, RelPath};

/// Campos separados por NUL (sin el vacío final).
pub(crate) fn nul_fields(bytes: &[u8]) -> Vec<String> {
    let mut fields: Vec<String> = bytes
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    if fields.last().is_some_and(String::is_empty) {
        fields.pop();
    }
    fields
}

/// `git worktree list --porcelain -z`: registros separados por un campo vacío.
pub(crate) fn worktrees(bytes: &[u8]) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    let mut current: Option<WorktreeInfo> = None;
    let mut bare = false;
    let finish = |wt: Option<WorktreeInfo>, bare: bool, out: &mut Vec<WorktreeInfo>| {
        if let Some(mut wt) = wt.filter(|_| !bare) {
            wt.is_main = out.is_empty();
            out.push(wt);
        }
    };
    for field in nul_fields(bytes)
        .into_iter()
        .chain(std::iter::once(String::new()))
    {
        if field.is_empty() {
            finish(current.take(), bare, &mut out);
            bare = false;
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field.as_str(), ""));
        match (key, current.as_mut()) {
            ("worktree", _) => {
                finish(current.take(), bare, &mut out);
                bare = false;
                current = Some(WorktreeInfo {
                    path: PathBuf::from(value),
                    branch: None,
                    head: None,
                    is_main: false,
                    locked: false,
                });
            }
            ("HEAD", Some(wt)) => wt.head = Some(value.to_owned()),
            ("branch", Some(wt)) => {
                wt.branch = value
                    .strip_prefix("refs/heads/")
                    .and_then(|b| BranchName::parse(b).ok());
            }
            ("locked", Some(wt)) => wt.locked = true,
            ("bare", Some(_)) => bare = true,
            _ => {}
        }
    }
    out
}

/// `for-each-ref --format=%(refname)%00%(upstream:short)%00%(upstream:track,nobracket)%00%(worktreepath)`
pub(crate) fn branches(bytes: &[u8]) -> Vec<Branch> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\0');
            let refname = f.next()?;
            let upstream = f.next().unwrap_or_default();
            let track = f.next().unwrap_or_default();
            let worktree = f.next().unwrap_or_default();
            let (name, is_remote) = match refname.strip_prefix("refs/heads/") {
                Some(local) => (local, false),
                None => {
                    let remote = refname.strip_prefix("refs/remotes/")?;
                    if remote.ends_with("/HEAD") {
                        return None;
                    }
                    (remote, true)
                }
            };
            let (ahead, behind) = parse_track(track);
            Some(Branch {
                name: BranchName::parse(name).ok()?,
                is_remote,
                upstream: (!upstream.is_empty()).then(|| upstream.to_owned()),
                ahead,
                behind,
                checked_out: !worktree.is_empty(),
            })
        })
        .collect()
}

/// `"ahead 3, behind 2"`, `"ahead 1"`, `"behind 4"`, `"gone"` o vacío.
fn parse_track(track: &str) -> (u32, u32) {
    let mut ahead = 0;
    let mut behind = 0;
    for part in track.split(',') {
        let mut words = part.split_whitespace();
        match (words.next(), words.next().and_then(|n| n.parse().ok())) {
            (Some("ahead"), Some(n)) => ahead = n,
            (Some("behind"), Some(n)) => behind = n,
            _ => {}
        }
    }
    (ahead, behind)
}

/// `git status --porcelain=v2 -z`. Una entrada por ruta: si hay cambios en el
/// directorio de trabajo manda ese estado (`staged: false`); si solo hay
/// cambios en el índice, `staged: true`.
pub(crate) fn status_v2(bytes: &[u8]) -> Vec<FileStatus> {
    let fields = nul_fields(bytes);
    let mut out = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let field = &fields[i];
        i += 1;
        let entry = match field.as_bytes().first() {
            Some(b'1') => field
                .splitn(9, ' ')
                .collect::<Vec<_>>()
                .get(1..9)
                .and_then(|p| change(p[0], p[7], None)),
            Some(b'2') => {
                let parts: Vec<_> = field.splitn(10, ' ').collect();
                let orig = fields.get(i).cloned();
                i += 1;
                parts
                    .get(1..10)
                    .and_then(|p| change(p[0], p[8], orig.as_deref()))
            }
            Some(b'u') => field
                .splitn(11, ' ')
                .nth(10)
                .and_then(|path| RelPath::parse(path).ok())
                .map(|path| FileStatus {
                    path,
                    state: FileState::Conflicted,
                    staged: false,
                }),
            Some(b'?') => field
                .get(2..)
                .and_then(|path| RelPath::parse(path).ok())
                .map(|path| FileStatus {
                    path,
                    state: FileState::Untracked,
                    staged: false,
                }),
            _ => None,
        };
        out.extend(entry);
    }
    out
}

fn change(xy: &str, path: &str, orig: Option<&str>) -> Option<FileStatus> {
    let mut chars = xy.chars();
    let (x, y) = (chars.next()?, chars.next()?);
    let (code, staged) = if y != '.' { (y, false) } else { (x, true) };
    let state = match code {
        'A' => FileState::Added,
        'D' => FileState::Deleted,
        'R' | 'C' => FileState::Renamed {
            from: RelPath::parse(orig?).ok()?,
        },
        'U' => FileState::Conflicted,
        _ => FileState::Modified,
    };
    Some(FileStatus {
        path: RelPath::parse(path).ok()?,
        state,
        staged,
    })
}

/// Diff unificado de `git diff --src-prefix=a/ --dst-prefix=b/ --no-renames`.
pub(crate) fn unified_diff(text: &str) -> Vec<FileDiff> {
    struct Pending {
        header_path: Option<String>,
        old: Option<String>,
        new: Option<String>,
        binary: bool,
        hunks: Vec<Hunk>,
    }
    fn finish(p: Pending, out: &mut Vec<FileDiff>) {
        let path = p
            .new
            .clone()
            .or(p.old.clone())
            .or(p.header_path.clone())
            .and_then(|s| RelPath::parse(&s).ok());
        if let Some(path) = path {
            out.push(FileDiff {
                path,
                old_path: None,
                binary: p.binary,
                hunks: p.hunks,
            });
        }
    }

    let mut out = Vec::new();
    let mut current: Option<Pending> = None;
    // Líneas restantes del hunk en curso (viejas, nuevas).
    let mut remaining = (0u32, 0u32);

    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(p) = current.as_mut().filter(|_| remaining != (0, 0)) {
            let hunk = p.hunks.last_mut().expect("hunk abierto");
            let (kind, rest) = line.split_at(line.len().min(1));
            match kind {
                " " | "" => {
                    hunk.lines.push(DiffLine::Context(rest.to_owned()));
                    remaining.0 = remaining.0.saturating_sub(1);
                    remaining.1 = remaining.1.saturating_sub(1);
                }
                "-" => {
                    hunk.lines.push(DiffLine::Removed(rest.to_owned()));
                    remaining.0 = remaining.0.saturating_sub(1);
                }
                "+" => {
                    hunk.lines.push(DiffLine::Added(rest.to_owned()));
                    remaining.1 = remaining.1.saturating_sub(1);
                }
                _ => {} // "\ No newline at end of file"
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(p) = current.take() {
                finish(p, &mut out);
            }
            current = Some(Pending {
                header_path: header_path(rest),
                old: None,
                new: None,
                binary: false,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(p) = current.as_mut() else { continue };
        if let Some(path) = line.strip_prefix("--- ") {
            p.old = strip_prefix_path(path, "a/");
        } else if let Some(path) = line.strip_prefix("+++ ") {
            p.new = strip_prefix_path(path, "b/");
        } else if line.starts_with("Binary files ") {
            p.binary = true;
        } else if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_hunk_header(header) {
                remaining = (hunk.old_lines, hunk.new_lines);
                p.hunks.push(hunk);
            }
        }
    }
    if let Some(p) = current {
        finish(p, &mut out);
    }
    out
}

/// `-12,3 +12,4 @@ contexto` → Hunk vacío con los rangos.
fn parse_hunk_header(header: &str) -> Option<Hunk> {
    let mut parts = header.split_whitespace();
    let (old_start, old_lines) = parse_range(parts.next()?.strip_prefix('-')?)?;
    let (new_start, new_lines) = parse_range(parts.next()?.strip_prefix('+')?)?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines: Vec::new(),
    })
}

fn parse_range(range: &str) -> Option<(u32, u32)> {
    match range.split_once(',') {
        Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
        None => Some((range.parse().ok()?, 1)),
    }
}

/// `a/ruta` (o `"a/ruta con \"comillas\""`) → `ruta`; `/dev/null` → `None`.
fn strip_prefix_path(raw: &str, prefix: &str) -> Option<String> {
    let raw = raw.trim_end_matches('\t');
    let path = unquote(raw);
    if path == "/dev/null" {
        return None;
    }
    path.strip_prefix(prefix).map(str::to_owned)
}

/// Respaldo para archivos sin `---`/`+++` (binarios, cambios de modo):
/// `a/x b/x` → `x`. Ambiguo solo si la ruta contiene `" b/"`.
fn header_path(rest: &str) -> Option<String> {
    let rest = unquote(rest);
    let after_a = rest.strip_prefix("a/")?;
    let split = after_a.find(" b/")?;
    Some(after_a[..split].to_owned())
}

/// Deshace las comillas estilo C que git usa en rutas con caracteres especiales.
fn unquote(s: &str) -> String {
    if !(s.starts_with('"') && s.contains('"')) {
        return s.to_owned();
    }
    let mut bytes = Vec::new();
    let mut chars = s.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quotes = !in_quotes,
            '\\' if in_quotes => match chars.next() {
                Some('n') => bytes.push(b'\n'),
                Some('t') => bytes.push(b'\t'),
                Some('"') => bytes.push(b'"'),
                Some('\\') => bytes.push(b'\\'),
                Some(d @ '0'..='7') => {
                    let mut value = d.to_digit(8).unwrap_or(0);
                    for _ in 0..2 {
                        if let Some(next) = chars.peek().and_then(|n| n.to_digit(8)) {
                            value = value * 8 + next;
                            chars.next();
                        }
                    }
                    bytes.push(value as u8);
                }
                Some(other) => {
                    let mut buf = [0; 4];
                    bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                }
                None => {}
            },
            other => {
                let mut buf = [0; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(s: &str) -> RelPath {
        RelPath::parse(s).unwrap()
    }

    #[test]
    fn parses_worktree_list() {
        let out = b"worktree /r\0HEAD aaa\0branch refs/heads/main\0\0\
                    worktree /wt/x\0HEAD bbb\0detached\0locked razon\0\0\
                    worktree /bare\0bare\0\0";
        let wts = worktrees(out);
        assert_eq!(wts.len(), 2, "el bare se omite");
        assert_eq!(wts[0].path, PathBuf::from("/r"));
        assert!(wts[0].is_main);
        assert_eq!(wts[0].branch, Some(BranchName::parse("main").unwrap()));
        assert_eq!(wts[0].head.as_deref(), Some("aaa"));
        assert!(!wts[1].is_main);
        assert_eq!(wts[1].branch, None);
        assert!(wts[1].locked);
    }

    #[test]
    fn parses_branches_with_tracking() {
        let out = "refs/heads/main\0origin/main\0ahead 2, behind 1\0/r\n\
                   refs/heads/dev\0\0\0\n\
                   refs/remotes/origin/HEAD\0\0\0\n\
                   refs/remotes/origin/main\0\0\0\n\
                   refs/heads/gone\0origin/gone\0gone\0\n";
        let b = branches(out.as_bytes());
        let names: Vec<_> = b.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, ["main", "dev", "origin/main", "gone"]);
        assert_eq!((b[0].ahead, b[0].behind), (2, 1));
        assert_eq!(b[0].upstream.as_deref(), Some("origin/main"));
        assert!(b[0].checked_out && !b[1].checked_out);
        assert!(b[2].is_remote);
        assert_eq!((b[3].ahead, b[3].behind), (0, 0));
    }

    #[test]
    fn parses_status_v2() {
        let out = b"1 .M N... 100644 100644 100644 aaa aaa README.md\0\
                    1 A. N... 000000 100644 100644 000 bbb src/nuevo.rs\0\
                    1 .D N... 100644 100644 000000 ccc ccc con espacio.txt\0\
                    2 R. N... 100644 100644 100644 ddd ddd R100 b.txt\0a.txt\0\
                    u UU N... 100644 100644 100644 100644 e1 e2 e3 conflicto.txt\0\
                    ? sin seguir/\xc3\xb1.txt\0";
        let st = status_v2(out);
        let summary: Vec<_> = st
            .iter()
            .map(|s| (s.path.as_str(), s.state.clone(), s.staged))
            .collect();
        assert_eq!(
            summary,
            [
                ("README.md", FileState::Modified, false),
                ("src/nuevo.rs", FileState::Added, true),
                ("con espacio.txt", FileState::Deleted, false),
                ("b.txt", FileState::Renamed { from: rel("a.txt") }, true),
                ("conflicto.txt", FileState::Conflicted, false),
                ("sin seguir/ñ.txt", FileState::Untracked, false),
            ]
        );
    }

    #[test]
    fn parses_unified_diff() {
        let text = "diff --git a/README.md b/README.md\n\
                    index 1..2 100644\n\
                    --- a/README.md\n\
                    +++ b/README.md\n\
                    @@ -1,2 +1,3 @@\n \
                    hola\n\
                    --- no es cabecera\n\
                    +mundo\r\n\
                    +++ tampoco\n\
                    \\ No newline at end of file\n\
                    diff --git a/borrado.txt b/borrado.txt\n\
                    deleted file mode 100644\n\
                    --- a/borrado.txt\n\
                    +++ /dev/null\n\
                    @@ -1 +0,0 @@\n\
                    -x\n\
                    diff --git a/img.png b/img.png\n\
                    Binary files a/img.png and b/img.png differ\n\
                    diff --git \"a/\\303\\261 \\\"q\\\".txt\" \"b/\\303\\261 \\\"q\\\".txt\"\n\
                    --- \"a/\\303\\261 \\\"q\\\".txt\"\n\
                    +++ \"b/\\303\\261 \\\"q\\\".txt\"\n\
                    @@ -0,0 +1 @@\n\
                    +nuevo\n";
        let files = unified_diff(text);
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["README.md", "borrado.txt", "img.png", "ñ \"q\".txt"]
        );

        let readme = &files[0].hunks[0];
        assert_eq!(
            (readme.old_start, readme.old_lines, readme.new_lines),
            (1, 2, 3)
        );
        assert_eq!(
            readme.lines,
            [
                DiffLine::Context("hola".into()),
                DiffLine::Removed("-- no es cabecera".into()),
                DiffLine::Added("mundo".into()),
                DiffLine::Added("++ tampoco".into()),
            ]
        );
        assert_eq!(files[1].hunks[0].lines, [DiffLine::Removed("x".into())]);
        assert!(files[2].binary && files[2].hunks.is_empty());
        assert_eq!(files[3].hunks[0].lines, [DiffLine::Added("nuevo".into())]);
    }

    #[test]
    fn hunk_header_without_counts_defaults_to_one() {
        let h = parse_hunk_header("-3 +4 @@ fn x()").unwrap();
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (3, 1, 4, 1)
        );
    }
}
