use crate::app::{
    root::RootView,
    settings_navigation::{SettingsSearch, search_matches},
};
use crate::shared::i18n::label;
use crate::shared::preferences::{PreferenceChange, PreferenceOwner, Preferences};
use crate::shared::settings::AppSettings;
use crate::shared::shortcut_runtime::{SUPPORTED, bindings};
use crate::shared::shortcuts::ShortcutCommandId as Command;
use ely_gpui_component::buttons::{Button, ButtonVariant};
use ely_gpui_component::forms::HotkeyInput;
use gpui::{
    AnyElement, App, Context, Entity, IntoElement, Keystroke, OperatingSystem, Window, div,
    prelude::*,
};

fn binding_stroke(binding: &str) -> Option<Keystroke> {
    let binding = binding
        .replace(
            "CmdOrCtrl",
            if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            },
        )
        .replace("Ctrl", "ctrl")
        .replace("Cmd", "cmd")
        .replace("Shift", "shift")
        .replace("Alt", "alt")
        .replace('+', "-");
    Keystroke::parse_with_os(&binding, host_os()).ok()
}

/// gpui needs the host OS to parse dynamic keystrokes and to label them.
fn host_os() -> OperatingSystem {
    if cfg!(target_os = "macos") {
        OperatingSystem::Mac
    } else if cfg!(target_os = "windows") {
        OperatingSystem::Windows
    } else if cfg!(target_os = "android") {
        OperatingSystem::Android
    } else {
        OperatingSystem::Linux
    }
}

fn stroke_binding(stroke: &Keystroke) -> String {
    let mut parts = Vec::new();
    if stroke.modifiers.control {
        parts.push("Ctrl".to_owned());
    }
    if stroke.modifiers.platform {
        parts.push("Cmd".to_owned());
    }
    if stroke.modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if stroke.modifiers.shift {
        parts.push("Shift".to_owned());
    }
    parts.push(stroke.key.clone());
    parts.join("+")
}

pub(crate) fn edit_binding(
    values: &[String],
    index: usize,
    value: Option<String>,
) -> Option<Vec<String>> {
    let mut values = values.to_vec();
    match (index.cmp(&values.len()), value) {
        (std::cmp::Ordering::Less, Some(value)) => values[index] = value,
        (std::cmp::Ordering::Equal, Some(value)) => values.push(value),
        (std::cmp::Ordering::Less, None) => {
            values.remove(index);
        }
        _ => return None,
    }
    Some(values)
}

pub(crate) fn shortcut_matches(command: Command, query: &str) -> bool {
    // 中英文标签和共享 command ID 都可检索；不把未实现的 command 暴露成可配置项。
    let labels = match command {
        Command::OpenSettings => "Settings 设置",
        Command::OpenCommandCenter => "Command center 命令中心",
        Command::ToggleTerminal => "Toggle terminal 切换终端",
        Command::ToggleSidePane => "Toggle tool dock 切换工具面板",
        Command::NewTask => "New task 新任务",
        Command::OpenWorkspace => "Open workspace 打开工作区",
        _ => "",
    };
    search_matches(query, &format!("{labels} {}", command.as_str()))
}

fn editable(cx: &App) -> bool {
    let owner = cx.global::<PreferenceOwner>().0.read(cx);
    !owner.saving && !owner.read_only && !owner.suspended
}

fn submit_binding(
    command: Command,
    index: usize,
    expected: Option<&str>,
    value: Option<String>,
    cx: &mut App,
) {
    if !editable(cx) {
        return;
    }
    let owner = cx.global::<PreferenceOwner>().0.read(cx);
    let current = bindings(command, owner.snapshot.shortcut_bindings.as_ref());
    // 录制期间 owner 可能已提交新值；复验原位置，避免旧回调覆盖另一个 binding。
    if current.get(index).map(String::as_str) != expected {
        return;
    }
    if let Some(values) = edit_binding(&current, index, value) {
        Preferences::enqueue(
            PreferenceChange::Shortcut(command.as_str().into(), Some(values)),
            cx,
        );
    }
}

fn recorder(
    id: String,
    command: Command,
    index: usize,
    value: Option<String>,
    blocked: bool,
    adding: Option<Entity<bool>>,
) -> AnyElement {
    let stroke = value.as_deref().and_then(binding_stroke);
    if blocked {
        // Ely recorder 没有 disabled API；禁用态使用同库 Button，完全移除可录制的焦点目标。
        return Button::new(
            id,
            stroke.map_or_else(
                || label("Disabled", "已禁用").into(),
                |s| s.unparse(host_os()),
            ),
        )
        .disabled(true)
        .into_any_element();
    }
    HotkeyInput::new(id, stroke)
        .on_change(move |stroke, _, cx| {
            submit_binding(
                command,
                index,
                value.as_deref(),
                stroke.as_ref().map(stroke_binding),
                cx,
            );
            if let Some(adding) = &adding {
                adding.update(cx, |adding, cx| {
                    *adding = false;
                    cx.notify();
                });
            }
        })
        .into_any_element()
}

