use super::*;
use crate::provider_settings::model_draft_commit::DraftCommit;
use serde_json::json;

fn inherited() -> Value {
    json!({
        "enabled": true,
        "properties": {
            "requiresMfjsToolSchema": false,
            "contextWindow": 200000,
            "inputFormat": {
                "supportsText": true,
                "supportsImage": true,
                "supportsVideo": false,
                "supportsAudio": false,
                "supportsPdf": false
            },
            "outputFormat": { "supportsText": true },
            "supportsToolCall": true,
            "supportsJsonSchemaOutput": true,
            "supportsNativeWebSearch": false,
            "supportsMidConversationSystem": false
        },
        "optionSpecs": {
            "reasoningLevel": { "values": ["low", "high"], "map": "{}" },
            "maxOutputTokens": { "max": 32000, "map": "{'max_tokens': maxOutputTokens}" }
        }
    })
}

fn draft(id: &str, model: &CurrentModel) -> ModelDraft {
    let mut draft = ModelDraft::from_model(model);
    draft.id = id.into();
    draft
}

#[test]
fn smart_mode_without_edits_saves_an_empty_overlay() {
    let resolution = inherited();
    let model = CurrentModel::for_add(Some(&resolution));
    let draft = draft(" glm-x ", &CurrentModel::for_add(None)).project(&model);
    assert_eq!(draft.levels, ["low", "high"]);
    assert!(draft.input_flag("supportsImage"));
    assert_eq!(
        commit(&draft, &model),
        Ok(DraftCommit {
            model_id: "glm-x".into(),
            use_recommended: true,
            config: json!({}),
        })
    );
}

#[test]
fn smart_mode_keeps_only_explicit_overrides() {
    let resolution = inherited();
    let model = CurrentModel::for_add(Some(&resolution));
    let mut draft = draft("m", &CurrentModel::for_add(None));
    draft.context_window = "200000".into();
    draft.max_output = "8000".into();
    draft.toggle_capability(&model, 1);
    draft.toggle_input(&model, "supportsPdf");
    let saved = commit(&draft, &model).unwrap();
    assert_eq!(
        saved.config,
        json!({
            "properties": {
                // 与推荐值相同的显式数值也保留为覆盖。
                "contextWindow": 200000,
                "supportsNativeWebSearch": true,
                "inputFormat": { "supportsPdf": true }
            },
            "optionSpecs": { "maxOutputTokens": { "max": 8000 } }
        })
    );
}

#[test]
fn smart_mode_writes_reasoning_and_max_output_mappings() {
    let resolution = inherited();
    let model = CurrentModel::for_add(Some(&resolution));
    let mut draft = draft("m", &model);
    draft.set_levels(&model, vec!["low".into(), " mid ".into(), "high".into()]);
    draft.level_map = "{'effort': reasoningLevel}".into();
    draft.max_output_map = "{'max_completion_tokens': maxOutputTokens}".into();
    let saved = commit(&draft, &model).unwrap();
    assert_eq!(
        saved.config["optionSpecs"],
        json!({
            "reasoningLevel": { "values": ["low", "mid", "high"], "map": "{'effort': reasoningLevel}" },
            "maxOutputTokens": { "map": "{'max_completion_tokens': maxOutputTokens}" }
        })
    );
}

#[test]
fn manual_mode_materializes_editable_leaves() {
    let resolution = inherited();
    let model = CurrentModel::for_add(Some(&resolution));
    let mut draft = draft("m", &model);
    draft.set_recommended(&model, false);
    assert_eq!(draft.context_window, "200000");
    assert_eq!(draft.max_output, "32000");
    assert_eq!(draft.level_map, "{}");
    // 固定模式不再请求推荐，基线随之消失。
    let manual = CurrentModel::for_add(None);
    let saved = commit(&draft, &manual).unwrap();
    assert!(!saved.use_recommended);
    assert_eq!(
        saved.config,
        json!({
            "properties": {
                "contextWindow": 200000,
                "supportsJsonSchemaOutput": true,
                "supportsNativeWebSearch": false,
                "supportsMidConversationSystem": false,
                "inputFormat": { "supportsImage": true, "supportsVideo": false, "supportsPdf": false }
            },
            "optionSpecs": {
                "reasoningLevel": { "values": ["low", "high"], "map": "{}" },
                "maxOutputTokens": { "max": 32000 }
            }
        })
    );
}

#[test]
fn switching_back_to_smart_resets_to_recommended_defaults() {
    let resolution = inherited();
    let model = CurrentModel::for_add(Some(&resolution));
    let mut draft = draft("m", &model);
    draft.set_recommended(&model, false);
    draft.toggle_capability(&model, 0);
    draft.set_recommended(&model, true);
    assert!(draft.recommended);
    assert_eq!(draft.id, "m");
    assert!(draft.context_window.is_empty());
    assert!(draft.overridden.is_empty());
    assert_eq!(commit(&draft, &model).unwrap().config, json!({}));
}

