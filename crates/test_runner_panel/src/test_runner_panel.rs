//! Test Runner Panel — Phase 4 of the Ribhu feature set.

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

const TEST_RUNNER_PANEL_KEY: &str = "TestRunnerPanel";

actions!(
    test_runner_panel,
    [
        /// Toggles focus on the test runner panel.
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
        workspace.toggle_panel_focus::<TestRunnerPanel>(window, cx);
    });
}

pub struct TestRunnerPanel {
    focus_handle: FocusHandle,
}

impl TestRunnerPanel {
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

impl Focusable for TestRunnerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for TestRunnerPanel {}

impl Panel for TestRunnerPanel {
    fn persistent_name() -> &'static str { "TestRunnerPanel" }
    fn panel_key() -> &'static str { TEST_RUNNER_PANEL_KEY }
    fn position(&self, _: &Window, _: &App) -> DockPosition { DockPosition::Bottom }
    fn position_is_valid(&self, position: DockPosition) -> bool { matches!(position, DockPosition::Bottom) }
    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}
    fn default_size(&self, _: &Window, _: &App) -> Pixels { px(240.) }
    fn icon(&self, _: &Window, _: &App) -> Option<IconName> { Some(IconName::PlayFilled) }
    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> { Some("Test Runner") }
    fn toggle_action(&self) -> Box<dyn Action> { Box::new(ToggleFocus) }
    fn activation_priority(&self) -> u32 { 10 }
}

impl PanelHeader for TestRunnerPanel {}

impl Render for TestRunnerPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("TestRunnerPanel")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(Label::new("Test Runner").size(LabelSize::Small).color(Color::Default))
    }
}
