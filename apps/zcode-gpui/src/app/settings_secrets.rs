//! Settings › Secrets (GPUI only, SECRETS.md). Values go straight to the OS
//! secret store and are never shown again; the page lists names, whether the
//! OS store still has a value, and which workspaces get each secret.

use crate::app::root::RootView;
use crate::app::settings_rows::settings_card;
use crate::backend::workspace::WorkspacePurpose;
use crate::secrets::MIN_REDACTED_LEN;
use crate::shared::i18n::label;
use crate::shared::theme::active_theme;
use ely_gpui_component::buttons::{Button, ButtonVariant};
use ely_gpui_component::forms::{Input, InputEvent, TextInput};
use ely_gpui_component::theme::ControlSize;
use gpui::{
    AnyElement, AppContext, Context, Entity, Focusable, IntoElement, Window, div, prelude::*,
};

pub(crate) struct SecretForm {
    pub name: Entity<TextInput>,
    pub value: Entity<TextInput>,
    /// Feedback for the last save (e.g. the short-value warning).
    pub notice: Option<&'static str>,
}

impl RootView {
    fn ensure_secret_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.secrets_form.is_some() {
            return;
        }
        let name = cx.new(|cx| TextInput::new(window, cx).placeholder("STEAM_TOKEN"));
        let value = cx.new(|cx| {
            TextInput::new(window, cx)
                .placeholder(label("Value", "值"))
                .masked()
        });
        for input in [&name, &value] {
            cx.subscribe(
                input,
                |this: &mut RootView, _, event: &InputEvent, cx| match event {
                    InputEvent::Submit => this.save_secret_form(cx),
                    InputEvent::Changed => cx.notify(),
                    _ => {}
                },
            )
            .detach();
        }
        self.settings.secrets_form = Some(SecretForm {
            name,
            value,
            notice: None,
        });
    }

    pub(crate) fn save_secret_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.settings.secrets_form.as_ref() else {
            return;
        };
        let name = form.name.read(cx).text().trim().to_owned();
        let value = form.value.read(cx).text().to_owned();
        let (name_input, value_input) = (form.name.clone(), form.value.clone());
        // Replacing keeps the secret's scope; a new one starts global.
        let scope = self
            .state
            .read(cx)
            .secrets
            .entries
            .iter()
            .find(|meta| meta.name == name)
            .map(|meta| meta.workspaces.clone())
            .unwrap_or_default();
        let saved = self
            .state
            .update(cx, |s, cx| s.save_secret(&name, &value, scope, cx));
        if saved.is_ok() {
            // The value leaves the UI as soon as the OS store has it.
            value_input.update(cx, |input, cx| input.set_text("", cx));
            name_input.update(cx, |input, cx| input.set_text("", cx));
        }
        if let Some(form) = self.settings.secrets_form.as_mut() {
            form.notice = (saved.is_ok() && value.chars().count() < MIN_REDACTED_LEN).then(|| {
                label(
                    "Saved. Values shorter than 6 characters are not masked in tool output.",
                    "已保存。短于 6 个字符的值不会在工具输出中被遮蔽。",
                )
            });
        }
        cx.notify();
    }

    pub(crate) fn secrets_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_secret_form(window, cx);
        let theme = active_theme();
        let state = self.state.read(cx);
        let vault = &state.secrets;
        let projects: Vec<(String, String)> = state
            .workspaces
            .iter()
            .filter(|ws| ws.purpose == WorkspacePurpose::Project)
            .map(|ws| (ws.key.clone(), ws.display.clone()))
            .collect();
        let unavailable = state
            .active_ws_key()
            .is_some_and(|key| vault.unavailable.contains(&key));
        let listed: Vec<(String, bool, Vec<String>)> = vault
            .entries
            .iter()
            .map(|meta| {
                let stored = !vault.loaded || vault.has_value(&meta.name);
                (meta.name.clone(), stored, meta.workspaces.clone())
            })
            .collect();
        let (error, loaded) = (vault.error.clone(), vault.loaded);
        let rows: Vec<AnyElement> = listed
            .iter()
            .map(|(name, stored, scope)| secret_row(name, *stored, scope, &projects, cx))
            .collect();
        let form = self.settings.secrets_form.as_ref().unwrap();
        let can_save = loaded
            && !form.name.read(cx).text().trim().is_empty()
            && !form.value.read(cx).text().is_empty();
        let notice = form.notice;
        let save = Button::new(
            "settings-secrets-save",
            label("Save to OS store", "保存到系统密钥库"),
        )
        .disabled(!can_save)
        .on_click(cx.listener(|this, _, _, cx| this.save_secret_form(cx)));
        let add = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(div().w(gpui::px(220.)).child(Input::new(&form.name)))
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::px(200.))
                    .child(Input::new(&form.value)),
            )
            .child(save);
        #[cfg(test)]
        let add = crate::app::test_support::track_children(
            add,
            vec![
                "settings-secrets-name".into(),
                "settings-secrets-value".into(),
                "settings-secrets-save".into(),
            ],
        );
        let status = if !loaded {
            Some(label("Reading the OS secret store…", "正在读取系统密钥库…"))
        } else if unavailable {
            Some(label(
                "This workspace's agent backend doesn't support secrets. Run GPUI with its own backend (bun source or zcode-backend).",
                "当前工作区的智能体后端不支持密钥。请使用 GPUI 自带的后端（bun 源码或 zcode-backend）。",
            ))
        } else {
            None
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_color(theme.subtle).child(label(
                "Secrets are kept in your OS secret store (Windows Credential Manager, macOS Keychain, Linux Secret Service), not in files. Agents get them as environment variables in Bash and stdio MCP servers, and every value is replaced with *** in tool output before the model sees it. Any program running as you can still read the OS store, including commands the agent runs.",
                "密钥保存在系统密钥库（Windows 凭据管理器、macOS 钥匙串、Linux Secret Service），不写入文件。智能体在 Bash 与 stdio MCP 服务器中以环境变量获得它们；模型看到工具输出前，每个值都会被替换为 ***。以你身份运行的任何程序（包括智能体执行的命令）仍可读取系统密钥库。",
            )))
            .child(add)
            .children(notice.map(|n| div().text_color(theme.subtle).child(n)))
            .children(status.map(|s| div().text_color(theme.subtle).child(s)))
            .children(error.map(|e| div().text_color(theme.destructive).child(e)))
            .child(if rows.is_empty() {
                div()
                    .text_color(theme.subtle)
                    .child(label("No secrets yet.", "还没有密钥。"))
                    .into_any_element()
            } else {
                settings_card(rows).into_any_element()
            })
            .into_any_element()
    }
}