#[test]
fn validation_reports_the_first_invalid_field() {
    let resolution = inherited();
    let smart = CurrentModel::for_add(Some(&resolution));
    let unresolved = CurrentModel::for_add(None);
    let base = draft("m", &smart).project(&smart);
    let check = |edit: &dyn Fn(&mut ModelDraft), model: &CurrentModel| {
        let mut d = base.clone();
        edit(&mut d);
        commit(&d, model).unwrap_err()
    };
    assert_eq!(check(&|d| d.id = "  ".into(), &smart), DraftField::Id);
    assert_eq!(
        check(&|d| d.context_window = "0".into(), &smart),
        DraftField::ContextWindow
    );
    assert_eq!(check(&|_| {}, &unresolved), DraftField::ContextWindow);
    assert_eq!(
        check(&|d| d.max_output = "x".into(), &smart),
        DraftField::MaxOutputTokens
    );
    assert_eq!(
        check(
            &|d| {
                d.recommended = false;
                d.context_window = "1".into()
            },
            &unresolved
        ),
        DraftField::MaxOutputTokens
    );
    assert_eq!(
        check(&|d| d.input[0].1 = false, &smart),
        DraftField::InputFormat
    );
    assert_eq!(
        check(&|d| d.levels = vec![], &smart),
        DraftField::ReasoningLevelValues
    );
    assert_eq!(
        check(&|d| d.levels = vec!["a".into(), " a".into()], &smart),
        DraftField::ReasoningLevelValues
    );
    let no_map = json!({ "properties": { "contextWindow": 1000 },
        "optionSpecs": { "reasoningLevel": { "values": ["low"] } } });
    let no_map = CurrentModel::for_add(Some(&no_map));
    let d = draft("m", &no_map).project(&no_map);
    assert_eq!(commit(&d, &no_map), Err(DraftField::ReasoningLevelMap));
}

#[test]
fn placeholders_follow_the_baseline_only_in_smart_mode() {
    let resolution = inherited();
    let model = CurrentModel::for_add(Some(&resolution));
    let mut draft = draft("m", &model);
    let shown = placeholders(&draft, &model);
    assert_eq!(shown.context_window.as_deref(), Some("200000"));
    assert_eq!(shown.max_output.as_deref(), Some("32000"));
    assert_eq!(shown.level_map.as_deref(), Some("{}"));
    assert_eq!(
        shown.max_output_map.as_deref(),
        Some("{'max_tokens': maxOutputTokens}")
    );
    draft.recommended = false;
    assert_eq!(placeholders(&draft, &model), Placeholders::default());
}

#[test]
fn editing_a_smart_model_keeps_its_saved_overrides() {
    let baseline = inherited();
    let personal = json!({
        "properties": {
            "contextWindow": 1000000,
            "supportsNativeWebSearch": true,
            "inputFormat": { "supportsVideo": true }
        },
        "optionSpecs": { "reasoningLevel": { "values": ["max"] } }
    });
    let mut effective = baseline.clone();
    effective["properties"]["contextWindow"] = json!(1000000);
    effective["properties"]["supportsNativeWebSearch"] = json!(true);
    effective["properties"]["inputFormat"]["supportsVideo"] = json!(true);
    effective["optionSpecs"]["reasoningLevel"]["values"] = json!(["max"]);
    let model = CurrentModel {
        config: effective,
        personal: personal.clone(),
        inherited: Some(baseline),
    };

    let draft = ModelDraft::for_edit(&model, "claude-opus-5-5", true);
    assert_eq!(draft.context_window, "1000000");
    let shown = draft.project(&model);
    // 已保存的覆盖在智能模式下不能被推荐基线顶掉。
    assert!(shown.capabilities[1], "native web search override kept");
    assert!(shown.input_flag("supportsVideo"), "video override kept");
    assert_eq!(shown.levels, vec!["max".to_string()]);

    let saved = commit(&shown, &model).unwrap();
    assert_eq!(saved.model_id, "claude-opus-5-5");
    assert!(saved.use_recommended);
    assert_eq!(saved.config["properties"]["contextWindow"], json!(1000000));
    assert_eq!(
        saved.config["properties"]["supportsNativeWebSearch"],
        json!(true)
    );
    assert_eq!(
        saved.config["properties"]["inputFormat"]["supportsVideo"],
        json!(true)
    );
    assert_eq!(
        saved.config["optionSpecs"]["reasoningLevel"]["values"],
        json!(["max"])
    );
    assert!(
        saved.config.get("enabled").is_none(),
        "the row switch owns enabled"
    );
}
