use crate::app::settings::SettingsSection;
use crate::app::settings_models::filtered_model_groups;
use crate::app::settings_navigation::{SettingsGroup, navigation_sections, settings_layout};
use crate::app::{root::RootView, store::AppState, test_support::*};
use crate::composer::catalog::{ModelOption, WorkspaceConfig};
use crate::shared::preferences::{PreferenceOwner, Preferences};
use crate::shared::settings::AppSettings;
use gpui::{MouseButton, TestApp, px, size};
use std::sync::Arc;

#[test]
fn grouped_navigation_is_complete_unique_and_searchable() {
    assert_eq!(
        SettingsSection::parse("subagents"),
        Some(SettingsSection::Subagents)
    );
    assert_eq!(SettingsSection::parse("not-a-section"), None);
    let sections = navigation_sections("");
    assert_eq!(sections.len(), 16);
    assert_eq!(
        sections[0],
        (SettingsGroup::Basics, SettingsSection::General)
    );
    assert_eq!(
        sections[2],
        (SettingsGroup::Basics, SettingsSection::Models)
    );
    assert_eq!(
        sections[5],
        (SettingsGroup::AgentCapabilities, SettingsSection::Subagents)
    );
    assert_eq!(
        sections[8],
        (SettingsGroup::AgentCapabilities, SettingsSection::Browser)
    );
    assert_eq!(
        sections[9],
        (SettingsGroup::AgentCapabilities, SettingsSection::Voice)
    );
    assert_eq!(
        sections[10],
        (
            SettingsGroup::AgentCapabilities,
            SettingsSection::SystemPrompt
        )
    );
    assert_eq!(
        sections[11],
        (SettingsGroup::AgentCapabilities, SettingsSection::Secrets)
    );
    assert_eq!(
        sections[12],
        (
            SettingsGroup::AgentCapabilities,
            SettingsSection::CodeSearch
        )
    );
    assert_eq!(
        sections[13],
        (
            SettingsGroup::AgentCapabilities,
            SettingsSection::Diagnostics
        )
    );
    assert_eq!(
        sections[14],
        (SettingsGroup::DataAndStats, SettingsSection::Usage)
    );
    assert_eq!(
        sections[15],
        (SettingsGroup::DataAndStats, SettingsSection::Phone)
    );
    for (_, section) in &sections {
        assert_eq!(
            sections
                .iter()
                .filter(|(_, value)| value == section)
                .count(),
            1
        );
    }
    assert_eq!(
        navigation_sections("  model SETTINGS  "),
        vec![(SettingsGroup::Basics, SettingsSection::Models,)]
    );
    assert_eq!(
        navigation_sections("子智能体")[0].1,
        SettingsSection::Subagents
    );
    assert_eq!(navigation_sections("语音")[0].1, SettingsSection::Voice);
    assert!(navigation_sections("missing section").is_empty());
    assert!(!settings_layout(1280.).compact);
    assert!(settings_layout(700.).compact);
    assert_eq!(settings_layout(1280.).navigation_width, 268.);
    assert_eq!(settings_layout(700.).navigation_width, 68.);
}

pub(super) fn model(provider: &str, name: &str, reasoning: &str) -> ModelOption {
    ModelOption {
        value: format!("{provider}/{name}"),
        name: name.into(),
        provider_name: "Same display name".into(),
        thought_levels: vec![reasoning.into()],
        default_thought: reasoning.into(),
        provider: provider.into(),
        model: name.into(),
    }
}

