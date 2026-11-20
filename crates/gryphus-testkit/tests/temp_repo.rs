//! `TempRepo` crea repos git reales y aislados (requiere `git` instalado).

use gryphus_testkit::TempRepo;

#[test]
fn starts_clean_on_main_with_readme() {
    let repo = TempRepo::new();
    let root = repo.root();
    assert_eq!(repo.git(root, &["branch", "--show-current"]).trim(), "main");
    assert_eq!(repo.git(root, &["status", "--porcelain"]), "");
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "hola\n"
    );
    assert_eq!(repo.git(root, &["log", "--format=%s"]).trim(), "init");
}

#[test]
fn creates_worktrees_and_commits_in_them() {
    let repo = TempRepo::new();
    let wt = repo.add_worktree("login", "feat/login");
    assert!(wt.join("README.md").exists());
    repo.write_file(&wt, "src/a.txt", "a\n");
    repo.commit_all(&wt, "a");
    assert_eq!(
        repo.git(&wt, &["branch", "--show-current"]).trim(),
        "feat/login"
    );
    assert_eq!(repo.git(&wt, &["status", "--porcelain"]), "");
    let list = repo.git(repo.root(), &["worktree", "list", "--porcelain"]);
    assert!(list.contains("branch refs/heads/feat/login"), "{list}");
}

#[test]
fn is_removed_on_drop() {
    let base = {
        let repo = TempRepo::new();
        repo.base().to_path_buf()
    };
    assert!(!base.exists());
}
