//! Resource Monitor Panel — Phase 8B of the Ribhu feature set.

pub mod collectors;

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

const RESOURCE_MONITOR_PANEL_KEY: &str = "ResourceMonitorPanel";

actions!(
    resource_monitor,
    [
        /// Toggles focus on the resource monitor panel.
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
        workspace.toggle_panel_focus::<ResourceMonitorPanel>(window, cx);
    });
}

pub struct ResourceMonitorPanel {
    focus_handle: FocusHandle,
}

impl ResourceMonitorPanel {
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

impl Focusable for ResourceMonitorPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle { self.focus_handle.clone() }
}

impl EventEmitter<PanelEvent> for ResourceMonitorPanel {}

impl Panel for ResourceMonitorPanel {
    fn persistent_name() -> &'static str { "ResourceMonitorPanel" }
    fn panel_key() -> &'static str { RESOURCE_MONITOR_PANEL_KEY }
    fn position(&self, _: &Window, _: &App) -> DockPosition { DockPosition::Right }
    fn position_is_valid(&self, position: DockPosition) -> bool { matches!(position, DockPosition::Left | DockPosition::Right) }
    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}
    fn default_size(&self, _: &Window, _: &App) -> Pixels { px(280.) }
    fn icon(&self, _: &Window, _: &App) -> Option<IconName> { Some(IconName::BoltFilled) }
    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> { Some("Resource Monitor") }
    fn toggle_action(&self) -> Box<dyn Action> { Box::new(ToggleFocus) }
    fn activation_priority(&self) -> u32 { 14 }
}

impl PanelHeader for ResourceMonitorPanel {}

impl Render for ResourceMonitorPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("ResourceMonitorPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(Label::new("Resource Monitor").size(LabelSize::Small).color(Color::Default))
    }
}
