//! Comandos Tauri. Cada uno: valida la entrada, resuelve la sesión de la ventana
//! que llama y ejecuta el caso de uso en un hilo bloqueante (nunca en el hilo de UI).

use std::path::PathBuf;
use std::sync::Arc;

use gryphus_app::ProjectSession;
use gryphus_core::ports::{AgentChunk, ChunkSink, TermSize};
use gryphus_core::{AgentId, FeatureId, ProjectId, WorktreeId};
use gryphus_ipc::*;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::dto;
use crate::events::TauriEvents;
use crate::state::AppState;

type CmdResult<T> = Result<T, IpcError>;

fn internal(e: impl std::fmt::Display) -> IpcError {
    IpcError {
        code: "INTERNAL".into(),
        message: e.to_string(),
    }
}

/// Ejecuta trabajo bloqueante (git, disco, procesos) fuera del hilo de la UI.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(internal)?
}

/// La sesión siempre se deduce de la ventana que llama (ADR-003).
fn session(state: &AppState, window: &WebviewWindow) -> CmdResult<ProjectSession> {
    state
        .registry()
        .session(window.label())
        .ok_or_else(|| IpcError {
            code: "NO_PROJECT".into(),
            message: "esta ventana no tiene un proyecto abierto".into(),
        })
}

/// Atajo: resuelve la sesión y ejecuta `f` en un hilo bloqueante.
async fn with_session<T: Send + 'static>(
    state: &AppState,
    window: &WebviewWindow,
    f: impl FnOnce(ProjectSession) -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    let s = session(state, window)?;
    blocking(move || f(s)).await
}

fn project_dto(s: &ProjectSession) -> ProjectDto {
    dto::project(&s.project(), s.config(), |cmd| {
        gryphus_agents::resolve_program(cmd, None).is_some()
    })
}

// ── Métricas ─────────────────────────────────────────────────────────────────

static UI_READY_MS: std::sync::OnceLock<u64> = std::sync::OnceLock::new();

/// Arranque en frío: ms desde el inicio del proceso hasta que la primera UI se montó.
#[tauri::command]
pub fn ui_ready(app: AppHandle) -> u64 {
    *UI_READY_MS.get_or_init(|| {
        crate::startup::trace("ui_ready");
        if crate::startup::exit_when_ready() {
            app.exit(0);
        }
        crate::startup::elapsed_ms()
    })
}

