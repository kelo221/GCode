//! Model settings page state: a cache of the agent's `providerSettings/read`
//! view (see PROVIDER_SETTINGS.md). The agent owns provider_config.json; every
//! mutation response replaces `view` wholesale.

use serde_json::Value;
use std::collections::HashMap;

pub(crate) mod model_draft;
mod model_draft_commit;
pub(crate) mod requests;

const CUSTOM_GROUP: &str = "standard-personal";
pub(crate) const API_FORMATS: [&str; 3] = [
    "anthropic-messages",
    "openai-chat-completions",
    "openai-responses",
];

/// One in-flight page request; the response settles it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Request {
    Read,
    /// `tag` marks a mutation whose outcome a dialog waits for (`added`).
    Mutate {
        tag: Option<u64>,
    },
    /// `providerSettings/resolveModelConfig` for the add-model dialog.
    Resolve {
        tag: u64,
    },
    Test {
        provider: String,
        model: String,
    },
    /// `provider/refreshRegistry` on a workspace agent that did not perform the
    /// mutation; its catalog is re-probed once the registry re-read the file.
    RefreshRegistry,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TestStatus {
    Running,
    Passed,
    Failed(String),
}

#[derive(Default)]
pub(crate) struct ProviderSettingsState {
    pub view: Option<ProviderView>,
    pub loading: bool,
    pub busy: bool,
    /// Field commits made while a mutation is in flight (e.g. blur of one field
    /// right after another); sent in order so no edit is lost.
    pub queued: std::collections::VecDeque<(Value, Option<u64>)>,
    /// Source of `Request` tags for dialog-owned requests.
    pub next_tag: u64,
    /// Latest settled `resolveModelConfig`: (tag, inheritedConfig or error).
    pub resolved: Option<(u64, Result<Value, String>)>,
    /// Latest settled tagged mutation: (tag, success or error message).
    pub added: Option<(u64, Result<(), String>)>,
    /// Agent predates `providerSettings/*` (-32601): fall back to the catalog list.
    pub unavailable: bool,
    pub error: Option<String>,
    pub selected: Option<String>,
    pub adding: bool,
    pub confirm_delete: Option<String>,
    pub tests: HashMap<(String, String), TestStatus>,
}

