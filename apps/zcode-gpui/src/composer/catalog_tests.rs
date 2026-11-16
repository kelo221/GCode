use super::*;
use crate::composer::config_cmds::EffectiveConfig;

fn opt(provider: &str, model: &str, name: &str, provider_name: &str) -> ModelOption {
    ModelOption {
        value: format!("{provider}/{model}"),
        name: name.to_string(),
        provider_name: provider_name.to_string(),
        thought_levels: Vec::new(),
        default_thought: String::new(),
        provider: provider.to_string(),
        model: model.to_string(),
    }
}

#[test]
fn workspace_slash_catalog_is_whole_replacement_without_builtin_fallback() {
    let mut config = WorkspaceConfig::default();
    config.apply_state(
        &serde_json::json!({"slashCommands":[{"name":"init","description":"Initialize"}]}),
    );
    assert_eq!(config.slash_commands()[0].name, "init");
    config.apply_state(&serde_json::json!({"slashCommands":[]}));
    assert!(config.slash_commands().is_empty());
    config.apply_state(
        &serde_json::json!({"slashCommands":[{"name":"init","description":"Initialize"}]}),
    );
    config.apply_state(&serde_json::json!({"configOptions":[]}));
    assert!(config.slash_commands().is_empty());
}

#[test]
fn test_group_models_by_provider() {
    let models = vec![
        opt("zaiPlan", "glm-4.6", "GLM-4.6", "GLM Coding Plan"),
        opt("zaiPlan", "glm-4.5-air", "GLM-4.5-Air", "GLM Coding Plan"),
        opt("custom", "my-model", "My Model", "My Provider"),
    ];

    let groups = group_models(&models);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "GLM Coding Plan");
    assert_eq!(groups[0].items.len(), 2);
    assert_eq!(groups[0].items[1].model, "glm-4.5-air");
    assert_eq!(groups[1].label, "My Provider");
}

#[test]
fn test_group_models_label_falls_back_to_provider_id() {
    let models = vec![opt("openai", "gpt-5", "GPT-5", "")];
    let groups = group_models(&models);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "openai");
}

#[test]
fn test_group_models_same_name_different_ids_stay_separate() {
    let models = vec![opt("a", "m1", "M1", "Acme"), opt("b", "m2", "M2", "Acme")];
    let groups = group_models(&models);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].items[0].model, "m1");
    assert_eq!(groups[1].items[0].model, "m2");
}

#[test]
fn test_group_models_preserves_catalog_order() {
    let models = vec![
        opt("b", "m-b", "MB", "B"),
        opt("a", "m-a", "MA", "A"),
        opt("b", "m-b2", "MB2", "B"),
    ];
    let groups = group_models(&models);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].label, "B");
    assert_eq!(groups[0].items.len(), 2);
    assert_eq!(groups[1].label, "A");
}

#[test]
fn test_trigger_display_prefers_catalog_name() {
    let models = vec![opt("zaiPlan", "glm-4.6", "GLM-4.6", "GLM Coding Plan")];
    let effective = EffectiveConfig {
        provider: "zaiPlan".into(),
        model: "glm-4.6".into(),
        ..Default::default()
    };
    // Single provider group: bare model name, no redundant provider prefix.
    assert_eq!(effective.trigger_display(&models), "GLM-4.6");
}

#[test]
fn test_trigger_display_prefixes_provider_when_multi_provider() {
    let models = vec![
        opt("zaiPlan", "glm-4.6", "GLM-4.6", "GLM Coding Plan"),
        opt("custom", "my-model", "My Model", "My Provider"),
    ];
    let effective = EffectiveConfig {
        provider: "custom".into(),
        model: "my-model".into(),
        ..Default::default()
    };
    assert_eq!(effective.trigger_display(&models), "My Provider/My Model");
}

#[test]
fn test_trigger_display_falls_back_outside_catalog() {
    // `<synthetic>` and retired ids are not in the catalog; fall back to the
    // effective projection instead of rendering a wrong name.
    let effective = EffectiveConfig {
        model_label: "restored model".into(),
        model: "<synthetic>".into(),
        ..Default::default()
    };
    assert_eq!(effective.trigger_display(&[]), "restored model");

    let effective = EffectiveConfig {
        model: "<synthetic>".into(),
        ..Default::default()
    };
    assert_eq!(effective.trigger_display(&[]), "<synthetic>");
}

#[test]
fn model_unavailable_only_for_a_session_model_missing_from_a_loaded_catalog() {
    let models = vec![opt("zaiPlan", "glm-4.6", "GLM-4.6", "GLM Coding Plan")];
    let session = |provider: &str, model: &str| EffectiveConfig {
        provider: provider.into(),
        model: model.into(),
        has_session: true,
        ..Default::default()
    };
    assert!(!session("zaiPlan", "glm-4.6").model_unavailable(&models));
    // Removed provider and retired id both warn.
    assert!(session("gone", "glm-4.6").model_unavailable(&models));
    assert!(session("zaiPlan", "glm-3").model_unavailable(&models));
    // No warning before the catalog loads, for placeholders, or without a session.
    assert!(!session("gone", "x").model_unavailable(&[]));
    assert!(!session("zaiPlan", "<synthetic>").model_unavailable(&models));
    assert!(!session("", "").model_unavailable(&models));
    let draft = EffectiveConfig {
        has_session: false,
        ..session("gone", "x")
    };
    assert!(!draft.model_unavailable(&models));
}
