//! Database Panel — Phase 6 of the Ribhu feature set.

use gpui::{
    Action, App, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, ParentElement, Render, Styled, WeakEntity, Window, actions, px,
};
use panel::PanelHeader;
use ui::{
    Color, IconName, Label, LabelSize,
    prelude::*,
};
use workspace::{
    Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};

const DATABASE_PANEL_KEY: &str = "DatabasePanel";

actions!(
    database_panel,
    [
        /// Toggles focus on the database panel.
        ToggleFocus,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        register(workspace);
    })
    .detach();
}

pub fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<DatabasePanel>(window, cx);
    });
}

pub struct DatabasePanel {
    focus_handle: FocusHandle,
}

impl DatabasePanel {
    pub async fn load(
        workspace: WeakEntity<Workspace>,
        mut cx: AsyncWindowContext,
    ) -> anyhow::Result<Entity<Self>> {
        workspace.update_in(&mut cx, |_workspace, _window, cx| {
            cx.new(|cx| Self {
                focus_handle: cx.focus_handle(),
            })
        })
    }
}

impl Focusable for DatabasePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle { self.focus_handle.clone() }
}

impl EventEmitter<PanelEvent> for DatabasePanel {}

impl Panel for DatabasePanel {
    fn persistent_name() -> &'static str { "DatabasePanel" }
    fn panel_key() -> &'static str { DATABASE_PANEL_KEY }
    fn position(&self, _: &Window, _: &App) -> DockPosition { DockPosition::Right }
    fn position_is_valid(&self, position: DockPosition) -> bool { matches!(position, DockPosition::Left | DockPosition::Right) }
    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}
    fn default_size(&self, _: &Window, _: &App) -> Pixels { px(280.) }
    fn icon(&self, _: &Window, _: &App) -> Option<IconName> { Some(IconName::Server) }
    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> { Some("Database") }
    fn toggle_action(&self) -> Box<dyn Action> { Box::new(ToggleFocus) }
    fn activation_priority(&self) -> u32 { 13 }
}

impl PanelHeader for DatabasePanel {}

impl Render for DatabasePanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DatabasePanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(Label::new("Database").size(LabelSize::Small).color(Color::Default))
    }
}
