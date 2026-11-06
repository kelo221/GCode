use crate::app::root::RootView;
use crate::review::agent_card::summary_preview;
use crate::shared::theme_colors::color as rgb;
use crate::shared::{
    i18n::label,
    theme::{MUTED, TEXT, ui_size},
};
use ely_gpui_component::{
    buttons::{Button, ButtonVariant},
    theme::ControlSize,
};
use gpui::StatefulInteractiveElement;
use gpui::{AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

impl RootView {
    pub(crate) fn render_ended_agents(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.agents_expanded {
            return None;
        }
        self.state.update(cx, |state, cx| {
            state.ensure_subagent_directory(false, false, cx)
        });
        let nestable: Vec<String> = {
            let state = self.state.read(cx);
            let (workspace, parent) = state.active_ws_key().zip(state.active.clone())?;
            let cache = state.ws(&workspace)?.subagent_directories.get(&parent)?;
            cache
                .items
                .iter()
                .filter(|a| crate::review::nested_agents::may_nest(&a.subagent_type))
                .map(|a| a.child_session_id.clone())
                .collect()
        };
        let mut nested = std::collections::HashMap::new();
        for sid in nestable {
            if let Some(rows) = self.nested_agent_rows(&sid, false, cx) {
                nested.insert(sid, rows);
            }
        }
        let state = self.state.read(cx);
        let (workspace, parent) = state.active_ws_key().zip(state.active.clone())?;
        let cache = state.ws(&workspace)?.subagent_directories.get(&parent)?;
        let loading = cache.loading;
        let error = cache.error.clone();
        let more = cache.next_cursor.is_some();
        let mut rows = div()
            .id("ended-agents-list")
            .flex()
            .flex_col()
            .gap_1()
            .max_h(px(200.))
            .overflow_y_scroll();
        for agent in &cache.items {
            let child = agent.child_session_id.clone();
            let workspace = workspace.clone();
            let parent = parent.clone();
            let title = agent.title.clone();
            let mut row = div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(title))
                        .children(
                            // The summary is the child's whole markdown report; the
                            // list shows one line of it and Open shows the rest.
                            summary_preview(agent.summary.as_deref(), 140).map(|summary| {
                                div()
                                    .truncate()
                                    .text_size(px(ui_size(10.)))
                                    .text_color(rgb(MUTED))
                                    .child(summary)
                            }),
                        ),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(ui_size(10.)))
                        .text_color(rgb(MUTED))
                        .child(agent.status.clone())
                        .children(agent.started_at.zip(agent.ended_at).map(|(start, end)| {
                            div().child(crate::conversation::turn_meta::format_duration(
                                end.saturating_sub(start),
                            ))
                        })),
                );
            let open = div().child(
                Button::new(
                    gpui::SharedString::from(format!("ended-open-{child}")),
                    label("Open", "打开"),
                )
                .size(ControlSize::Sm)
                .variant(ButtonVariant::Ghost)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_child_conversation(&workspace, &parent, &child, window, cx)
                })),
            );
            #[cfg(test)]
            let open = crate::app::test_support::track_children(
                open,
                vec![format!("ended-open-{}", agent.child_session_id)],
            );
            row = row.child(open);
            rows = rows.child(row);
            if let Some(children) = nested.remove(&agent.child_session_id) {
                rows = rows.child(div().pl(px(6.)).child(children));
            }
        }
        let empty = cache.items.is_empty();
        Some(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .px_2()
                .pb_2()
                .text_size(px(ui_size(12.)))
                .text_color(rgb(TEXT))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label("Ended agents", "已结束的子代理"))
                        .child(
                            Button::new("refresh-ended-agents", label("Refresh", "刷新"))
                                .variant(ButtonVariant::Ghost)
                                .size(ControlSize::Sm)
                                .disabled(loading)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.state.update(cx, |s, cx| {
                                        s.ensure_subagent_directory(true, false, cx)
                                    })
                                })),
                        ),
                )
                .children(loading.then(|| {
                    div()
                        .text_color(rgb(MUTED))
                        .child(label("Loading…", "加载中…"))
                }))
                .children(error.map(|error| div().text_color(rgb(MUTED)).child(error)))
                .children((empty && !loading).then(|| {
                    div()
                        .text_color(rgb(MUTED))
                        .child(label("No ended agents", "没有已结束的子代理"))
                }))
                .child(rows)
                .children(more.then(|| {
                    Button::new("more-ended-agents", label("Load more", "加载更多"))
                        .size(ControlSize::Sm)
                        .disabled(loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state
                                .update(cx, |s, cx| s.ensure_subagent_directory(false, true, cx))
                        }))
                        .into_any_element()
                }))
                .into_any_element(),
        )
    }
}
