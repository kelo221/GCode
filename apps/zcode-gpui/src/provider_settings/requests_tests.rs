use crate::app::store::AppState;
use crate::provider_settings::TestStatus;
use gpui::{AppContext, TestAppContext};
use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

fn connect(s: &mut AppState) -> (String, Receiver<String>) {
    let key = s.workspaces[0].key.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    s.workspaces[0].inbound = Some(tx);
    s.workspaces[0].started = true;
    s.active_workspace = Some(key.clone());
    (key, rx)
}

fn next(rx: &Receiver<String>) -> Value {
    serde_json::from_str(&rx.try_recv().unwrap()).unwrap()
}

fn view() -> Value {
    crate::provider_settings::tests::sample_view()
}

#[gpui::test]
fn read_loads_the_view_and_old_agents_fall_back(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.read_provider_settings(true, cx);
        let sent = next(&rx);
        assert_eq!(sent["method"], "providerSettings/read");
        assert_eq!(sent["params"], json!({ "refresh": true }));
        assert!(s.provider_settings.loading);
        s.handle_response(
            &key,
            sent["id"].as_u64().unwrap(),
            Some(json!({ "view": view() })),
            None,
            cx,
        );
        assert!(!s.provider_settings.loading);
        assert_eq!(s.provider_settings.view.as_ref().unwrap().revision, 7);

        s.read_provider_settings(false, cx);
        let sent = next(&rx);
        let missing = json!({ "code": -32601, "message": "Method not found" });
        s.handle_response(&key, sent["id"].as_u64().unwrap(), None, Some(missing), cx);
        assert!(s.provider_settings.unavailable);
        assert!(s.provider_settings.error.is_none());
    });
}

#[gpui::test]
fn mutation_is_serialized_selects_created_provider_and_reprobes_catalog(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.mutate_provider_settings(
            json!({ "type": "createProvider", "providerName": "New" }),
            cx,
        );
        let sent = next(&rx);
        assert_eq!(sent["method"], "providerSettings/mutate");
        assert_eq!(sent["params"]["operation"]["type"], "createProvider");
        assert!(s.provider_settings.busy);
        // A commit made while busy waits for the first response.
        let queued = json!({ "type": "updateProvider", "providerId": "c1", "apiKey": "k" });
        s.mutate_provider_settings(queued.clone(), cx);
        assert!(rx.try_recv().is_err());

        s.provider_settings.adding = true;
        let result = json!({ "view": view(), "providerId": "c2" });
        s.handle_response(&key, sent["id"].as_u64().unwrap(), Some(result), None, cx);
        assert!(!s.provider_settings.adding);
        assert_eq!(s.provider_settings.selected.as_deref(), Some("c2"));
        assert_eq!(
            next(&rx)["method"],
            "session/create",
            "model catalog re-probed"
        );
        let follow_up = next(&rx);
        assert_eq!(follow_up["params"]["operation"], queued);
        assert!(s.provider_settings.busy);
        s.handle_response(
            &key,
            follow_up["id"].as_u64().unwrap(),
            Some(json!({ "view": view() })),
            None,
            cx,
        );
        assert!(!s.provider_settings.busy);
        assert!(s.provider_settings.queued.is_empty());
    });
}

#[gpui::test]
fn mutation_errors_surface_and_keep_the_cached_view(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.provider_settings.view = crate::provider_settings::ProviderView::from_value(&view());
        s.mutate_provider_settings(json!({ "type": "deleteProvider", "providerId": "zai" }), cx);
        let sent = next(&rx);
        let error = json!({ "code": -32602, "message": "Only custom providers can be deleted" });
        s.handle_response(&key, sent["id"].as_u64().unwrap(), None, Some(error), cx);
        assert_eq!(
            s.provider_settings.error.as_deref(),
            Some("Only custom providers can be deleted")
        );
        assert!(s.provider_settings.view.is_some());
        assert!(
            rx.try_recv().is_err(),
            "no catalog probe after a failed write"
        );
    });
}