impl RootView {
    pub(crate) fn render_shortcut_settings(
        &mut self,
        settings: &AppSettings,
        blocked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search = self.settings_search(SettingsSearch::Shortcuts, window, cx);
        let mut list = div().flex().flex_col().gap_4().child(search)
            .child(label("Record a binding; Backspace removes it. Reset restores defaults; Disable removes all bindings.",
                "录制一个快捷键；退格删除该绑定。重置恢复默认值；禁用移除所有绑定。"));
        let commands: Vec<_> = SUPPORTED
            .iter()
            .copied()
            .filter(|c| shortcut_matches(*c, &self.settings.shortcut_query))
            .collect();
        let no_matches = commands.is_empty();
        for command in commands {
            let id = command.as_str();
            let values = bindings(command, settings.shortcut_bindings.as_ref());
            let adding = window.use_keyed_state(format!("shortcut-adding-{id}"), cx, |_, _| false);
            let add_state = adding.clone();
            let actions = div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new(format!("add-{id}"), label("Add binding", "添加绑定"))
                        .variant(ButtonVariant::Ghost)
                        .disabled(blocked)
                        .on_click(move |_, _, cx| {
                            if editable(cx) {
                                add_state.update(cx, |v, cx| {
                                    *v = true;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    Button::new(format!("disable-{id}"), label("Disable", "禁用"))
                        .variant(ButtonVariant::Ghost)
                        .disabled(blocked)
                        .on_click(move |_, _, cx| {
                            if editable(cx) {
                                Preferences::enqueue(
                                    PreferenceChange::Shortcut(id.into(), Some(vec![])),
                                    cx,
                                );
                            }
                        }),
                )
                .child(
                    Button::new(format!("reset-{id}"), label("Reset", "重置"))
                        .variant(ButtonVariant::Ghost)
                        .disabled(blocked)
                        .on_click(move |_, _, cx| {
                            if editable(cx) {
                                Preferences::enqueue(
                                    PreferenceChange::Shortcut(id.into(), None),
                                    cx,
                                );
                            }
                        }),
                );
            #[cfg(test)]
            let actions = crate::app::test_support::track_children(
                actions,
                vec![
                    format!("add-{id}"),
                    format!("disable-{id}"),
                    format!("reset-{id}"),
                ],
            );
            let mut rows = div().flex().flex_col().gap_2();
            for index in 0..values.len().max(1) {
                let value = values.get(index).cloned();
                let expected = value.clone();
                let recorder_id = if index == 0 {
                    format!("shortcut-{id}")
                } else {
                    format!("shortcut-{id}-{index}")
                };
                let row = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(recorder(
                        recorder_id.clone(),
                        command,
                        index,
                        value,
                        blocked,
                        None,
                    ))
                    .child(
                        Button::new(format!("remove-{id}-{index}"), label("Remove", "移除"))
                            .variant(ButtonVariant::Ghost)
                            .disabled(blocked || values.is_empty())
                            .on_click(move |_, _, cx| {
                                submit_binding(command, index, expected.as_deref(), None, cx)
                            }),
                    );
                #[cfg(test)]
                let row = crate::app::test_support::track_children(
                    row,
                    vec![recorder_id, format!("remove-{id}-{index}")],
                );
                rows = rows.child(row);
            }
            if *adding.read(cx) && !blocked {
                let cancel = adding.clone();
                let add_id = format!("shortcut-add-{id}");
                let row = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(recorder(
                        add_id.clone(),
                        command,
                        values.len(),
                        None,
                        false,
                        Some(adding.clone()),
                    ))
                    .child(
                        Button::new(format!("cancel-add-{id}"), label("Cancel", "取消"))
                            .variant(ButtonVariant::Ghost)
                            .on_click(move |_, _, cx| {
                                cancel.update(cx, |v, cx| {
                                    *v = false;
                                    cx.notify();
                                })
                            }),
                    );
                #[cfg(test)]
                let row = crate::app::test_support::track_children(
                    row,
                    vec![add_id, format!("cancel-add-{id}")],
                );
                rows = rows.child(row);
            }
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(crate::shared::shortcut_runtime::label(command))
                    .child(actions)
                    .child(rows),
            );
        }
        list.when(no_matches, |el| {
            el.child(label("No matching shortcuts", "没有匹配的快捷键"))
        })
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_conversion_roundtrips() {
        let stroke = binding_stroke("Ctrl+Shift+y").unwrap();
        assert_eq!(stroke_binding(&stroke), "Ctrl+Shift+y");
    }
}
