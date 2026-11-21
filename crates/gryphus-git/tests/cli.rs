//! Casos propios de la CLI que el contrato no cubre.

use gryphus_core::ports::{DiffBase, DiffLine, FileState, GitPort, PortErrorKind};
use gryphus_core::BranchName;
use gryphus_git::CliGit;
use gryphus_testkit::contracts::assert_kind;
use gryphus_testkit::TempRepo;

fn setup() -> (TempRepo, CliGit) {
    let repo = TempRepo::new();
    let git = repo
        .git_env()
        .into_iter()
        .fold(CliGit::new(), |git, (k, v)| git.env(k, v));
    (repo, git)
}

#[test]
fn paths_with_spaces_and_unicode_survive_status_and_diff() {
    let (repo, git) = setup();
    let root = repo.root();
    repo.write_file(root, "carpeta con espacio/ñandú (copia).txt", "uno\n");
    repo.commit_all(root, "raro");
    repo.write_file(root, "carpeta con espacio/ñandú (copia).txt", "uno\ndos\n");

    let status = git.status(root).unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(
        status[0].path.as_str(),
        "carpeta con espacio/ñandú (copia).txt"
    );
    assert_eq!(status[0].state, FileState::Modified);

    let diff = git.diff(root, &DiffBase::Head).unwrap();
    assert_eq!(
        diff.files[0].path.as_str(),
        "carpeta con espacio/ñandú (copia).txt"
    );
    assert!(diff.files[0].hunks[0]
        .lines
        .contains(&DiffLine::Added("dos".into())));
}

#[test]
fn staged_only_changes_are_marked_staged() {
    let (repo, git) = setup();
    let root = repo.root();
    repo.write_file(root, "nuevo.txt", "x\n");
    repo.git(root, &["add", "nuevo.txt"]);
    let status = git.status(root).unwrap();
    assert_eq!(status[0].state, FileState::Added);
    assert!(status[0].staged);
}

#[test]
fn branches_report_upstream_and_ahead_behind() {
    let (repo, git) = setup();
    let root = repo.root();
    repo.create_branch("dev");
    repo.write_file(root, "a.txt", "a\n");
    repo.commit_all(root, "a");
    repo.write_file(root, "b.txt", "b\n");
    repo.commit_all(root, "b");
    repo.git(root, &["branch", "--set-upstream-to=main", "dev"]);

    let branches = git.list_branches(root).unwrap();
    let dev = branches
        .iter()
        .find(|b| b.name.as_str() == "dev")
        .expect("dev");
    assert_eq!(dev.upstream.as_deref(), Some("main"));
    assert_eq!((dev.ahead, dev.behind), (0, 2));
    assert!(!dev.is_remote && !dev.checked_out);
}

#[test]
fn detached_head_has_no_branch() {
    let (repo, git) = setup();
    let root = repo.root();
    repo.git(root, &["checkout", "-q", "--detach"]);
    assert_eq!(git.repo_info(root).unwrap().head, None);
    assert_eq!(git.list_worktrees(root).unwrap()[0].branch, None);
}

#[test]
fn untracked_binary_files_are_marked_binary() {
    let (repo, git) = setup();
    std::fs::write(repo.root().join("img.bin"), [0u8, 159, 146, 150]).unwrap();
    let diff = git.diff(repo.root(), &DiffBase::Head).unwrap();
    let file = diff
        .files
        .iter()
        .find(|f| f.path.as_str() == "img.bin")
        .unwrap();
    assert!(file.binary && file.hunks.is_empty());
}

/// Abrir un repo recién creado (`git init`, sin commits) no debe romper el diff.
#[test]
fn diff_in_a_repo_without_commits_shows_staged_and_untracked_files() {
    let (repo, git) = setup();
    let empty = repo.base().join("vacio");
    std::fs::create_dir(&empty).unwrap();
    repo.git(&empty, &["init", "-q"]);
    std::fs::write(empty.join("indexado.txt"), "a\n").unwrap();
    repo.git(&empty, &["add", "indexado.txt"]);
    std::fs::write(empty.join("nuevo.txt"), "b\n").unwrap();

    let diff = git.diff(&empty, &DiffBase::Head).unwrap();
    let paths: Vec<_> = diff.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["indexado.txt", "nuevo.txt"]);
    assert_eq!(diff.files[0].hunks[0].lines, [DiffLine::Added("a".into())]);
}

#[test]
fn merge_base_with_unknown_branch_is_not_found() {
    let (repo, git) = setup();
    assert_kind(
        git.diff(
            repo.root(),
            &DiffBase::MergeBase(BranchName::parse("nada").unwrap()),
        ),
        PortErrorKind::NotFound,
    );
}

#[test]
fn directory_outside_a_repo_is_not_found() {
    let (repo, git) = setup();
    let outside = repo.base().join("fuera");
    std::fs::create_dir(&outside).unwrap();
    assert_kind(git.repo_info(&outside), PortErrorKind::NotFound);
    assert_kind(git.status(&outside), PortErrorKind::NotFound);
}

#[test]
fn main_worktree_cannot_be_removed() {
    let (repo, git) = setup();
    assert_kind(
        git.remove_worktree(repo.root(), repo.root(), true),
        PortErrorKind::InvalidInput,
    );
}

#[test]
fn base_that_looks_like_an_option_is_rejected() {
    let (repo, git) = setup();
    let req = gryphus_core::ports::NewWorktree {
        path: repo.worktree_path("x"),
        branch: BranchName::parse("feat/x").unwrap(),
        create_branch: true,
        base: Some("--orphan".into()),
    };
    assert_kind(
        git.add_worktree(repo.root(), &req),
        PortErrorKind::InvalidInput,
    );
}

#[test]
fn missing_git_binary_is_unavailable() {
    let (repo, _) = setup();
    let git = CliGit::with_program("gryphus-no-existe-git");
    assert_kind(git.status(repo.root()), PortErrorKind::Unavailable);
}
