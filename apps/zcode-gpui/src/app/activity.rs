use crate::app::{dock::DockTab, root::RootView};
use crate::conversation::model::ConversationState;
use crate::shared::i18n::label;
use ely_gpui_component::{
    buttons::{Button, ButtonVariant},
    primitives::{IconName, Tooltip},
    theme::ControlSize,
};
use gpui::{AnyElement, Context, ParentElement, Styled, Window, div, prelude::*};

#[derive(Default, Debug, PartialEq)]
pub(crate) struct ActivityCounts {
    pub bash: usize,
    pub workflows: usize,
    pub subagents: usize,
}

impl ActivityCounts {
    pub(crate) fn from_conversation(conv: &ConversationState) -> Self {
        Self {
            bash: conv
                .background_works
                .iter()
                .filter(|w| w.kind == "bash" && w.status == "running")
                .count(),
            workflows: conv
                .background_works
                .iter()
                .filter(|w| w.kind == "workflow" && w.status == "running")
                .count(),
            subagents: conv.running_subagents().len(),
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.bash + self.workflows + self.subagents
    }

    pub(crate) fn label(&self, compact: bool) -> String {
        if compact {
            return self.total().to_string();
        }
        [
            (self.bash, label("terminals", "终端")),
            (self.workflows, label("workflows", "工作流")),
            (self.subagents, label("agents", "智能体")),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, kind)| format!("{count} {kind}"))
        .collect::<Vec<_>>()
        .join(" · ")
    }

    fn icon(&self) -> IconName {
        match (self.bash > 0, self.workflows > 0, self.subagents > 0) {
            (false, false, true) => IconName::Bot,
            (true, false, false) => IconName::Terminal,
            (false, true, false) => IconName::Workflow,
            _ => IconName::Activity,
        }
    }
}

impl RootView {
    pub(crate) fn composer_activity(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let counts = self
            .state
            .read(cx)
            .active_conversation()
            .map(ActivityCounts::from_conversation)?;
        if counts.total() == 0 {
            return None;
        }
        let tooltip = format!(
            "{}: {} {}, {} {}, {} {}",
            label("Open background work", "打开后台工作"),
            counts.subagents,
            label("agents", "智能体"),
            counts.bash,
            label("terminals", "终端"),
            counts.workflows,
            label("workflows", "工作流")
        );
        let button = Button::new(
            "composer-background-work",
            counts.label(self.composer_compact),
        )
        .icon(if self.composer_compact {
            IconName::Activity
        } else {
            counts.icon()
        })
        .variant(ButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .on_click(cx.listener(|this, _, _, cx| {
            cx.stop_propagation();
            this.dock_open = true;
            this.dock_tab = DockTab::Review;
            this.agents_expanded = true;
            cx.notify();
        }));
        let element = div()
            .id("composer-activity-wrapper")
            .flex_none()
            .tooltip(Tooltip::text(tooltip))
            .child(button);
        let element = div().child(element);
        #[cfg(test)]
        let element =
            crate::app::test_support::track_children(element, vec!["composer-activity".into()]);
        Some(element.into_any_element())
    }

    pub(crate) fn activity_cancel_button(
        &self,
        id: String,
        work_id: String,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.state.read(cx);
        let (workspace, parent) = state.active_ws_key().zip(state.active.clone())?;
        if state.is_read_only_view() {
            return None;
        }
        let pending = state.cancellation_pending(&workspace, &parent, &work_id);
        Some(
            Button::new(gpui::SharedString::from(id), label("Stop", "停止"))
                .variant(ButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .loading(pending)
                .disabled(pending)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, cx| {
                        state.cancel_background_work_for(&workspace, &parent, &work_id, cx)
                    });
                }))
                .into_any_element(),
        )
    }

    pub(crate) fn render_background_activity(&self, cx: &mut Context<Self>) -> AnyElement {
        let works = self
            .state
            .read(cx)
            .active_conversation()
            .map(|conv| {
                conv.background_works
                    .iter()
                    .filter(|work| {
                        work.status == "running"
                            && matches!(work.kind.as_str(), "bash" | "workflow")
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .max_h(gpui::px(160.))
            .id("background-activity")
            .overflow_y_scroll()
            .children(works.into_iter().map(|work| {
                let icon = if work.kind == "bash" {
                    IconName::Terminal
                } else {
                    IconName::Workflow
                };
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .child(ely_gpui_component::primitives::Icon::new(icon))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(gpui::px(crate::shared::theme::font_size_sm()))
                            .child(work.title),
                    )
                    .children(
                        (work.cancellable != Some(false))
                            .then(|| {
                                self.activity_cancel_button(
                                    format!("activity-stop-{}", work.work_id),
                                    work.work_id,
                                    cx,
                                )
                            })
                            .flatten(),
                    )
            }))
            .into_any_element()
    }

    pub(crate) fn open_child_conversation(
        &mut self,
        workspace: &str,
        parent: &str,
        child: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| {
            state.open_subagent_for(workspace, parent, child, cx)
        });
        if self.state.read(cx).viewing_child.as_deref() == Some(child) {
            // 隐藏 composer 不会自动撤销焦点；离开父输入后才能保证 child 真正只读。
            window.focus(&self.settings.focus, cx);
            // 原先这里会关闭侧栏，且返回父会话时没有恢复，用户每次都要重新打开。
            // Desktop 打开子代理不改变侧栏开合（useAppPanels.handleOpenSubagentSession），故保持原状。
        }
        cx.notify();
    }

    pub(crate) fn guard_child_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        let state = self.state.read(cx);
        if state.is_read_only_view() && state.composer.read(cx).focus.is_focused(window) {
            window.focus(&self.settings.focus, cx);
        }
    }

    pub(crate) fn close_child_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.close_subagent(cx));
        // Back from a nested Explore lands in its (read-only) parent subagent.
        if !self.state.read(cx).is_read_only_view() {
            self.return_focus_to_composer(window, cx);
        }
        cx.notify();
    }
}
