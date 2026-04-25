//! API Client Panel — Phase 5 of the Ribhu feature set.

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

const API_CLIENT_PANEL_KEY: &str = "ApiClientPanel";

actions!(
    api_client_panel,
    [
        /// Toggles focus on the API client panel.
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
        workspace.toggle_panel_focus::<ApiClientPanel>(window, cx);
    });
}

pub struct ApiClientPanel {
    focus_handle: FocusHandle,
}

impl ApiClientPanel {
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

impl Focusable for ApiClientPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle { self.focus_handle.clone() }
}

impl EventEmitter<PanelEvent> for ApiClientPanel {}

impl Panel for ApiClientPanel {
    fn persistent_name() -> &'static str { "ApiClientPanel" }
    fn panel_key() -> &'static str { API_CLIENT_PANEL_KEY }
    fn position(&self, _: &Window, _: &App) -> DockPosition { DockPosition::Right }
    fn position_is_valid(&self, position: DockPosition) -> bool { matches!(position, DockPosition::Left | DockPosition::Right) }
    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}
    fn default_size(&self, _: &Window, _: &App) -> Pixels { px(280.) }
    fn icon(&self, _: &Window, _: &App) -> Option<IconName> { Some(IconName::ToolWeb) }
    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> { Some("API Client") }
    fn toggle_action(&self) -> Box<dyn Action> { Box::new(ToggleFocus) }
    fn activation_priority(&self) -> u32 { 12 }
}

impl PanelHeader for ApiClientPanel {}

impl Render for ApiClientPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("ApiClientPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(Label::new("API Client").size(LabelSize::Small).color(Color::Default))
    }
}
