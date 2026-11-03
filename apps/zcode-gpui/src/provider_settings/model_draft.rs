//! Add-model dialog draft (desktop `ProviderModelMetadata.ts` +
//! `ProviderModelDraftState.ts`, add mode). The agent resolves recommended
//! configs; the draft only holds the user's intent, and untouched controls are
//! projected from the latest `inheritedConfig` (see PROVIDER_SETTINGS.md).

use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(crate) use super::model_draft_commit::{DraftField, commit};

/// Capability toggles in display order (`properties.<key>`).
pub(crate) const CAPABILITIES: [&str; 3] = [
    "supportsJsonSchemaOutput",
    "supportsNativeWebSearch",
    "supportsMidConversationSystem",
];
/// Input types the dialog toggles; text is always required.
pub(crate) const MODALITIES: [&str; 3] = ["supportsImage", "supportsVideo", "supportsPdf"];
const INPUT_KEYS: [&str; 5] = [
    "supportsText",
    "supportsImage",
    "supportsVideo",
    "supportsAudio",
    "supportsPdf",
];
const LEVELS_KEY: &str = "reasoningLevelValuesValue";

/// The model the draft edits: effective config, personal overlay and the
/// recommended baseline (absent until the agent resolved one).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CurrentModel {
    pub config: Value,
    pub personal: Value,
    pub inherited: Option<Value>,
}

impl CurrentModel {
    /// A model being added: no overlay yet; the resolution, when present, is
    /// both its effective and its inherited config (desktop `modelWithResolution`).
    pub(crate) fn for_add(resolution: Option<&Value>) -> Self {
        match resolution {
            Some(inherited) => Self {
                config: inherited.clone(),
                personal: Value::Object(Map::new()),
                inherited: Some(inherited.clone()),
            },
            None => Self {
                config: Value::Object(Map::new()),
                personal: Value::Object(Map::new()),
                inherited: None,
            },
        }
    }