#[test]
fn models_filter_is_an_inspection_of_the_existing_catalog() {
    let config = WorkspaceConfig {
        models: vec![
            model("first", "Alpha", "high"),
            model("second", "Beta", "low"),
            model("first", "Gamma", "medium"),
        ],
        ..Default::default()
    };
    let before = format!("{config:?}");
    let all = filtered_model_groups(&config.models, "");
    assert_eq!(all.len(), 2); // 同名 provider 标签不能合并不同 owner 的模型。
    assert_eq!(all[0].items.len(), 2);
    assert_eq!(
        filtered_model_groups(&config.models, "FIRST high")[0].items[0].name,
        "Alpha"
    );
    assert_eq!(
        filtered_model_groups(&config.models, " beta ")[0].items[0].provider,
        "second"
    );
    assert!(filtered_model_groups(&config.models, "unknown").is_empty());
    assert_eq!(format!("{config:?}"), before);
}

fn app(path: std::path::PathBuf) -> TestApp {
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        Preferences::install_at(AppSettings::default(), path, Arc::new(|| false), cx);
    });
    app
}

#[test]
fn direct_section_and_measured_back_preserve_draft_and_per_window_selection() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-settings-nav-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    app.update_entity(&state, |s, cx| {
        s.active_workspace = Some(s.workspaces[0].key.clone());
        s.active = Some("parent".into());
        s.ui_model_value = Some("first/Alpha".into());
        s.composer.update(cx, |c, _| c.set_text("unsent draft"));
        s.workspace_configs.insert(
            s.workspaces[0].key.clone(),
            WorkspaceConfig {
                models: vec![model("first", "Alpha", "high")],
                ..Default::default()
            },
        );
    });
    let mut first = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    first.simulate_resize(size(px(1280.), px(820.)));
    first.update(|view, window, cx| {
        view.open_settings_section(SettingsSection::Models, window, cx);
        view.open_settings_section(SettingsSection::Models, window, cx);
        view.settings.model_query = "high".into();
    });
    first.draw();
    let back = app.read(|cx| cx.global::<TestTargets>().0["settings-back"].center());
    first.simulate_click(back, MouseButton::Left);
    first.read(|view, cx| {
        assert!(!view.settings.open);
        assert_eq!(view.settings.section, SettingsSection::Models);
        assert_eq!(view.settings.model_query, "high");
        assert_eq!(view.state.read(cx).active.as_deref(), Some("parent"));
        assert_eq!(
            view.state.read(cx).ui_model_value.as_deref(),
            Some("first/Alpha")
        );
        assert_eq!(view.state.read(cx).composer.read(cx).text(), "unsent draft");
    });
    first.update(|view, window, cx| view.open_settings(window, cx));
    first.read(|view, _| assert_eq!(view.settings.section, SettingsSection::Models));
    let other_state = app.new_entity(AppState::for_test);
    let second = app.open_window(|_, cx| RootView::new(other_state.clone(), cx));
    second.read(|view, _| {
        assert_eq!(view.settings.section, SettingsSection::General);
        assert!(view.settings.model_query.is_empty());
    });
    first.simulate_keystroke("escape");
    first.update(|view, window, cx| {
        assert!(!view.settings.open);
        assert!(
            view.state
                .read(cx)
                .composer
                .read(cx)
                .focus
                .is_focused(window)
        );
    });
    app.read(|cx| {
        assert!(
            cx.global::<PreferenceOwner>()
                .0
                .read(cx)
                .snapshot
                .shortcut_bindings
                .is_none()
        );
    });
    assert!(!dir.exists());
}

