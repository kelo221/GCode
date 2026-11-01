use crate::app::root::RootView;
use crate::app::settings::SettingsSection;
use crate::app::store::AppState;
use crate::app::test_support::{RUNTIME_TEST_LOCK, TestTargets};
use crate::secrets::SecretVault;
use crate::secrets::store::MemoryStore;
use gpui::{MouseButton, TestApp, px, size};
use std::sync::Arc;

fn target(app: &TestApp, id: &str) -> gpui::Point<gpui::Pixels> {
    app.read(|cx| cx.global::<TestTargets>().0[id].center())
}

#[test]
fn saving_stores_the_value_clears_the_form_and_warns_about_short_values() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        let path = std::env::temp_dir().join(format!("gpui-secrets-{}.json", uuid::Uuid::now_v7()));
        crate::shared::preferences::Preferences::install_at(
            crate::shared::settings::load_settings_from_path(&path),
            path,
            Arc::new(|| false),
            cx,
        );
    });
    let store = Arc::new(MemoryStore::default());
    let state = app.new_entity(AppState::for_test);
    app.update_entity(&state, |s, _| {
        let index = std::env::temp_dir()
            .join(format!("gpui-secrets-index-{}", uuid::Uuid::now_v7()))
            .join("gpui_secrets.json");
        s.secrets = SecretVault::load(index, store.clone());
        s.secrets.finish_load(Default::default(), None);
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| window.bounds_changed(cx));
    window.draw();
    window.simulate_click(target(&app, "open-settings"), MouseButton::Left);
    window.draw();
    window.simulate_click(target(&app, "settings-secrets"), MouseButton::Left);
    window.read(|v, _| assert!(v.settings.section == SettingsSection::Secrets));
    window.draw();

    let fill = |window: &mut gpui::TestAppWindow<RootView>, name: &str, value: &str| {
        let (name, value) = (name.to_owned(), value.to_owned());
        window.update(move |v, _, cx| {
            let form = v.settings.secrets_form.as_ref().unwrap();
            let (n, val) = (form.name.clone(), form.value.clone());
            n.update(cx, |input, cx| input.set_text(name, cx));
            val.update(cx, |input, cx| input.set_text(value, cx));
        });
        window.draw();
    };

    fill(&mut window, "STEAM_TOKEN", "steam-value-1");
    window.simulate_click(target(&app, "settings-secrets-save"), MouseButton::Left);
    assert_eq!(
        store
            .values
            .lock()
            .unwrap()
            .get("STEAM_TOKEN")
            .map(String::as_str),
        Some("steam-value-1")
    );
    window.read(|v, cx| {
        let form = v.settings.secrets_form.as_ref().unwrap();
        assert_eq!(form.value.read(cx).text(), "", "the value leaves the UI");
        assert_eq!(form.notice, None);
    });
    app.read_entity(&state, |s, _| {
        assert_eq!(s.secrets.entries.len(), 1);
        assert_eq!(s.secrets.error, None);
    });

    window.draw();
    fill(&mut window, "PIN", "1234");
    window.simulate_click(target(&app, "settings-secrets-save"), MouseButton::Left);
    window.read(|v, _| assert!(v.settings.secrets_form.as_ref().unwrap().notice.is_some()));

    // Invalid names surface the vault error and store nothing.
    window.draw();
    fill(&mut window, "ZCODE_HOME", "some-value");
    window.simulate_click(target(&app, "settings-secrets-save"), MouseButton::Left);
    assert!(!store.values.lock().unwrap().contains_key("ZCODE_HOME"));
    app.read_entity(&state, |s, _| assert!(s.secrets.error.is_some()));
}