// ── Proyecto y ventanas ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> CmdResult<Option<String>> {
    blocking(move || {
        Ok(app
            .dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
pub async fn open_project(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: OpenProjectReq,
) -> CmdResult<OpenProjectRes> {
    let path = PathBuf::from(&req.path);
    let git = state.git.clone();
    let root = blocking(move || {
        use gryphus_core::ports::GitPort;
        git.repo_info(&path)
            .map(|i| i.root)
            .map_err(|e| dto::error(e.into()))
    })
    .await?;

    // Un proyecto vive en una sola ventana: si ya está abierto, se enfoca.
    if let Some(label) = state.registry().label_for_root(&root) {
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.set_focus();
        }
        return Ok(OpenProjectRes {
            other_window: label != window.label(),
        });
    }

    let reuse_current = state.registry().session(window.label()).is_none();
    let label = if reuse_current {
        window.label().to_owned()
    } else {
        format!("project-{}", ProjectId::from_canonical_root(&root))
    };
    let events = Arc::new(TauriEvents {
        app: app.clone(),
        label: label.clone(),
    });
    let ports = state.ports(events);
    let open_root = root.clone();
    let session =
        blocking(move || ProjectSession::open(&open_root, ports).map_err(dto::error)).await?;
    let title = format!(
        "Gryphus — {}",
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    state.registry().bind(&label, session);

    if reuse_current {
        let _ = window.set_title(&title);
    } else {
        crate::window_builder(&app, &label, &title)
            .build()
            .map_err(internal)?;
    }
    Ok(OpenProjectRes {
        other_window: !reuse_current,
    })
}

/// `None` si la ventana aún no tiene proyecto (pantalla de bienvenida).
#[tauri::command]
pub async fn get_project(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> CmdResult<Option<ProjectDto>> {
    // El guard del registro no puede vivir a través del `await`.
    let current = state.registry().session(window.label());
    match current {
        None => Ok(None),
        Some(s) => blocking(move || Ok(Some(project_dto(&s)))).await,
    }
}

/// Cierra el proyecto de esta ventana sin cerrarla: suelta la terminal, detiene
/// los agentes y guarda. Idempotente.
#[tauri::command]
pub async fn close_project(window: WebviewWindow, state: State<'_, AppState>) -> CmdResult<()> {
    state.detach(window.label());
    let session = state.registry().remove(window.label());
    let _ = window.set_title("Gryphus");
    match session {
        None => Ok(()),
        Some(s) => blocking(move || s.close().map_err(dto::error)).await,
    }
}

// ── Features ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn create_feature(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: CreateFeatureReq,
) -> CmdResult<u64> {
    with_session(&state, &window, move |s| {
        s.create_feature(&req.name, dto::worktree_choice(req.worktree))
            .map(|f| f.get())
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn delete_feature(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: DeleteFeatureReq,
) -> CmdResult<()> {
    with_session(&state, &window, move |s| {
        s.delete_feature(
            FeatureId::from_raw(req.feature),
            req.remove_worktrees,
            req.force,
        )
        .map_err(dto::error)
    })
    .await
}

// ── Agentes ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn spawn_agent(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: SpawnAgentReq,
) -> CmdResult<u64> {
    let provider = dto::provider(&req.provider)?;
    with_session(&state, &window, move |s| {
        s.spawn_agent(
            FeatureId::from_raw(req.feature),
            WorktreeId::from_raw(req.worktree),
            provider,
        )
        .map(|a| a.get())
        .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn stop_agent(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: AgentReq,
) -> CmdResult<()> {
    with_session(&state, &window, move |s| {
        s.stop_agent(AgentId::from_raw(req.agent))
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn restart_agent(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: AgentReq,
) -> CmdResult<()> {
    with_session(&state, &window, move |s| {
        s.restart_agent(AgentId::from_raw(req.agent))
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn remove_agent(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: AgentReq,
) -> CmdResult<()> {
    with_session(&state, &window, move |s| {
        s.remove_agent(AgentId::from_raw(req.agent))
            .map_err(dto::error)
    })
    .await
}

/// Entrada de teclado: camino caliente, sin hilo bloqueante (escribir en la PTY es inmediato).
#[tauri::command]
pub fn agent_input(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: AgentInputReq,
) -> CmdResult<()> {
    session(&state, &window)?
        .agent_input(AgentId::from_raw(req.agent), req.data.as_bytes())
        .map_err(dto::error)
}

#[tauri::command]
pub fn agent_resize(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: AgentResizeReq,
) -> CmdResult<()> {
    session(&state, &window)?
        .agent_resize(
            AgentId::from_raw(req.agent),
            TermSize {
                cols: req.cols.max(1),
                rows: req.rows.max(1),
            },
        )
        .map_err(dto::error)
}

struct ChannelSink(Channel<InvokeResponseBody>);

impl ChunkSink for ChannelSink {
    fn send(&self, chunk: AgentChunk) -> bool {
        self.0
            .send(InvokeResponseBody::Raw(dto::chunk(chunk)))
            .is_ok()
    }
}

/// Conecta la terminal de la ventana a un agente (sustituye la conexión anterior).
#[tauri::command]
pub fn attach_agent(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: AgentReq,
    channel: Channel<InvokeResponseBody>,
) -> CmdResult<()> {
    let s = session(&state, &window)?;
    state.detach(window.label());
    let sub = s
        .attach_agent(AgentId::from_raw(req.agent), Arc::new(ChannelSink(channel)))
        .map_err(dto::error)?;
    state.attach(window.label(), sub);
    Ok(())
}

#[tauri::command]
pub fn detach_agent(window: WebviewWindow, state: State<'_, AppState>) {
    state.detach(window.label());
}

// ── Archivos ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_dir(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: PathReq,
) -> CmdResult<Vec<DirEntryDto>> {
    with_session(&state, &window, move |s| {
        s.list_dir(WorktreeId::from_raw(req.worktree), &req.path)
            .map(|v| v.into_iter().map(dto::dir_entry).collect())
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn read_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: PathReq,
) -> CmdResult<FileContentDto> {
    with_session(&state, &window, move |s| {
        s.read_file(WorktreeId::from_raw(req.worktree), &req.path)
            .map(dto::file_content)
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn write_file(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: WriteFileReq,
) -> CmdResult<()> {
    with_session(&state, &window, move |s| {
        s.write_file(
            WorktreeId::from_raw(req.worktree),
            &req.path,
            req.content.as_bytes(),
        )
        .map_err(dto::error)
    })
    .await
}

// ── Git ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn git_status(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: WorktreeReq,
) -> CmdResult<Vec<FileStatusDto>> {
    with_session(&state, &window, move |s| {
        s.git_status(WorktreeId::from_raw(req.worktree))
            .map(|v| v.into_iter().map(dto::file_status).collect())
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn git_diff(
    window: WebviewWindow,
    state: State<'_, AppState>,
    req: DiffReq,
) -> CmdResult<DiffDto> {
    let base = dto::diff_base(req.base)?;
    with_session(&state, &window, move |s| {
        s.git_diff(WorktreeId::from_raw(req.worktree), &base)
            .map(dto::diff)
            .map_err(dto::error)
    })
    .await
}

#[tauri::command]
pub async fn git_overview(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> CmdResult<GitOverviewDto> {
    with_session(&state, &window, move |s| {
        s.git_overview().map(dto::overview).map_err(dto::error)
    })
    .await
}
