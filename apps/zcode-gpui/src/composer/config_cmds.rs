//! Model / thinking-level / collaboration-mode switching (CAS acks are handled
//! in backend/session_cmds.rs) and the effective config projection used by the
//! selectors.

use crate::app::store::AppState;
use crate::backend::workspace::CommandCtx;
use gpui::Context;
use serde_json::{Value, json};

impl AppState {
    /// Switch model (and optionally thinking level) on the active session.
    /// `thought` empty string means "no explicit thought".
    pub fn switch_model(
        &mut self,
        provider: &str,
        model: &str,
        thought: &str,
        cx: &mut Context<Self>,
    ) {
        let (Some(sid), Some(ws_key)) = (self.active.clone(), self.active_ws_key()) else {
            return;
        };
        if self.set_restored_model(provider, model, thought) {
            cx.notify();
            return;
        }
        let payload = json!({ "provider": provider, "model": model, "thought": thought });
        self.send_session_command(
            &ws_key,
            CommandCtx::new(&sid, "switchModelConfig", payload).cas(),
        );
        cx.notify();
    }

    /// Thinking-level-only switch: same provider/model, different `thought`.
    pub fn switch_thought(&mut self, thought: &str, cx: &mut Context<Self>) {
        let effective = self.effective_config();
        let (provider, model) = (effective.provider, effective.model);
        self.switch_model(&provider, &model, thought, cx);
    }

