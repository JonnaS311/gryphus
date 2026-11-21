//! Iconos del explorador (Material Icon Theme, ver ui/icons/ y ui/js/scripts/icons.mjs).
//!
//! Lógica pura: qué icono le toca a un nombre de archivo o carpeta. Las tablas las
//! carga `bridge::icons`; aquí solo se recibe una función de búsqueda para poder
//! probarlo con `cargo test` nativo.

/// Tablas de `ui/icons/manifest.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Table {
    FileNames,
    FileExtensions,
    FolderNames,
    FolderNamesExpanded,
}

impl Table {
    pub fn key(self) -> &'static str {
        match self {
            Table::FileNames => "fileNames",
            Table::FileExtensions => "fileExtensions",
            Table::FolderNames => "folderNames",
            Table::FolderNamesExpanded => "folderNamesExpanded",
        }
    }
}

pub const DEFAULT_FILE: &str = "file";
pub const DEFAULT_FOLDER: &str = "folder";
pub const DEFAULT_FOLDER_OPEN: &str = "folder-open";

/// Ruta servida del SVG de un icono.
pub fn url(icon: &str) -> String {
    format!("icons/{icon}.svg")
}

/// Como VS Code: nombre exacto y luego la extensión más larga primero
/// (`a.spec.ts` → `spec.ts`, `ts`). Sin distinguir mayúsculas.
pub fn file_icon(name: &str, get: impl Fn(Table, &str) -> Option<String>) -> String {
    let lower = name.to_lowercase();
    if let Some(icon) = get(Table::FileNames, &lower) {
        return icon;
    }
    lower
        .match_indices('.')
        // Un punto inicial (`.env`) no separa extensión, salvo que sea el único nombre.
        .filter(|(i, _)| *i > 0 || lower.matches('.').count() == 1)
        .find_map(|(i, _)| get(Table::FileExtensions, &lower[i + 1..]))
        .unwrap_or_else(|| DEFAULT_FILE.to_owned())
}

pub fn folder_icon(name: &str, open: bool, get: impl Fn(Table, &str) -> Option<String>) -> String {
    let lower = name.to_lowercase();
    let (table, default) = if open {
        (Table::FolderNamesExpanded, DEFAULT_FOLDER_OPEN)
    } else {
        (Table::FolderNames, DEFAULT_FOLDER)
    };
    get(table, &lower).unwrap_or_else(|| default.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tables() -> impl Fn(Table, &str) -> Option<String> {
        let mut m: HashMap<(&str, &str), &str> = HashMap::new();
        m.insert(("fileNames", "package.json"), "nodejs");
        m.insert(("fileExtensions", "ts"), "typescript");
        m.insert(("fileExtensions", "d.ts"), "typescript-def");
        m.insert(("fileExtensions", "env"), "tune");
        m.insert(("folderNames", "src"), "folder-src");
        m.insert(("folderNamesExpanded", "src"), "folder-src-open");
        move |t, k| m.get(&(t.key(), k)).map(|s| (*s).to_owned())
    }

    #[test]
    fn exact_file_name_wins() {
        assert_eq!(file_icon("Package.JSON", tables()), "nodejs");
    }

    #[test]
    fn longest_extension_first() {
        assert_eq!(file_icon("index.d.ts", tables()), "typescript-def");
        assert_eq!(file_icon("main.TS", tables()), "typescript");
        assert_eq!(file_icon("app.test.ts", tables()), "typescript");
    }

    #[test]
    fn dotfiles_and_unknown_files() {
        assert_eq!(file_icon(".env", tables()), "tune");
        assert_eq!(file_icon("LICENSE", tables()), DEFAULT_FILE);
        assert_eq!(file_icon("x.unknown", tables()), DEFAULT_FILE);
    }

    #[test]
    fn folders_open_and_closed() {
        assert_eq!(folder_icon("SRC", false, tables()), "folder-src");
        assert_eq!(folder_icon("src", true, tables()), "folder-src-open");
        assert_eq!(folder_icon("misc", false, tables()), DEFAULT_FOLDER);
        assert_eq!(folder_icon("misc", true, tables()), DEFAULT_FOLDER_OPEN);
        assert_eq!(url("folder"), "icons/folder.svg");
    }
}
