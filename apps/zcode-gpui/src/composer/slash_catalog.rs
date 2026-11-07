use crate::app::store::AppState;
use crate::backend::inspection::QueryState;
use crate::composer::slash::{SlashCommand, filter_slash_commands};
use gpui::{Context, SharedString};
use serde_json::Value;

pub(crate) const SLASH_CATALOG_ERROR: &str =
    "Slash command catalog is unavailable; Refresh to retry";

pub(crate) fn parse_commands(value: &Value) -> Result<Vec<SlashCommand>, String> {
    let commands: Vec<SlashCommand> =
        serde_json::from_value(value.clone()).map_err(|_| SLASH_CATALOG_ERROR)?;
    let values = value.as_array().ok_or(SLASH_CATALOG_ERROR)?;
    if commands
        .iter()
        .any(|command| command.name.trim().is_empty())
        || values.iter().any(|command| {
            ["inputHint", "source"]
                .iter()
                .any(|key| command.get(key).is_some_and(Value::is_null))
        })
    {
        return Err(SLASH_CATALOG_ERROR.into());
    }
    Ok(commands)
}

#[derive(Clone)]
pub(crate) struct SlashCatalogReceipt {
    pub workspace: String,
    pub session: Option<String>,
    generation: u64,
    navigation: u64,
    revision: u64,
    replacement: u64,
    text: String,
}

impl AppState {
    pub(crate) fn active_slash_query(&self) -> Option<&QueryState<Vec<SlashCommand>>> {
        self.ws(&self.active_ws_key()?)?
            .slash_catalogs
            .get(&self.active)
    }

    pub(crate) fn active_slash_commands(&self) -> Option<&[SlashCommand]> {
        let query = self.active_slash_query()?;
        (!query.loading && query.error.is_none())
            .then_some(query.value.as_deref())
            .flatten()
    }

    pub(crate) fn slash_catalog_receipt(&self, cx: &gpui::App) -> Option<SlashCatalogReceipt> {
        if self.is_read_only_view() {
            return None;
        }
        let workspace = self.active_ws_key()?;
        let ws = self.ws(&workspace)?;
        if !ws.started || ws.inbound.is_none() {
            return None;
        }
        let composer = self.composer.read(cx);
        Some(SlashCatalogReceipt {
            workspace,
            session: self.active.clone(),
            generation: ws.generation,
            navigation: self.navigation_generation,
            revision: self.active_slash_query()?.revision,
            replacement: composer.replacement_generation,
            text: composer.text().into(),
        })
    }

    pub(crate) fn slash_receipt_current(
        &self,
        receipt: &SlashCatalogReceipt,
        cx: &gpui::App,
    ) -> bool {
        let composer = self.composer.read(cx);
        !self.is_read_only_view()
            && self.active_ws_key().as_deref() == Some(&receipt.workspace)
            && self.active == receipt.session
            && self.navigation_generation == receipt.navigation
            && self.ws(&receipt.workspace).is_some_and(|ws| {
                ws.started && ws.inbound.is_some() && ws.generation == receipt.generation
            })
            && self
                .active_slash_query()
                .is_some_and(|q| q.revision == receipt.revision)
            && composer.replacement_generation == receipt.replacement
            && composer.text() == receipt.text
            && composer.marked_utf16.is_none()
    }

    pub(crate) fn refresh_slash_catalog(
        &mut self,
        receipt: &SlashCatalogReceipt,
        cx: &mut Context<Self>,
    ) {
        if self.slash_receipt_current(receipt, cx) {
            self.ensure_slash_catalog(true, cx);
        }
    }

    pub(crate) fn apply_slash_suggestion(
        &mut self,
        receipt: &SlashCatalogReceipt,
        command: &SlashCommand,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.slash_receipt_current(receipt, cx)
            || !self.active_slash_commands().is_some_and(|commands| {
                filter_slash_commands(commands, &receipt.text)
                    .is_some_and(|matches| matches.contains(&command))
            })
        {
            return false;
        }
        self.composer.update(cx, |composer, cx| {
            composer.set_text(&format!("/{} ", command.name));
            cx.notify();
        });
        true
    }
}

impl crate::app::root::RootView {
    pub(crate) fn slash_catalog_feedback(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use ely_gpui_component::buttons::Button;
        use gpui::{IntoElement, ParentElement, Styled, div};
        let state = self.state.read(cx);
        filter_slash_commands(&[], state.composer.read(cx).text())?;
        let query = state.active_slash_query();
        let receipt = state.slash_catalog_receipt(cx);
        let label = crate::shared::i18n::label;
        let waking = state
            .active_ws_key()
            .is_some_and(|key| state.send_needs_wake(&key));
        let message = match query {
            Some(q) if q.loading => label("Loading commands…", "正在加载命令…").to_string(),
            None if waking => label("Loading commands…", "正在加载命令…").to_string(),
            Some(q) if q.error.is_some() => label(
                "Commands unavailable; Refresh to retry",
                "命令不可用，请刷新重试",
            )
            .to_string(),
            Some(q) if q.value.as_ref().is_some_and(Vec::is_empty) => {
                label("No available commands", "没有可用命令").to_string()
            }
            Some(q) if q.value.is_some() => label("Available commands", "可用命令").to_string(),
            _ => label(
                "Connect a workspace to load commands",
                "连接工作区以加载命令",
            )
            .to_string(),
        };
        let loading = query.is_some_and(|q| q.loading);
        let button = Button::new("refresh-slash-catalog", label("Refresh", "刷新"))
            .size(ely_gpui_component::theme::ControlSize::Sm)
            .disabled(loading || receipt.is_none())
            .on_click(cx.listener(move |this, _, _, cx| {
                let Some(receipt) = &receipt else { return };
                this.state.update(cx, |state, cx| {
                    state.refresh_slash_catalog(receipt, cx);
                });
            }));
        let wrapper = div().child(button);
        #[cfg(test)]
        let wrapper =
            crate::app::test_support::track_children(wrapper, vec!["refresh-slash-catalog".into()]);
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(gpui::px(crate::shared::theme::font_size_sm()))
                .child(div().flex_1().child(SharedString::from(message)))
                .child(wrapper)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "slash_catalog_tests.rs"]
pub(crate) mod tests;