    /// Recommended baseline, falling back to the effective config.
    fn baseline(&self) -> &Value {
        self.inherited.as_ref().unwrap_or(&self.config)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModelDraft {
    pub id: String,
    pub context_window: String,
    pub max_output: String,
    /// `supportsText`, `supportsImage`, ... → value.
    pub input: [(&'static str, bool); 5],
    pub enabled: bool,
    pub recommended: bool,
    /// Controls the user touched (desktop `overriddenFieldsValue` keys).
    pub overridden: BTreeSet<String>,
    pub capabilities: [bool; 3],
    pub levels: Vec<String>,
    pub level_map: String,
    /// GPUI addition: explicit `optionSpecs.maxOutputTokens.map`.
    pub max_output_map: String,
}

impl ModelDraft {
    /// Desktop `createProviderModelDraftValues`: text fields come from the
    /// personal overlay only; inherited values are placeholders.
    pub(crate) fn from_model(model: &CurrentModel) -> Self {
        let config = &model.config;
        let personal = &model.personal;
        let input = INPUT_KEYS.map(|key| {
            let value = flag(config, &["properties", "inputFormat", key]);
            (key, value.unwrap_or(key == "supportsText"))
        });
        Self {
            id: String::new(),
            context_window: at(personal, &["properties", "contextWindow"])
                .map(number_text)
                .unwrap_or_default(),
            max_output: at(personal, &["optionSpecs", "maxOutputTokens", "max"])
                .map(number_text)
                .unwrap_or_default(),
            input,
            enabled: at(config, &["enabled"]).and_then(Value::as_bool) != Some(false),
            recommended: true,
            overridden: BTreeSet::new(),
            capabilities: CAPABILITIES
                .map(|key| flag(config, &["properties", key]).unwrap_or(false)),
            levels: levels(config),
            level_map: text(personal, &["optionSpecs", "reasoningLevel", "map"]),
            max_output_map: text(personal, &["optionSpecs", "maxOutputTokens", "map"]),
        }
    }

    /// Edit mode (desktop `mode="edit"`): starts from the saved model. Leaves
    /// the personal overlay already sets count as touched, so smart mode keeps
    /// showing and re-saving them instead of the recommended baseline.
    pub(crate) fn for_edit(model: &CurrentModel, id: &str, recommended: bool) -> Self {
        let mut draft = Self::from_model(model);
        draft.id = id.to_owned();
        draft.recommended = recommended;
        let personal = &model.personal;
        for key in CAPABILITIES {
            if at(personal, &["properties", key]).is_some() {
                draft.overridden.insert(format!("{key}Value"));
            }
        }
        for key in INPUT_KEYS {
            if at(personal, &["properties", "inputFormat", key]).is_some() {
                draft.overridden.insert(format!("inputFormatValue.{key}"));
            }
        }
        if at(personal, &["optionSpecs", "reasoningLevel", "values"]).is_some() {
            draft.overridden.insert(LEVELS_KEY.into());
        }
        draft
    }

    pub(crate) fn input_flag(&self, key: &str) -> bool {
        self.input
            .iter()
            .find(|(k, _)| *k == key)
            .is_some_and(|(_, v)| *v)
    }

    /// Desktop `projectModelDraft`: in smart mode untouched controls follow
    /// the baseline; manual mode keeps the draft as typed.
    pub(crate) fn project(&self, model: &CurrentModel) -> Self {
        if !self.recommended {
            return self.clone();
        }
        let defaults = Self::from_model(&CurrentModel {
            config: model.baseline().clone(),
            personal: Value::Object(Map::new()),
            inherited: None,
        });
        let mut next = self.clone();
        for (index, key) in CAPABILITIES.iter().enumerate() {
            if !self.overridden.contains(&format!("{key}Value")) {
                next.capabilities[index] = defaults.capabilities[index];
            }
        }
        if !self.overridden.contains(LEVELS_KEY) {
            next.levels = defaults.levels.clone();
        }
        for (index, (key, _)) in self.input.iter().enumerate() {
            if !self.overridden.contains(&format!("inputFormatValue.{key}")) {
                next.input[index].1 = defaults.input[index].1;
            }
        }
        next
    }

    pub(crate) fn toggle_capability(&mut self, model: &CurrentModel, index: usize) {
        let projected = self.project(model);
        *self = projected;
        self.capabilities[index] = !self.capabilities[index];
        self.overridden
            .insert(format!("{}Value", CAPABILITIES[index]));
    }

    pub(crate) fn toggle_input(&mut self, model: &CurrentModel, key: &str) {
        let projected = self.project(model);
        *self = projected;
        if let Some(entry) = self.input.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = !entry.1;
            self.overridden.insert(format!("inputFormatValue.{key}"));
        }
    }

    pub(crate) fn set_levels(&mut self, model: &CurrentModel, levels: Vec<String>) {
        let projected = self.project(model);
        *self = projected;
        self.levels = levels;
        self.overridden.insert(LEVELS_KEY.into());
    }

    /// Desktop `updateModelDraft` with `useRecommendedConfigValue`; the caller
    /// re-seeds the text inputs from the result.
    pub(crate) fn set_recommended(&mut self, model: &CurrentModel, recommended: bool) {
        if recommended == self.recommended {
            return;
        }
        if recommended {
            // 切回智能配置是显式恢复：清除可编辑覆盖，只保留 ID 与启用状态（desktop restoreModelDraft）。
            let mut restored = Self::from_model(&CurrentModel {
                config: model.inherited.clone().unwrap_or(Value::Object(Map::new())),
                personal: Value::Object(Map::new()),
                inherited: None,
            });
            restored.id = std::mem::take(&mut self.id);
            restored.enabled = self.enabled;
            *self = restored;
            return;
        }
        let projected = self.project(model);
        let baseline = model.baseline();
        *self = projected;
        // 只补空的继承输入；非空的错误输入保留，让保存指出错误。
        if self.context_window.is_empty() {
            self.context_window = at(baseline, &["properties", "contextWindow"])
                .map(number_text)
                .unwrap_or_default();
        }
        if self.max_output.is_empty() {
            self.max_output = at(baseline, &["optionSpecs", "maxOutputTokens", "max"])
                .map(number_text)
                .unwrap_or_default();
        }
        if self.level_map.is_empty() {
            self.level_map = text(baseline, &["optionSpecs", "reasoningLevel", "map"]);
        }
        self.recommended = false;
    }
}

/// Placeholders for the text inputs: baseline values in smart mode only.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Placeholders {
    pub context_window: Option<String>,
    pub max_output: Option<String>,
    pub level_map: Option<String>,
    pub max_output_map: Option<String>,
}

pub(crate) fn placeholders(draft: &ModelDraft, model: &CurrentModel) -> Placeholders {
    let Some(inherited) = model.inherited.as_ref().filter(|_| draft.recommended) else {
        return Placeholders::default();
    };
    let map = |path: &[&str]| Some(text(inherited, path)).filter(|s| !s.is_empty());
    Placeholders {
        context_window: at(inherited, &["properties", "contextWindow"]).map(number_text),
        max_output: at(inherited, &["optionSpecs", "maxOutputTokens", "max"]).map(number_text),
        level_map: map(&["optionSpecs", "reasoningLevel", "map"]),
        max_output_map: map(&["optionSpecs", "maxOutputTokens", "map"]),
    }
}

/// A path lookup that treats `null` like a missing key (JS `?.` / `??`).
pub(crate) fn at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    (!current.is_null()).then_some(current)
}

pub(crate) fn flag(value: &Value, path: &[&str]) -> Option<bool> {
    at(value, path).and_then(Value::as_bool)
}

fn text(value: &Value, path: &[&str]) -> String {
    at(value, path)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number_text(value: &Value) -> String {
    value
        .as_u64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| value.to_string())
}

pub(crate) fn levels(config: &Value) -> Vec<String> {
    at(config, &["optionSpecs", "reasoningLevel", "values"])
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "model_draft_tests.rs"]
mod tests;