fn secret_row(
    name: &str,
    stored: bool,
    scope: &[String],
    projects: &[(String, String)],
    cx: &mut Context<RootView>,
) -> AnyElement {
    let theme = active_theme();
    let status = if stored {
        label("Stored", "已保存")
    } else {
        label(
            "Missing from the OS store — not injected",
            "系统密钥库中缺失，不会注入",
        )
    };
    let scope_text = if scope.is_empty() {
        label("All workspaces", "所有工作区")
    } else {
        label("Selected workspaces", "所选工作区")
    };
    let chips = projects.iter().map(|(key, display)| {
        let selected = scope.iter().any(|k| k == key);
        let (name, key) = (name.to_owned(), key.clone());
        Button::new(
            gpui::SharedString::from(format!("settings-secret-scope-{name}-{key}")),
            display.clone(),
        )
        .size(ControlSize::Sm)
        .variant(if selected {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Outline
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.state
                .update(cx, |s, cx| s.toggle_secret_workspace(&name, &key, cx));
        }))
    });
    let delete_name = name.to_owned();
    let delete = Button::new(
        gpui::SharedString::from(format!("settings-secret-delete-{name}")),
        label("Delete", "删除"),
    )
    .size(ControlSize::Sm)
    .variant(ButtonVariant::Danger)
    .on_click(cx.listener(move |this, _, _, cx| {
        this.state
            .update(cx, |s, cx| s.delete_secret(&delete_name, cx));
    }));
    let replace_name = name.to_owned();
    let replace = Button::new(
        gpui::SharedString::from(format!("settings-secret-replace-{name}")),
        label("Replace value", "替换值"),
    )
    .size(ControlSize::Sm)
    .variant(ButtonVariant::Ghost)
    .on_click(cx.listener(move |this, _, window, cx| {
        if let Some(form) = this.settings.secrets_form.as_ref() {
            let (name_input, value_input) = (form.name.clone(), form.value.clone());
            name_input.update(cx, |input, cx| input.set_text(replace_name.clone(), cx));
            let handle = value_input.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
    }));
    div()
        .flex()
        .flex_col()
        .gap_2()
        .px_4()
        .py_3()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(name.to_owned()),
                )
                .child(replace)
                .child(delete),
        )
        .child(
            div()
                .text_color(if stored {
                    theme.subtle
                } else {
                    theme.destructive
                })
                .child(format!("{status} · {scope_text}")),
        )
        .child(div().flex().flex_wrap().gap_1().children(chips))
        .into_any_element()
}

#[cfg(test)]
#[path = "settings_secrets_tests.rs"]
mod tests;