    /// Collaboration mode: build | edit | plan | yolo ("Full access").
    pub fn switch_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        let (Some(sid), Some(ws_key)) = (self.active.clone(), self.active_ws_key()) else {
            return;
        };
        if self.set_restored_mode(mode) {
            cx.notify();
            return;
        }
        let payload = json!({ "mode": mode });
        self.send_session_command(
            &ws_key,
            CommandCtx::new(&sid, "switchCollaborationMode", payload).cas(),
        );
        cx.notify();
    }

    pub(crate) fn find_model_option(
        &self,
        value: &str,
    ) -> Option<crate::composer::catalog::ModelOption> {
        let key = self.active_ws_key()?;
        self.workspace_configs
            .get(&key)?
            .models
            .iter()
            .find(|m| m.value == value)
            .cloned()
    }

    /// createSession.config override assembled from draft UI selections.
    pub(crate) fn draft_config(&self) -> Value {
        let mut config = json!({});
        if let Some(opt) = self.draft_model_option() {
            let thought = if opt.thought_levels.is_empty() {
                String::new()
            } else {
                opt.default_thought.clone()
            };
            config["provider"] = json!(opt.provider);
            config["model"] = json!(opt.model);
            if !thought.is_empty() {
                config["thought"] = json!(thought);
            }
        }
        let draft = self.effective_config_with_override(false);
        if !draft.thought.is_empty() && !draft.provider.is_empty() && !draft.model.is_empty() {
            // Desktop parity: handleDraftSelectThought (useDraftConfigControl.ts)
            // submits both `thought` and `modelSelection.options.reasoningLevel`.
            config["provider"] = json!(draft.provider);
            config["model"] = json!(draft.model);
            config["thought"] = json!(draft.thought);
            config["modelSelection"] = json!({
                "providerId": draft.provider,
                "modelId": draft.model,
                "options": { "reasoningLevel": draft.thought },
            });
        }
        if self.ui_mode.is_some() || self.ui_plan {
            // Desktop parity (composerSubmissionConfig.ts): the permission mode
            // and the plan overlay travel separately. 原来 plan 一律提交为 build，
            // 批准计划后会话回到“变更前确认”，丢掉了用户选的完全访问。
            config["mode"] = json!(draft.mode);
            if draft.plan_enabled {
                config["planEnabled"] = json!(true);
            }
        }
        config
    }

    /// Effective session config for rendering: live session config, else the
    /// draft selections, else workspace defaults from the config topic.
    pub fn effective_config(&self) -> EffectiveConfig {
        self.effective_config_with_override(true)
    }

    fn effective_config_with_override(&self, use_override: bool) -> EffectiveConfig {
        if use_override && let Value::Object(config) = self.submission_override() {
            let config = Value::Object(config);
            let base = self.effective_config_with_override(false);
            return EffectiveConfig {
                provider: config
                    .pointer("/modelSelection/providerId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| base.provider.clone()),
                model: config
                    .pointer("/modelSelection/modelId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| base.model.clone()),
                thought: config
                    .pointer("/modelSelection/options/reasoningLevel")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| base.thought.clone()),
                thought_levels: self
                    .active_ws_key()
                    .and_then(|key| self.workspace_configs.get(&key))
                    .and_then(|catalog| {
                        catalog.models.iter().find(|m| {
                            Some(m.provider.as_str())
                                == config
                                    .pointer("/modelSelection/providerId")
                                    .and_then(Value::as_str)
                                && Some(m.model.as_str())
                                    == config
                                        .pointer("/modelSelection/modelId")
                                        .and_then(Value::as_str)
                        })
                    })
                    .map(|m| m.thought_levels.clone())
                    .unwrap_or_else(|| base.thought_levels.clone()),
                model_label: base.model_label,
                mode: config
                    .get("mode")
                    .and_then(Value::as_str)
                    .filter(|mode| *mode != "plan")
                    .map(str::to_owned)
                    .unwrap_or_else(|| base.mode.clone()),
                plan_enabled: config
                    .get("planEnabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(base.plan_enabled),
                has_session: self.active.is_some(),
            };
        }
        if let Some(c) = self.active_conversation() {
            return EffectiveConfig {
                provider: c.config.provider.clone(),
                model: c.config.model.clone(),
                model_label: String::new(),
                thought: c.config.thought.clone(),
                thought_levels: c.config.thought_levels.clone(),
                mode: crate::composer::plan_mode::permission_mode(&c.config.mode).into(),
                plan_enabled: c.config.plan_enabled,
                has_session: true,
            };
        }
        let ws_key = self.active_ws_key();
        let cfg = ws_key
            .as_deref()
            .and_then(|k| self.workspace_configs.get(k));
        // Draft model: UI choice wins, else the last used model, else the
        // workspace default entry.
        let last_used = self
            .ui_model_value
            .is_none()
            .then(|| self.last_used_option())
            .flatten();
        let (model_label, provider, model, thought_levels) =
            if let Some(value) = &self.ui_model_value {
                match self.find_model_option(value) {
                    Some(opt) => (
                        opt.name.clone(),
                        opt.provider.clone(),
                        opt.model.clone(),
                        opt.thought_levels.clone(),
                    ),
                    None => (value.clone(), String::new(), String::new(), vec![]),
                }
            } else if let Some(opt) = &last_used {
                (
                    opt.name.clone(),
                    opt.provider.clone(),
                    opt.model.clone(),
                    opt.thought_levels.clone(),
                )
            } else if let Some(ws_cfg) = cfg {
                let default_value = ws_cfg
                    .raw_options
                    .iter()
                    .find(|o| o.get("id").and_then(Value::as_str) == Some("model"))
                    .and_then(|o| o.get("currentValue"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                match ws_cfg.models.iter().find(|m| m.value == default_value) {
                    Some(opt) => (
                        opt.name.clone(),
                        opt.provider.clone(),
                        opt.model.clone(),
                        opt.thought_levels.clone(),
                    ),
                    None => (String::new(), String::new(), String::new(), vec![]),
                }
            } else {
                (String::new(), String::new(), String::new(), vec![])
            };
        let source = self
            .ui_mode
            .as_deref()
            .or_else(|| cfg.map(|c| c.default_mode.as_str()))
            .unwrap_or("");
        let mode = crate::composer::plan_mode::permission_mode(source).to_string();
        let plan_enabled = self.ui_plan || source == "plan";
        let remembered_thought = last_used
            .is_some()
            .then(|| self.draft_models.last.as_ref().map(|l| l.thought.clone()))
            .flatten();
        let thought = self
            .ui_thought
            .clone()
            .or(remembered_thought)
            .filter(|t| thought_levels.contains(t))
            .unwrap_or_default();
        EffectiveConfig {
            provider,
            model,
            model_label,
            thought,
            thought_levels,
            mode,
            plan_enabled,
            has_session: false,
        }
    }
}

#[derive(Clone, Default)]
pub struct EffectiveConfig {
    pub provider: String,
    pub model: String,
    pub model_label: String,
    pub thought: String,
    pub thought_levels: Vec<String>,
    /// Permission mode (build | guarded | yolo, legacy edit); never "plan".
    pub mode: String,
    /// Plan overlay on top of `mode`.
    pub plan_enabled: bool,
    pub has_session: bool,
}

impl EffectiveConfig {
    pub fn model_display(&self) -> String {
        if !self.model_label.is_empty() {
            self.model_label.clone()
        } else if !self.model.is_empty() {
            self.model.clone()
        } else {
            "model…".into()
        }
    }

    /// Trigger label with the catalog's display name (desktop parity:
    /// `resolveV4ModelTriggerDisplay` — builtin/family providers show the bare
    /// model name; custom providers get a `Provider/Model` prefix, only meaningful
    /// when the catalog actually has more than one provider). Falls back to
    /// `model_display` when the current model is not in the catalog
    /// (`<synthetic>`, retired ids, …).
    pub fn trigger_display(&self, models: &[crate::composer::catalog::ModelOption]) -> String {
        let matched = models
            .iter()
            .find(|o| o.provider == self.provider && o.model == self.model);
        let Some(opt) = matched else {
            return self.model_display();
        };
        let multi_provider = models.iter().any(|o| o.provider != self.provider);
        if multi_provider && !opt.provider_name.is_empty() {
            format!("{}/{}", opt.provider_name, opt.name)
        } else {
            opt.name.clone()
        }
    }

    /// A session whose provider/model is missing from the loaded catalog (the
    /// provider was removed or the id retired): the CLI rejects sends with
    /// `modelUnavailable`, so the trigger warns before that happens.
    /// Placeholder ids (`<synthetic>`) and an unloaded catalog never warn.
    pub fn model_unavailable(&self, models: &[crate::composer::catalog::ModelOption]) -> bool {
        self.has_session
            && !models.is_empty()
            && !self.provider.is_empty()
            && !self.model.is_empty()
            && !self.model.starts_with('<')
            && !models
                .iter()
                .any(|o| o.provider == self.provider && o.model == self.model)
    }

    pub fn thought_display(&self) -> String {
        let v = self.thought.trim().to_lowercase();
        match v.as_str() {
            "disabled" | "false" | "no" | "none" | "off" | "nothink" | "no-think" | "no_think" => {
                "Off".into()
            }
            "enable" | "enabled" | "on" | "true" => "On".into(),
            "low" => "Low".into(),
            "minimal" => "Minimal".into(),
            "medium" => "Medium".into(),
            "high" => "High".into(),
            "max" => "Max".into(),
            "ultra" => "Ultra".into(),
            "extra-high" | "extra_high" | "xhigh" => "X-High".into(),
            "" => "Default".into(),
            _ => title_case(&self.thought),
        }
    }
}

/// "medium" -> "Medium" (thinking-level labels, as in the desktop picker).
pub fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thought_display_mapping() {
        let display = |t: &str| {
            EffectiveConfig {
                thought: t.into(),
                ..Default::default()
            }
            .thought_display()
        };

        assert_eq!(display("enabled"), "On");
        assert_eq!(display("on"), "On");
        assert_eq!(display("disabled"), "Off");
        assert_eq!(display("off"), "Off");
        assert_eq!(display(""), "Default");
        assert_eq!(display("high"), "High");
        assert_eq!(display("xhigh"), "X-High");
    }
}
