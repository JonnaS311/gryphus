//! Presupuesto (docs/architecture/overview.md): `git status` en un repo de
//! 10 000 archivos < 150 ms. Ejecutar con `cargo bench -p gryphus-git`.

use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use gryphus_core::ports::{DiffBase, GitPort};
use gryphus_git::CliGit;
use gryphus_testkit::TempRepo;

const FILES: usize = 10_000;
const DIRS: usize = 100;
const MODIFIED: usize = 10;

fn big_repo() -> TempRepo {
    let repo = TempRepo::new();
    let root = repo.root();
    for i in 0..FILES {
        repo.write_file(
            root,
            &format!("dir{:03}/file{i:05}.txt", i % DIRS),
            &format!("contenido {i}\n"),
        );
    }
    repo.commit_all(root, "10k archivos");
    for i in 0..MODIFIED {
        repo.write_file(
            root,
            &format!("dir{:03}/file{i:05}.txt", i % DIRS),
            "modificado\n",
        );
    }
    repo
}

fn bench(c: &mut Criterion) {
    let repo = big_repo();
    let git = repo
        .git_env()
        .into_iter()
        .fold(CliGit::new(), |git, (k, v)| git.env(k, v));
    let root = repo.root();
    // Calienta la caché del índice y del sistema de archivos.
    assert_eq!(git.status(root).unwrap().len(), MODIFIED);

    let mut group = c.benchmark_group("git_10k_files");
    group
        .sample_size(30)
        .measurement_time(Duration::from_secs(10));
    group.bench_function("status", |b| b.iter(|| git.status(root).unwrap()));
    group.bench_function("diff_head", |b| {
        b.iter(|| git.diff(root, &DiffBase::Head).unwrap())
    });
    group.bench_function("list_worktrees", |b| {
        b.iter(|| git.list_worktrees(root).unwrap())
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
