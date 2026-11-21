//! API tipada del backend. Es la única capa que llama a `bridge::tauri`.

use gryphus_ipc::*;

use crate::bridge::tauri::{invoke, invoke_with_channel};
use crate::bridge::xterm::TerminalChannel;

pub type IpcResult<T> = Result<T, IpcError>;

async fn call<Res: serde::de::DeserializeOwned>(cmd: &str) -> IpcResult<Res> {
    invoke::<(), Res>(cmd, None).await
}

pub async fn pick_folder() -> IpcResult<Option<String>> {
    call(cmd::PICK_FOLDER).await
}

pub async fn open_project(path: String) -> IpcResult<OpenProjectRes> {
    invoke(cmd::OPEN_PROJECT, Some(&OpenProjectReq { path })).await
}

pub async fn get_project() -> IpcResult<Option<ProjectDto>> {
    call(cmd::GET_PROJECT).await
}

pub async fn close_project() -> IpcResult<()> {
    call(cmd::CLOSE_PROJECT).await
}

pub async fn create_feature(name: String, worktree: WorktreeChoiceDto) -> IpcResult<u64> {
    invoke(
        cmd::CREATE_FEATURE,
        Some(&CreateFeatureReq { name, worktree }),
    )
    .await
}

pub async fn delete_feature(feature: u64, remove_worktrees: bool, force: bool) -> IpcResult<()> {
    invoke(
        cmd::DELETE_FEATURE,
        Some(&DeleteFeatureReq {
            feature,
            remove_worktrees,
            force,
        }),
    )
    .await
}

pub async fn spawn_agent(feature: u64, worktree: u64, provider: String) -> IpcResult<u64> {
    invoke(
        cmd::SPAWN_AGENT,
        Some(&SpawnAgentReq {
            feature,
            worktree,
            provider,
        }),
    )
    .await
}

pub async fn stop_agent(agent: u64) -> IpcResult<()> {
    invoke(cmd::STOP_AGENT, Some(&AgentReq { agent })).await
}

pub async fn restart_agent(agent: u64) -> IpcResult<()> {
    invoke(cmd::RESTART_AGENT, Some(&AgentReq { agent })).await
}

pub async fn remove_agent(agent: u64) -> IpcResult<()> {
    invoke(cmd::REMOVE_AGENT, Some(&AgentReq { agent })).await
}

pub async fn agent_input(agent: u64, data: String) -> IpcResult<()> {
    invoke(cmd::AGENT_INPUT, Some(&AgentInputReq { agent, data })).await
}

pub async fn agent_resize(agent: u64, cols: u16, rows: u16) -> IpcResult<()> {
    invoke(
        cmd::AGENT_RESIZE,
        Some(&AgentResizeReq { agent, cols, rows }),
    )
    .await
}

pub async fn attach_agent(agent: u64, channel: &TerminalChannel) -> IpcResult<()> {
    invoke_with_channel(cmd::ATTACH_AGENT, &AgentReq { agent }, channel).await
}

pub async fn detach_agent() -> IpcResult<()> {
    call(cmd::DETACH_AGENT).await
}

pub async fn list_dir(worktree: u64, path: String) -> IpcResult<Vec<DirEntryDto>> {
    invoke(cmd::LIST_DIR, Some(&PathReq { worktree, path })).await
}

pub async fn read_file(worktree: u64, path: String) -> IpcResult<FileContentDto> {
    invoke(cmd::READ_FILE, Some(&PathReq { worktree, path })).await
}

pub async fn write_file(worktree: u64, path: String, content: String) -> IpcResult<()> {
    invoke(
        cmd::WRITE_FILE,
        Some(&WriteFileReq {
            worktree,
            path,
            content,
        }),
    )
    .await
}

pub async fn git_status(worktree: u64) -> IpcResult<Vec<FileStatusDto>> {
    invoke(cmd::GIT_STATUS, Some(&WorktreeReq { worktree })).await
}

pub async fn git_diff(worktree: u64, base: Option<String>) -> IpcResult<DiffDto> {
    invoke(cmd::GIT_DIFF, Some(&DiffReq { worktree, base })).await
}

pub async fn git_overview() -> IpcResult<GitOverviewDto> {
    call(cmd::GIT_OVERVIEW).await
}

/// Marca el arranque en frío (ms desde el inicio del proceso).
pub async fn ui_ready() -> IpcResult<u64> {
    call(cmd::UI_READY).await
}
