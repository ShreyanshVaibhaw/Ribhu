//! Ribhu Phase 3 — Project Profiles
//!
//! Adds named profile persistence on top of Zed's existing workspace save/restore:
//!   - Terminal session CWDs
//!   - Running dev-server processes (npm run dev, cargo run, etc.)
//!   - AI conversation context from AiConductor
//!
//! Key types:
//!   `ProfileManager` — GPUI entity that auto-saves every 30 seconds.
//!   `RibhuProjectSwitcher` — Modal (Cmd+O) for switching projects.

pub mod process_detector;
pub mod remote_state;
pub mod switcher;

use ai_conductor::AiConductor;
use gpui::{AppContext as _, Context, Entity, EventEmitter, Task, WeakEntity};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
use workspace::{Workspace, WorkspaceDb};

// ── Terminal session state ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSession {
    /// Working directory for the terminal tab.
    pub cwd: PathBuf,
    /// Tab index within the terminal panel.
    pub tab_index: usize,
}

// ── Dev-server process state ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevProcess {
    pub command: String,
    pub cwd: PathBuf,
    /// Terminal tab index this process was running in.
    pub tab_index: usize,
}

// ── Full profile state ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileState {
    pub terminal_sessions: Vec<TerminalSession>,
    pub dev_processes: Vec<DevProcess>,
    /// Compressed context summary from ContextWindowManager.
    pub context_summary: Option<String>,
}

// ── ProfileManager ────────────────────────────────────────────────────────────

/// Manages Ribhu project profile state for one workspace.
///
/// Create via `ProfileManager::register(workspace, ai_conductor, cx)`.
pub struct ProfileManager {
    project_id: Option<String>,
    project_path: Option<PathBuf>,
    state: ProfileState,
    ai_conductor: Entity<AiConductor>,
    _workspace: WeakEntity<Workspace>,
    db: WorkspaceDb,
    dirty: bool,
    _auto_save_task: Task<()>,
}

impl EventEmitter<ProfileManagerEvent> for ProfileManager {}

pub enum ProfileManagerEvent {
    Saved,
    Loaded,
}

impl ProfileManager {
    const AUTO_SAVE_INTERVAL: Duration = Duration::from_secs(30);

    pub fn register(
        workspace: &mut Workspace,
        ai_conductor: Entity<AiConductor>,
        cx: &mut Context<Workspace>,
    ) -> Entity<Self> {
        let weak = workspace.weak_handle();
        let db = WorkspaceDb::global(cx);

        cx.new(|cx: &mut Context<ProfileManager>| {
            let auto_save_task = cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(ProfileManager::AUTO_SAVE_INTERVAL)
                    .await;
                let should_save = this
                    .update(cx, |mgr, _| mgr.dirty)
                    .unwrap_or(false);
                if should_save {
                    if let Err(error) = this.update(cx, |mgr, cx| mgr.save(cx)) {
                        log::error!("Profile auto-save failed: {error}");
                    }
                }
            });

            ProfileManager {
                project_id: None,
                project_path: None,
                state: ProfileState::default(),
                ai_conductor,
                _workspace: weak,
                db,
                dirty: false,
                _auto_save_task: auto_save_task,
            }
        })
    }

    /// Call after workspace has loaded its project so we know the path.
    pub fn activate_project(&mut self, project_path: PathBuf, cx: &mut Context<Self>) {
        let path_str = project_path.to_string_lossy().to_string();
        self.project_path = Some(project_path);
        let db = self.db.clone();
        cx.spawn(async move |this, cx| {
            let project_id = match db.get_or_create_ribhu_project(&path_str).await {
                Ok(id) => id,
                Err(e) => {
                    log::error!("[ProfileManager] failed to get/create project: {e}");
                    return;
                }
            };
            if let Err(error) = db.touch_ribhu_project(&project_id).await {
                log::error!("[ProfileManager] failed to touch project: {error}");
            }

            // Load saved state
            let state = if let Ok(Some(json)) = db.load_ribhu_profile(&project_id, "sessions").await {
                serde_json::from_str::<ProfileState>(&json).ok()
            } else {
                None
            };

            if let Err(error) = this.update(cx, |mgr, cx| {
                mgr.project_id = Some(project_id);
                if let Some(state) = state {
                    mgr.state = state;
                }
                cx.emit(ProfileManagerEvent::Loaded);
            }) {
                log::error!("[ProfileManager] failed to update after load: {error}");
            }
        })
        .detach();
    }

    // ── State capture ─────────────────────────────────────────────────────────

    /// Capture terminal session cwds. Called by the terminal panel hook.
    pub fn capture_terminal_sessions(&mut self, sessions: Vec<TerminalSession>) {
        self.state.terminal_sessions = sessions;
        self.dirty = true;
    }

    /// Capture detected dev-server processes.
    pub fn capture_dev_processes(&mut self, processes: Vec<DevProcess>) {
        self.state.dev_processes = processes;
        self.dirty = true;
    }

    /// Snapshot AI context from the conductor into the profile state.
    pub fn snapshot_ai_context(&mut self, cx: &Context<Self>) {
        let summary = self.ai_conductor.read(cx).prompt_context();
        self.state.context_summary = if summary.is_empty() { None } else { Some(summary) };
        self.dirty = true;
    }

    // ── Persistence ───────────────────────────────────────────────────────────

    /// Persist current state asynchronously.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let Some(ref pid) = self.project_id else { return };
        let pid = pid.clone();
        let state = self.state.clone();
        let db = self.db.clone();
        self.dirty = false;
        cx.spawn(async move |this, cx| {
            match serde_json::to_string(&state) {
                Ok(json) => {
                    if let Err(e) = db.save_ribhu_profile(&pid, "sessions", &json).await {
                        log::error!("[ProfileManager] save failed: {e}");
                        return;
                    }
                    let _ = this.update(cx, |_, cx| cx.emit(ProfileManagerEvent::Saved));
                }
                Err(e) => {
                    log::error!("[ProfileManager] serialize failed: {e}");
                }
            }
        })
        .detach();
    }

    // ── Public accessors ──────────────────────────────────────────────────────

    pub fn current_state(&self) -> &ProfileState {
        &self.state
    }

    pub fn project_id(&self) -> Option<&str> {
        self.project_id.as_deref()
    }

    pub fn project_path(&self) -> Option<&PathBuf> {
        self.project_path.as_ref()
    }

    /// Force-save before closing the workspace.
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        self.snapshot_ai_context(cx);
        self.save(cx);
    }
}

// ── Public init ───────────────────────────────────────────────────────────────

/// Initialize the ProfileManager for a workspace. Call from workspace init.
pub fn init(
    workspace: &mut Workspace,
    ai_conductor: Entity<AiConductor>,
    cx: &mut Context<Workspace>,
) -> Entity<ProfileManager> {
    ProfileManager::register(workspace, ai_conductor, cx)
}
