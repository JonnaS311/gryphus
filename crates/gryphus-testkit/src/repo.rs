//! [`TempRepo`]: repositorio git real y aislado en un directorio temporal.
//!
//! Usa la CLI `git` con una configuración propia (`GIT_CONFIG_GLOBAL`) para no
//! depender de la del desarrollador. Se borra al soltarse.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Estado inicial: branch `main` con un commit que contiene `README.md` = `"hola\n"`.
pub struct TempRepo {
    _dir: TempDir,
    base: PathBuf,
    root: PathBuf,
    git_config: PathBuf,
}

impl TempRepo {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("gryphus-test-")
            .tempdir()
            .expect("crear directorio temporal");
        let base = dir.path().to_path_buf();
        let root = base.join("repo");
        fs::create_dir(&root).expect("crear repo");
        let git_config = base.join("gitconfig");
        fs::write(
            &git_config,
            "[user]\n\tname = Gryphus Test\n\temail = test@gryphus.invalid\n\
             [commit]\n\tgpgsign = false\n[core]\n\tautocrlf = false\n\
             [init]\n\tdefaultBranch = main\n",
        )
        .expect("escribir gitconfig");

        let repo = Self {
            _dir: dir,
            base,
            root,
            git_config,
        };
        repo.git(&repo.root, &["init", "-q", "-b", "main"]);
        repo.write_file(&repo.root, "README.md", "hola\n");
        repo.commit_all(&repo.root, "init");
        repo
    }

    /// Worktree principal.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directorio temporal que contiene el repo (para crear worktrees hermanos).
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Variables de entorno que aíslan git de la configuración del desarrollador.
    /// Úsalas también en el adaptador bajo prueba para que vea el mismo repo igual.
    pub fn git_env(&self) -> Vec<(String, String)> {
        vec![
            (
                "GIT_CONFIG_GLOBAL".to_owned(),
                self.git_config.to_string_lossy().into_owned(),
            ),
            ("GIT_CONFIG_NOSYSTEM".to_owned(), "1".to_owned()),
        ]
    }

    /// Ruta (aún inexistente) para un worktree nuevo.
    pub fn worktree_path(&self, name: &str) -> PathBuf {
        self.base.join("worktrees").join(name)
    }

    /// Ejecuta `git` en `cwd` y devuelve stdout. Entra en pánico si falla.
    pub fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CONFIG_GLOBAL", &self.git_config)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("no se pudo ejecutar git; ¿está instalado?");
        assert!(
            out.status.success(),
            "git {args:?} falló: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn write_file(&self, worktree: &Path, rel: &str, content: &str) {
        let path = worktree.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("crear directorios");
        }
        fs::write(path, content).expect("escribir archivo");
    }

    pub fn delete_file(&self, worktree: &Path, rel: &str) {
        fs::remove_file(worktree.join(rel)).expect("borrar archivo");
    }

    pub fn commit_all(&self, worktree: &Path, message: &str) {
        self.git(worktree, &["add", "-A"]);
        self.git(worktree, &["commit", "-q", "-m", message]);
    }

    pub fn create_branch(&self, name: &str) {
        self.git(&self.root, &["branch", name]);
    }

    /// Crea un worktree en una branch nueva y devuelve su ruta.
    pub fn add_worktree(&self, name: &str, branch: &str) -> PathBuf {
        let path = self.worktree_path(name);
        let path_str = path.to_string_lossy().into_owned();
        self.git(
            &self.root,
            &["worktree", "add", "-q", "-b", branch, &path_str],
        );
        path
    }
}

impl Default for TempRepo {
    fn default() -> Self {
        Self::new()
    }
}
