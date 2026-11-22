//! Suites de contrato: el comportamiento que debe cumplir **cualquier**
//! implementación de un puerto (fake o real).
//!
//! Cada suite define un *harness* (cómo obtener el puerto y cómo preparar el
//! escenario desde fuera del puerto) y una lista de casos. Una macro genera un
//! `#[test]` por caso:
//!
//! ```ignore
//! mod real_git {
//!     struct H { /* TempRepo + GixGit */ }
//!     impl gryphus_testkit::GitHarness for H { /* ... */ }
//!     gryphus_testkit::git_contract!(H::new);
//! }
//! ```

pub mod agents;
pub mod fs;
pub mod git;
pub mod store;

pub use agents::AgentHarness;
pub use fs::FsHarness;
pub use git::GitHarness;
pub use store::StoreHarness;

use std::fmt::Debug;
use std::path::Path;

use gryphus_core::ports::{PortErrorKind, PortResult};

/// Falla si `result` no es un error del tipo `kind`.
#[track_caller]
pub fn assert_kind<T: Debug>(result: PortResult<T>, kind: PortErrorKind) {
    match result {
        Err(e) if e.kind == kind => {}
        other => panic!("se esperaba un error {kind:?}, se obtuvo {other:?}"),
    }
}

/// Igualdad de rutas tolerante a canonicalización (real) y a rutas virtuales (fakes).
pub fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __contract_cases {
    ($suite:ident, $make:expr; $($case:ident),+ $(,)?) => {
        $(
            #[test]
            fn $case() {
                let harness = ($make)();
                $crate::contracts::$suite::$case(&harness);
            }
        )+
    };
}

/// Genera un `#[test]` por caso del contrato de [`gryphus_core::ports::GitPort`].
#[macro_export]
macro_rules! git_contract {
    ($make:expr) => {
        $crate::__contract_cases!(git, $make;
            repo_info_reports_main_branch,
            lists_only_the_main_worktree_initially,
            adds_worktree_on_new_branch,
            new_worktree_is_clean,
            adds_worktree_for_existing_branch,
            rejects_creating_an_existing_branch,
            rejects_branch_checked_out_elsewhere,
            rejects_unknown_base,
            status_reports_untracked_modified_and_deleted,
            status_is_clean_after_commit,
            diff_against_head_shows_line_changes,
            diff_includes_untracked_files_as_added,
            diff_against_merge_base_only_includes_feature_work,
            removes_clean_worktree_but_keeps_branch,
            refuses_to_remove_dirty_worktree_without_force,
            manually_deleted_worktree_is_not_listed,
            prune_frees_the_branch_of_a_manually_deleted_worktree,
            unknown_worktree_is_not_found,
        );
    };
}

/// Genera un `#[test]` por caso del contrato de [`gryphus_core::ports::FsPort`].
#[macro_export]
macro_rules! fs_contract {
    ($make:expr) => {
        $crate::__contract_cases!(fs, $make;
            lists_dirs_first_then_files_case_insensitively,
            lists_nested_directories,
            hides_the_git_directory,
            marks_gitignored_entries,
            list_dir_errors,
            reads_text_binary_and_too_large,
            read_file_errors,
            write_file_round_trips_and_creates_parents,
            write_file_overwrites,
            rename_moves_files_and_directories,
            rename_errors,
            copy_duplicates_files_and_directories,
            copy_errors,
            watch_reports_external_changes,
            dropping_the_subscription_stops_events,
        );
    };
}

/// Genera un `#[test]` por caso del contrato de [`gryphus_core::ports::StorePort`].
#[macro_export]
macro_rules! store_contract {
    ($make:expr) => {
        $crate::__contract_cases!(store, $make;
            missing_project_is_none,
            saved_project_round_trips,
            saving_again_overwrites,
            config_defaults_when_absent,
            unreadable_project_is_corrupted,
            project_violating_invariants_is_corrupted,
        );
    };
}

/// Genera un `#[test]` por caso del contrato de [`gryphus_core::ports::AgentRuntimePort`].
#[macro_export]
macro_rules! agents_contract {
    ($make:expr) => {
        $crate::__contract_cases!(agents, $make;
            attach_receives_the_banner,
            echoes_input_lines,
            exit_command_reports_the_exit_code,
            kill_reports_exit_exactly_once,
            late_attach_receives_scrollback,
            flood_arrives_complete_and_in_order,
            resize_live_agent,
            can_respawn_after_exit,
            missing_program_is_unavailable,
            unknown_agent_is_not_found,
        );
    };
}
