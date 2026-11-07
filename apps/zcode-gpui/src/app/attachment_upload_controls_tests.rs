use crate::app::{
    root::RootView,
    store::AppState,
    test_support::{RUNTIME_TEST_LOCK, TestTargets},
};
use gpui::{ClipboardItem, Image, ImageFormat, MouseButton, TestApp, px, size};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn clipboard_upload_cancel_retry_and_ready_send_are_reachable() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        crate::shared::preferences::Preferences::install_at(
            Default::default(),
            std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()),
            Arc::new(|| true),
            cx,
        );
    });
    let state = app.new_entity(AppState::for_test);
    let (tx, rx) = std::sync::mpsc::channel();
    let key = app.update_entity(&state, |s, cx| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.active_workspace = Some(key.clone());
        s.active = Some("parent".into());
        s.conversations
            .entry("parent".into())
            .or_default()
            .subscribed = true;
        s.draft = false;
        s.composer
            .update(cx, |composer, _| composer.set_text("keep this draft"));
        key
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
        window.bounds_changed(cx);
    });
    let mut bmp = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 0]))
        .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .unwrap();
    app.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Bmp,
            bmp,
        )))
    });
    window.draw();
    window.simulate_keystroke("secondary-v");
    window.draw();
    let begin: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(begin["method"], "v4/attachment/begin");
    // Providers reject image/bmp; the paste is transcoded before upload.
    assert_eq!(begin["params"]["mime"], "image/png");
    let upload = begin["params"]["uploadId"].as_str().unwrap().to_string();
    let send = app.update(|cx| cx.global::<TestTargets>().0["send-btn"]);
    window.simulate_click(send.center(), MouseButton::Left);
    assert!(rx.try_recv().is_err());
    app.update_entity(&state, |s, cx| {
        assert_eq!(s.composer.read(cx).text(), "keep this draft")
    });
    let cancel = app.update(|cx| cx.global::<TestTargets>().0["cancel-image-upload"]);
    window.simulate_click(cancel.center(), MouseButton::Left);
    let abort: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(abort["method"], "v4/attachment/abort");
    app.update_entity(&state, |s, cx| s.handle_response(&key, begin["id"].as_u64().unwrap(), Some(json!({"uploadId":upload,"state":"committed","nextChunkIndex":1,"ref":"zcode-artifact://parent/stale"})), None, cx));
    app.update_entity(&state, |s, cx| {
        assert!(s.composer.read(cx).attachments.is_empty())
    });
    window.update(|_, window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    window.simulate_keystroke("secondary-v");
    window.draw();
    let begin: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    app.update_entity(&state, |s, cx| {
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            None,
            Some(json!({"message":"do-not-store-image-sentinel"})),
            cx,
        )
    });
    window.draw();
    let retry = app.update(|cx| cx.global::<TestTargets>().0["retry-image-upload"]);
    window.simulate_click(retry.center(), MouseButton::Left);
    window.draw();
    let abort: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(abort["method"], "v4/attachment/abort");
    let begin: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    let id = begin["params"]["uploadId"].as_str().unwrap().to_string();
    app.update_entity(&state, |s, cx| {
        s.handle_response(
            &key,
            begin["id"].as_u64().unwrap(),
            Some(json!({"uploadId":id,"state":"staging","nextChunkIndex":0})),
            None,
            cx,
        )
    });
    let chunk: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    app.update_entity(&state, |s, cx| {
        s.handle_response(
            &key,
            chunk["id"].as_u64().unwrap(),
            Some(json!({"uploadId":id,"nextChunkIndex":1})),
            None,
            cx,
        )
    });
    let commit: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    app.update_entity(&state, |s, cx| {
        s.handle_response(
            &key,
            commit["id"].as_u64().unwrap(),
            Some(json!({"ref":"zcode-artifact://parent/ready"})),
            None,
            cx,
        )
    });
    window.draw();
    let send = app.update(|cx| cx.global::<TestTargets>().0["send-btn"]);
    window.simulate_click(send.center(), MouseButton::Left);
    let send: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
    assert_eq!(
        send["params"]["payload"]["attachments"][0]["ref"],
        "zcode-artifact://parent/ready"
    );
    assert_eq!(
        send["params"]["payload"]["attachments"][0]["mime"],
        "image/png"
    );
}

#[test]
fn undecodable_clipboard_bitmap_is_refused_before_upload() {
    let _guard = RUNTIME_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(ely_gpui_component::Assets),
    );
    app.update(|cx| {
        ely_gpui_component::init(cx);
        cx.set_global(TestTargets::default());
        crate::shared::preferences::Preferences::install_at(
            Default::default(),
            std::env::temp_dir().join(uuid::Uuid::now_v7().to_string()),
            Arc::new(|| true),
            cx,
        );
    });
    let state = app.new_entity(AppState::for_test);
    let (tx, rx) = std::sync::mpsc::channel();
    app.update_entity(&state, |s, _| {
        let key = s.workspaces[0].key.clone();
        s.workspaces[0].inbound = Some(tx);
        s.workspaces[0].started = true;
        s.workspaces[0].pumping = true;
        s.active_workspace = Some(key);
        s.active = Some("parent".into());
        s.conversations
            .entry("parent".into())
            .or_default()
            .subscribed = true;
        s.draft = false;
    });
    let mut window = app.open_window(|_, cx| RootView::new(state.clone(), cx));
    window.simulate_resize(size(px(1280.), px(820.)));
    window.update(|_, window, cx| {
        let focus = state.read(cx).composer.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    app.update(|cx| {
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
            ImageFormat::Bmp,
            vec![1, 2, 3],
        )))
    });
    window.draw();
    window.simulate_keystroke("secondary-v");
    window.draw();
    assert!(rx.try_recv().is_err());
    app.update_entity(&state, |s, cx| {
        assert!(s.composer.read(cx).attachments.is_empty())
    });
}
