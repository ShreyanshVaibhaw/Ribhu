//! Cloud Connect Panel — Phase 8A of the Ribhu feature set.

pub mod providers;
pub mod session;

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

const CLOUD_CONNECT_PANEL_KEY: &str = "CloudConnectPanel";

actions!(
    cloud_connect,
    [
        /// Toggles focus on the cloud connect panel.
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
        workspace.toggle_panel_focus::<CloudConnectPanel>(window, cx);
    });
}

pub struct CloudConnectPanel {
    focus_handle: FocusHandle,
}

impl CloudConnectPanel {
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

impl Focusable for CloudConnectPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle { self.focus_handle.clone() }
}

impl EventEmitter<PanelEvent> for CloudConnectPanel {}

impl Panel for CloudConnectPanel {
    fn persistent_name() -> &'static str { "CloudConnectPanel" }
    fn panel_key() -> &'static str { CLOUD_CONNECT_PANEL_KEY }
    fn position(&self, _: &Window, _: &App) -> DockPosition { DockPosition::Left }
    fn position_is_valid(&self, position: DockPosition) -> bool { matches!(position, DockPosition::Left | DockPosition::Right) }
    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}
    fn default_size(&self, _: &Window, _: &App) -> Pixels { px(260.) }
    fn icon(&self, _: &Window, _: &App) -> Option<IconName> { Some(IconName::Server) }
    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> { Some("Cloud Connect") }
    fn toggle_action(&self) -> Box<dyn Action> { Box::new(ToggleFocus) }
    fn activation_priority(&self) -> u32 { 11 }
}

impl PanelHeader for CloudConnectPanel {}

impl Render for CloudConnectPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("CloudConnectPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(Label::new("Cloud Connect").size(LabelSize::Small).color(Color::Default))
    }
}