#[gpui::test]
fn connectivity_test_records_pass_and_failure(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.test_provider_model("c1", "m1", cx);
        let sent = next(&rx);
        assert_eq!(sent["method"], "provider/testModelConnectivity");
        assert_eq!(
            sent["params"]["selection"],
            json!({ "providerId": "c1", "modelId": "m1" })
        );
        assert_eq!(sent["params"]["workspace"]["workspaceKey"], key);
        let slot = ("c1".to_owned(), "m1".to_owned());
        assert_eq!(s.provider_settings.tests[&slot], TestStatus::Running);
        s.handle_response(
            &key,
            sent["id"].as_u64().unwrap(),
            Some(json!({ "success": true })),
            None,
            cx,
        );
        assert_eq!(s.provider_settings.tests[&slot], TestStatus::Passed);

        s.test_provider_model("c1", "m1", cx);
        let sent = next(&rx);
        let error = json!({ "code": -32000, "message": "401 Unauthorized" });
        s.handle_response(&key, sent["id"].as_u64().unwrap(), None, Some(error), cx);
        assert_eq!(
            s.provider_settings.tests[&slot],
            TestStatus::Failed("401 Unauthorized".into())
        );
    });
}

#[gpui::test]
fn requests_without_a_started_agent_report_instead_of_hanging(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        s.read_provider_settings(false, cx);
        assert!(!s.provider_settings.loading);
        assert!(s.provider_settings.error.is_some());
    });
}

#[gpui::test]
fn tagged_mutation_reports_to_its_dialog_not_the_page(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let (key, rx) = connect(s);
        s.provider_settings.view = crate::provider_settings::ProviderView::from_value(&view());
        let operation = json!({ "type": "addModel", "providerId": "c1", "modelId": "m9" });
        let tag = s.mutate_provider_settings_tagged(operation.clone(), cx);
        let sent = next(&rx);
        assert_eq!(sent["params"]["operation"], operation);
        let error = json!({ "code": -32602, "message": "bad map" });
        s.handle_response(&key, sent["id"].as_u64().unwrap(), None, Some(error), cx);
        assert_eq!(
            s.provider_settings.added,
            Some((tag, Err("bad map".into())))
        );
        assert!(s.provider_settings.error.is_none());

        let again = s.mutate_provider_settings_tagged(operation, cx);
        assert_ne!(again, tag);
        let sent = next(&rx);
        let result = Some(json!({ "view": view() }));
        s.handle_response(&key, sent["id"].as_u64().unwrap(), result, None, cx);
        assert_eq!(s.provider_settings.added, Some((again, Ok(()))));
    });
}

#[gpui::test]
fn resolve_model_config_settles_under_its_tag(cx: &mut TestAppContext) {
    let state = cx.new(AppState::for_test);
    state.update(cx, |s, cx| {
        let tag = s.resolve_provider_model_config("c1", "m", false, cx);
        assert!(matches!(s.provider_settings.resolved, Some((t, Err(_))) if t == tag));
        assert!(
            s.provider_settings.error.is_none(),
            "preview failure stays in the dialog"
        );

        let (key, rx) = connect(s);
        s.mutate_provider_settings(
            json!({ "type": "deleteModel", "providerId": "c1", "modelId": "m1" }),
            cx,
        );
        let mutation = next(&rx);
        // 预览不与变更串行：变更在途时也立即发送。
        let tag = s.resolve_provider_model_config("c1", "glm", false, cx);
        let sent = next(&rx);
        assert_eq!(sent["method"], "providerSettings/resolveModelConfig");
        assert_eq!(
            sent["params"],
            json!({ "providerId": "c1", "modelId": "glm" })
        );
        let inherited = json!({ "properties": { "contextWindow": 1000 } });
        let result = json!({ "inheritedConfig": inherited, "effectiveConfig": {}, "issues": [] });
        s.handle_response(&key, sent["id"].as_u64().unwrap(), Some(result), None, cx);
        assert_eq!(s.provider_settings.resolved, Some((tag, Ok(inherited))));
        assert!(s.provider_settings.busy, "the mutation is still in flight");
        let _ = mutation;
    });
}