impl ProviderSettingsState {
    /// The selected provider, falling back to the first listed one.
    pub(crate) fn selected_provider(&self) -> Option<&Provider> {
        let view = self.view.as_ref()?;
        self.selected
            .as_deref()
            .and_then(|id| view.provider(id))
            .or_else(|| view.listed().into_iter().next())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Template {
    pub id: String,
    pub name_en: String,
    pub name_zh: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    pub id: String,
    pub builtin: bool,
    pub enabled: bool,
    pub executable: bool,
    pub context_window: Option<u64>,
    pub vision: bool,
    pub issues: Vec<String>,
    /// What the edit-model dialog starts from: `effectiveConfig`,
    /// `personalExactConfig` (empty object when absent) and the smart switch.
    pub effective: Value,
    pub personal: Value,
    pub use_recommended: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Provider {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub executable: bool,
    pub group: String,
    pub access_type: Option<String>,
    pub api_key: String,
    pub api_type: String,
    pub base_url: String,
    pub issues: Vec<String>,
    pub models: Vec<Model>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Status {
    Disabled,
    Unavailable,
    Ready,
}

impl Provider {
    pub(crate) fn custom(&self) -> bool {
        self.group == CUSTOM_GROUP
    }

    pub(crate) fn uses_api_key(&self) -> bool {
        matches!(
            self.access_type.as_deref(),
            Some("api-key" | "zhipu-coding-plan-api-key")
        )
    }

    pub(crate) fn account(&self) -> bool {
        self.access_type.as_deref() == Some("zhipu-account")
    }

    pub(crate) fn status(&self) -> Status {
        if !self.enabled {
            Status::Disabled
        } else if !self.executable || !self.issues.is_empty() {
            Status::Unavailable
        } else {
            Status::Ready
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProviderView {
    pub revision: u64,
    pub templates: Vec<Template>,
    pub order: Vec<String>,
    pub providers: Vec<Provider>,
}

impl ProviderView {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            revision: value.get("revision")?.as_u64()?,
            templates: array(value, "providerTemplates")
                .filter_map(template)
                .collect(),
            order: array(value, "providerOrder")
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            providers: array(value, "providers").filter_map(provider).collect(),
        })
    }

    pub(crate) fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub(crate) fn presets(&self) -> Vec<&Provider> {
        self.providers.iter().filter(|p| !p.custom()).collect()
    }

    /// Custom providers in `providerOrder`; unordered ones keep view order after.
    pub(crate) fn customs(&self) -> Vec<&Provider> {
        let mut customs: Vec<&Provider> = self.providers.iter().filter(|p| p.custom()).collect();
        customs.sort_by_key(|p| {
            self.order
                .iter()
                .position(|id| *id == p.id)
                .unwrap_or(usize::MAX)
        });
        customs
    }

    /// Navigation order: presets, then customs.
    pub(crate) fn listed(&self) -> Vec<&Provider> {
        let mut listed = self.presets();
        listed.extend(self.customs());
        listed
    }

    /// Custom provider ids after moving `id` one step (`up` or down).
    pub(crate) fn moved_custom_order(&self, id: &str, up: bool) -> Option<Vec<String>> {
        let mut ids: Vec<String> = self.customs().iter().map(|p| p.id.clone()).collect();
        moved(&mut ids, id, up).then_some(ids)
    }
}

/// Swap `id` with its neighbour; false when it is already at that edge.
pub(crate) fn moved(ids: &mut [String], id: &str, up: bool) -> bool {
    let Some(index) = ids.iter().position(|item| item == id) else {
        return false;
    };
    let target = if up {
        index.checked_sub(1)
    } else {
        Some(index + 1)
    };
    match target.filter(|t| *t < ids.len()) {
        Some(target) => {
            ids.swap(index, target);
            true
        }
        None => false,
    }
}

/// `200000` → `200K`, `1000000` → `1M`.
pub(crate) fn compact_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 && tokens.is_multiple_of(100_000) {
        let millions = tokens as f64 / 1_000_000.0;
        format!("{}M", trim_float(millions))
    } else if tokens >= 1_000 {
        format!("{}K", trim_float(tokens as f64 / 1_000.0))
    } else {
        tokens.to_string()
    }
}

fn trim_float(value: f64) -> String {
    let text = format!("{value:.1}");
    text.trim_end_matches(".0").to_owned()
}

fn array<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn text(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_str().map(str::to_owned)
}

fn issues(value: &Value) -> Vec<String> {
    array(value, "issues")
        .filter_map(|issue| issue.get("message").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn template(value: &Value) -> Option<Template> {
    let id = text(value, &["templateId"])?;
    let en = text(value, &["templateNameMap", "en-US"]);
    let zh = text(value, &["templateNameMap", "zh-CN"]);
    Some(Template {
        name_en: en
            .clone()
            .or_else(|| zh.clone())
            .unwrap_or_else(|| id.clone()),
        name_zh: zh.or(en).unwrap_or_else(|| id.clone()),
        id,
    })
}

fn model(value: &Value) -> Option<Model> {
    let config = value.get("effectiveConfig");
    let properties = config.and_then(|c| c.get("properties"));
    Some(Model {
        id: text(value, &["modelId"])?,
        builtin: value.get("builtin").and_then(Value::as_bool) == Some(true),
        enabled: value.get("enabled").and_then(Value::as_bool) != Some(false),
        executable: value.get("executable").and_then(Value::as_bool) == Some(true),
        context_window: properties
            .and_then(|p| p.get("contextWindow"))
            .and_then(Value::as_u64),
        vision: properties
            .and_then(|p| p.get("inputFormat"))
            .and_then(|f| f.get("supportsImage"))
            .and_then(Value::as_bool)
            == Some(true),
        issues: issues(value),
        effective: config.cloned().unwrap_or(Value::Object(Default::default())),
        personal: value
            .get("personalExactConfig")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or(Value::Object(Default::default())),
        use_recommended: value.get("useRecommendedConfig").and_then(Value::as_bool) != Some(false),
    })
}

fn provider(value: &Value) -> Option<Provider> {
    let id = text(value, &["providerId"])?;
    let config = value.get("effectiveConfig").cloned().unwrap_or(Value::Null);
    Some(Provider {
        name: text(value, &["providerName"]).unwrap_or_else(|| id.clone()),
        enabled: value.get("enabled").and_then(Value::as_bool) != Some(false),
        executable: value.get("executable").and_then(Value::as_bool) == Some(true),
        group: text(&config, &["group"]).unwrap_or_default(),
        access_type: text(&config, &["access", "type"]),
        api_key: text(&config, &["access", "apiKey"]).unwrap_or_default(),
        api_type: text(&config, &["api", "type"]).unwrap_or_else(|| API_FORMATS[0].into()),
        base_url: text(&config, &["api", "baseUrl"]).unwrap_or_default(),
        issues: issues(value),
        models: array(value, "models").filter_map(model).collect(),
        id,
    })
}

#[cfg(test)]
#[path = "view_tests.rs"]
pub(crate) mod tests;