#[test]
fn measured_search_and_narrow_navigation_keep_models_read_only() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-settings-search-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    app.update_entity(&state, |s, _| {
        s.active_workspace = Some(s.workspaces[0].key.clone());
        s.ui_model_value = Some("first/Alpha".into());
        s.workspace_configs.insert(
            s.workspaces[0].key.clone(),
            WorkspaceConfig {
                models: vec![
                    model("first", "Alpha", "high"),
                    model("second", "Beta", "low"),
                ],
                ..Default::default()
            },
        );
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(700.), px(600.)));
    window.update(|view, window, cx| view.open_settings(window, cx));
    window.draw();
    let search = app.read(|cx| cx.global::<TestTargets>().0["settings-search"].center());
    window.simulate_click(search, MouseButton::Left);
    window.simulate_input("model");
    window.draw();
    let models = app.read(|cx| cx.global::<TestTargets>().0["settings-models"].center());
    assert!(models.y < px(600.));
    window.simulate_click(models, MouseButton::Left);
    window.draw();
    let search = app.read(|cx| cx.global::<TestTargets>().0["settings-model-search"].center());
    window.simulate_click(search, MouseButton::Left);
    window.simulate_input("beta");
    window.read(|view, cx| {
        assert_eq!(view.settings.query, "model");
        assert_eq!(view.settings.model_query, "beta");
        let state = view.state.read(cx);
        assert_eq!(state.ui_model_value.as_deref(), Some("first/Alpha"));
        assert_eq!(
            state.workspace_configs[&state.workspaces[0].key]
                .models
                .len(),
            2
        );
        assert!(state.workspaces[0].pending.is_empty());
        assert!(state.composer.read(cx).text().is_empty());
    });
    assert!(!dir.exists());
}

#[test]
fn voice_section_renders_on_desktop_and_is_hidden_on_the_phone() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-settings-voice-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window
        .update(|view, window, cx| view.open_settings_section(SettingsSection::Voice, window, cx));
    window.draw();
    let targets = |app: &mut TestApp| -> Vec<String> {
        app.read(|cx| cx.global::<TestTargets>().0.keys().cloned().collect())
    };
    let desktop = targets(&mut app);
    assert!(desktop.contains(&"settings-voice".to_string()));
    assert!(desktop.contains(&"settings-voice-engine".to_string()));
    app.update(|cx| {
        cx.set_global(TestTargets::default());
        state.update(cx, |s, _| s.phone_client.enabled = true);
    });
    window.update(|view, window, cx| view.open_settings(window, cx));
    window.draw();
    let phone = targets(&mut app);
    assert!(phone.contains(&"settings-general".to_string()));
    assert!(!phone.contains(&"settings-voice".to_string()));
    assert!(!phone.contains(&"settings-browser".to_string()));
    assert!(!dir.exists());
}

#[test]
fn prompt_cache_row_shows_only_on_desktop_anthropic_provider_cards() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("gpui-settings-cache-{}", uuid::Uuid::now_v7()));
    let mut app = app(dir.join("setting.json"));
    let state = app.new_entity(AppState::for_test);
    let set_format = |app: &mut TestApp, format: &str| {
        let mut view = crate::provider_settings::tests::sample_view();
        view["providers"][0]["effectiveConfig"]["api"]["type"] = format.into();
        app.update_entity(&state, |s, _| {
            s.provider_settings.view = crate::provider_settings::ProviderView::from_value(&view);
            s.provider_settings.selected = Some("c1".into());
            s.provider_settings.loading = false;
        });
    };
    set_format(&mut app, "anthropic-messages");
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    let shown = |app: &mut TestApp| -> bool {
        app.read(|cx| {
            cx.global::<TestTargets>()
                .0
                .contains_key("provider-prompt-cache")
        })
    };
    let redraw = |app: &mut TestApp, window: &mut gpui::TestAppWindow<RootView>| {
        app.update(|cx| cx.set_global(TestTargets::default()));
        window.draw();
    };
    window
        .update(|view, window, cx| view.open_settings_section(SettingsSection::Models, window, cx));
    set_format(&mut app, "anthropic-messages");
    redraw(&mut app, &mut window);
    assert!(shown(&mut app), "Anthropic Messages provider shows the row");

    // OpenAI formats never receive Anthropic cache settings.
    set_format(&mut app, "openai-chat-completions");
    redraw(&mut app, &mut window);
    assert!(!shown(&mut app));

    set_format(&mut app, "anthropic-messages");
    app.update_entity(&state, |s, _| s.phone_client.enabled = true);
    redraw(&mut app, &mut window);
    assert!(!shown(&mut app), "the phone has no agents of its own");
    assert!(!dir.exists());
}
