//! Ribhu Project Switcher — opens with the `OpenProjectSwitcher` action (Cmd+O).
//!
//! A fuzzy-search modal over Ribhu project profiles. Shows:
//!   - Project name (bold)
//!   - Path (muted)
//!   - Last-opened timestamp (relative)
//!
//! Selecting a project opens it via the standard `workspace::open_paths`.

use std::{path::PathBuf, sync::Arc};

use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Task, WeakEntity, Window};
use picker::{Picker, PickerDelegate};
use ui::{ListItem, ListItemSpacing, prelude::*};
use util::ResultExt;
use workspace::{ModalView, RibhuProjectInfo, Workspace, WorkspaceDb};

// ── RibhuProjectSwitcher (modal wrapper) ─────────────────────────────────────

pub struct RibhuProjectSwitcher {
    picker: Entity<Picker<ProjectSwitcherDelegate>>,
}

impl ModalView for RibhuProjectSwitcher {}

impl EventEmitter<DismissEvent> for RibhuProjectSwitcher {}

impl Focusable for RibhuProjectSwitcher {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl RibhuProjectSwitcher {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        projects: Vec<RibhuProjectInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = ProjectSwitcherDelegate::new(workspace, projects, cx);
        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));
        RibhuProjectSwitcher { picker }
    }
}

impl Render for RibhuProjectSwitcher {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("RibhuProjectSwitcher")
            .w(rems(34.))
            .child(self.picker.clone())
    }
}

// ── ProjectSwitcherDelegate ───────────────────────────────────────────────────

pub struct ProjectSwitcherDelegate {
    workspace: WeakEntity<Workspace>,
    all_projects: Vec<RibhuProjectInfo>,
    matches: Vec<StringMatch>,
    selected_index: usize,
    switcher: WeakEntity<RibhuProjectSwitcher>,
}

impl ProjectSwitcherDelegate {
    fn new(
        workspace: WeakEntity<Workspace>,
        projects: Vec<RibhuProjectInfo>,
        cx: &mut Context<RibhuProjectSwitcher>,
    ) -> Self {
        // Initial matches — all projects, no query
        let matches = projects
            .iter()
            .enumerate()
            .map(|(i, p)| StringMatch {
                candidate_id: i,
                score: 0.0,
                positions: vec![],
                string: p.name.clone(),
            })
            .collect();
        ProjectSwitcherDelegate {
            workspace,
            all_projects: projects,
            matches,
            selected_index: 0,
            switcher: cx.weak_entity(),
        }
    }
}

impl PickerDelegate for ProjectSwitcherDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Switch project…".into()
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let candidates: Vec<StringMatchCandidate> = self
            .all_projects
            .iter()
            .enumerate()
            .map(|(i, p)| StringMatchCandidate::new(i, p.name.as_str()))
            .collect();
        let results = smol::block_on(fuzzy::match_strings(
            &candidates,
            &query,
            false,
            true,
            100,
            &Default::default(),
            cx.background_executor().clone(),
        ));
        self.matches = results;
        self.selected_index = 0;
        Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(m) = self.matches.get(self.selected_index) {
            if let Some(project) = self.all_projects.get(m.candidate_id) {
                let path = PathBuf::from(&project.path);
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace
                            .open_workspace_for_paths(false, vec![path], window, cx)
                            .detach();
                    });
                }
            }
        }
        self.dismissed(window, cx);
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.switcher
            .update(cx, |_, cx| cx.emit(DismissEvent))
            .log_err();
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let m = self.matches.get(ix)?;
        let project = self.all_projects.get(m.candidate_id)?;

        let name = SharedString::from(project.name.clone());
        let path = SharedString::from(project.path.clone());

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let age_secs = (now_secs - project.last_opened).max(0) as u64;
        let age_label: SharedString = if age_secs < 60 {
            "just now".into()
        } else if age_secs < 3600 {
            format!("{}m ago", age_secs / 60).into()
        } else if age_secs < 86400 {
            format!("{}h ago", age_secs / 3600).into()
        } else {
            format!("{}d ago", age_secs / 86400).into()
        };

        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    v_flex()
                        .child(Label::new(name))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(Label::new(path).color(Color::Muted))
                                .child(Label::new(age_label).color(Color::Muted)),
                        ),
                ),
        )
    }
}

// ── Action handler ────────────────────────────────────────────────────────────

/// Open the Ribhu project switcher modal. Wire to Cmd+O in keymap.
pub fn open_project_switcher(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let db = WorkspaceDb::global(cx);
    let weak = workspace.weak_handle();
    cx.spawn_in(window, async move |workspace, cx| {
        let projects = match db.list_ribhu_projects().await {
            Ok(p) => p,
            Err(e) => {
                log::error!("[ProfileSwitcher] failed to load projects: {e}");
                return;
            }
        };
        workspace
            .update_in(cx, |ws, window, cx| {
                ws.toggle_modal(window, cx, |window, cx| {
                    RibhuProjectSwitcher::new(weak, projects, window, cx)
                });
            })
            .ok();
    })
    .detach();
}
