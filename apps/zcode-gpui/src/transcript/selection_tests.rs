//! Transcript text is selectable like Desktop's browser text: a path in an
//! expanded tool card can be drag-selected and copied.

use crate::app::{
    root::RootView,
    store::AppState,
    test_support::{RUNTIME_TEST_LOCK, TestTargets},
};
use crate::conversation::model::{ConversationState, Row};
use gpui::{Modifiers, MouseButton, MouseMoveEvent, TestApp, point, px, size};
use std::sync::Arc;

const PATH: &str = r"C:\Users\V\Documents\DynLightBevy\screenshots\inspector.png";

#[test]
fn tool_card_path_can_be_drag_selected_and_copied() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        crate::shared::preferences::Preferences::install_at(
            // Tool cards fold in noise-free mode; this test drags inside one.
            crate::shared::settings::AppSettings {
                gpui_noise_free: Some(false),
                ..Default::default()
            },
            std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()),
            Arc::new(|| true),
            cx,
        );
    });
    let state = app.new_entity(AppState::for_test);
    app.update_entity(&state, |s, _| {
        s.active_workspace = Some(s.workspaces[0].key.clone());
        s.active = Some("s".into());
        let mut conv = ConversationState {
            subscribed: true,
            ..Default::default()
        };
        conv.rows.insert(
            1,
            Row::ToolCall {
                row_id: 1,
                label: "Read".into(),
                tool_name: "Read".into(),
                status: "success".into(),
                input_text: serde_json::json!({ "file_path": PATH }).to_string(),
                output_text: "[Attached image/jpeg: Read image]".into(),
                tool_call_id: None,
                display: None,
                needs_read: false,
            },
        );
        s.conversations.insert("s".into(), conv);
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|v, window, cx| {
        window.bounds_changed(cx);
        v.expanded_tools.insert(1);
        cx.notify();
    });
    window.draw();
    window.draw();
    let field = app.update(|cx| cx.global::<TestTargets>().0["selectable-field-file_path"]);
    let y = field.top() + px(5.);

    window.simulate_mouse_down(point(field.left() + px(1.), y), MouseButton::Left);
    window.draw();
    window.simulate_event(MouseMoveEvent {
        position: point(field.right() + px(40.), y),
        modifiers: Modifiers::default(),
        pressed_button: Some(MouseButton::Left),
    });
    window.simulate_mouse_up(point(field.right() + px(40.), y), MouseButton::Left);
    window.draw();
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    window.simulate_keystroke(&format!("{primary}-c"));
    assert_eq!(
        app.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some(PATH)
    );
    window.read(|v, _| {
        assert!(
            v.expanded_tools.contains(&1),
            "selecting does not collapse the card"
        )
    });
}
