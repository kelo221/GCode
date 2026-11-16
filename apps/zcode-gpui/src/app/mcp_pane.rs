//! Settings › MCP Servers, laid out like desktop `McpSettingsSection` /
//! `McpServerList` (SETTINGS.md "MCP Servers page").
//!
//! Spec source: packages/shared/src/zcode-protocol/index.ts (zcodeMcpListResultSchema).

use crate::app::root::RootView;
use crate::app::subagents_rows::{canonical_plugin_name, group_header, half_border};
use crate::shared::i18n::{label, t, t_fmt};
use crate::shared::mcp::{McpDisplayStatus, McpServerSnapshot, group_servers};
use crate::shared::theme::{active_theme, ui_size};
use ely_gpui_component::{
    buttons::{ButtonVariant, IconButton},
    primitives::{IconName, Tooltip},
    theme::ControlSize,
};
use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};

impl RootView {
    pub(crate) fn mcp_pane(
        &mut self,
        settings: &crate::shared::settings::AppSettings,
        blocked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::app::inspection_view::InspectionKind;
        self.ensure_inspection(InspectionKind::Mcp, cx);
        let feedback = self.inspection_feedback(InspectionKind::Mcp, cx);
        let (servers, loading) = self
            .state
            .read(cx)
            .active_inspection()
            .map(|i| (i.mcp.value.clone().unwrap_or_default(), i.mcp.loading))
            .unwrap_or_default();
        let (installed, plugins) = group_servers(&servers);

        let refresh = div().child(
            IconButton::new("mcp-refresh", IconName::RefreshCw)
                .size(ControlSize::Md)
                .variant(ButtonVariant::Ghost)
                .tooltip(label("Refresh", "刷新"))
                .disabled(loading)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.state
                        .update(cx, |state, cx| state.fetch_mcp_servers(cx));
                })),
        );
        #[cfg(test)]
        let refresh = crate::app::test_support::track_children(refresh, vec!["mcp-refresh".into()]);

        let installed_title = t("settings.plugin.mcp.installed");
        let installed_group = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(group_header(installed_title, installed.len()))
                    .child(refresh),
            )
            .child(if installed.is_empty() {
                empty_card()
            } else {
                self.mcp_rows(&installed, false, cx)
            });

        let mut page = div()
            .id("mcp-page")
            .flex()
            .flex_col()
            .gap_8()
            .min_w_0()
            .child(feedback)
            .child(crate::app::settings_tool_search::tool_search_card(
                settings, blocked,
            ))
            .child(crate::app::settings_codemode::codemode_card(
                settings, blocked,
            ))
            .child(installed_group);
        for (plugin, rows) in plugins {
            page = page.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(group_header(&canonical_plugin_name(&plugin), rows.len()))
                    .child(self.mcp_rows(&rows, true, cx)),
            );
        }
        page.into_any_element()
    }

    fn mcp_rows(
        &self,
        servers: &[McpServerSnapshot],
        plugin: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let mut list = div()
            .flex()
            .flex_col()
            .min_w_0()
            .rounded_xl()
            .bg(theme.surface)
            .overflow_hidden();
        for (index, server) in servers.iter().enumerate() {
            if index != 0 {
                list = list.child(div().h(px(1.)).bg(half_border()));
            }
            list = list.child(self.mcp_row(server, plugin, cx));
        }
        list.into_any_element()
    }

    fn mcp_row(
        &self,
        server: &McpServerSnapshot,
        plugin: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = active_theme();
        let status = server.display_status();
        let name = match server.plugin_parts() {
            Some((_, name)) if plugin => name,
            _ => server.name.as_str(),
        };
        let reason = t(&server.status_reason_key()).to_owned();
        let failed = status == McpDisplayStatus::Error;
        let detail = if plugin {
            // Desktop PluginMcpServerList: a status sentence replaces the config line.
            if server.authorization.is_some() {
                t("settings.mcp.plugin.authorizationRequiredDescription").to_owned()
            } else {
                match status {
                    McpDisplayStatus::Connected => t("settings.mcp.plugin.connectedDescription"),
                    McpDisplayStatus::Connecting => t("settings.mcp.plugin.connectingDescription"),
                    McpDisplayStatus::Disconnected => {
                        t("settings.mcp.plugin.disconnectedDescription")
                    }
                    _ => t("settings.mcp.plugin.unavailableDescription"),
                }
                .to_owned()
            }
        } else {
            match status {
                McpDisplayStatus::Connected => format!(
                    "{} · {}",
                    server.transport,
                    t_fmt(
                        "settings.mcp.status.toolCount",
                        &[("count", &server.tool_count.to_string())]
                    )
                ),
                _ if server.status == "disabled" => {
                    format!("{} · {}", server.transport, label("Disabled", "已禁用"))
                }
                _ => server.transport.clone(),
            }
        };
        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .child(name.to_owned()),
            )
            .when(!(plugin && failed), |el| {
                el.child(
                    div()
                        .min_w_0()
                        .mt(px(2.))
                        .truncate()
                        .text_size(px(ui_size(12.)))
                        .text_color(theme.subtle)
                        .child(detail),
                )
            })
            .when(failed, |el| {
                el.child(
                    div()
                        .min_w_0()
                        .mt(px(2.))
                        .text_size(px(ui_size(12.)))
                        .text_color(theme.destructive)
                        .child(t(&server.failure_key()).to_owned()),
                )
            });
        div()
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_3()
            .min_w_0()
            .text_size(px(ui_size(14.)))
            .child(status_tile(server, reason))
            .child(text)
            .children(
                self.mcp_authorization_button(&server.name, cx)
                    .map(|button| div().flex_shrink_0().child(button)),
            )
            .into_any_element()
    }
}

/// Desktop `McpStatusDot` colors.
fn status_color(server: &McpServerSnapshot) -> gpui::Rgba {
    let theme = active_theme();
    match server.display_status() {
        McpDisplayStatus::Connected => theme.success,
        McpDisplayStatus::Connecting => theme.warning,
        McpDisplayStatus::Error => theme.destructive,
        _ if server.authorization.is_some() => theme.warning,
        _ => theme.subtlest,
    }
}

/// Server icon tile with the status dot in its corner; the tooltip says why.
fn status_tile(server: &McpServerSnapshot, reason: String) -> impl IntoElement {
    let theme = active_theme();
    div()
        .id(gpui::SharedString::from(format!(
            "mcp-status-{}",
            server.name
        )))
        .relative()
        .flex_none()
        .size(px(36.))
        .rounded_xl()
        .bg(theme.background)
        .flex()
        .items_center()
        .justify_center()
        .tooltip(Tooltip::text(reason))
        .child(
            gpui::svg()
                .path(IconName::Server.path())
                .size(px(16.))
                .text_color(theme.subtle),
        )
        .child(
            div()
                .absolute()
                .bottom(px(-4.))
                .right(px(-4.))
                .size(px(14.))
                .rounded_full()
                .bg(theme.card)
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(8.)).rounded_full().bg(status_color(server))),
        )
}

fn empty_card() -> AnyElement {
    let theme = active_theme();
    div()
        .id("mcp-empty")
        .rounded_xl()
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .px_4()
        .py_10()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .text_size(px(ui_size(14.)))
        .child(
            div()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(t("settings.mcp.emptyTitle").to_owned()),
        )
        .child(
            div()
                .text_color(theme.subtle)
                .child(t("settings.mcp.emptyDescription").to_owned()),
        )
        .into_any_element()
}

#[cfg(test)]
#[path = "mcp_pane_tests.rs"]
mod tests;
