//! Add-model dialog state (desktop `ProviderModelMetadataDialog`, add mode;
//! see PROVIDER_SETTINGS.md). The draft and inputs live here; the agent owns
//! recommended configs (`resolveModelConfig`) and the write (`addModel`).

use crate::app::root::RootView;
use crate::provider_settings::model_draft::{CurrentModel, DraftField, ModelDraft, commit};
use crate::shared::i18n::label;
use ely_gpui_component::forms::{InputEvent, TextInput};
use gpui::{AppContext, Context, Entity, Subscription, Task, Window};
use serde_json::{Value, json};
use std::time::Duration;

/// Desktop `useIdleTrigger`: resolve once the ID stops changing.
pub(crate) const RESOLVE_IDLE: Duration = Duration::from_millis(1200);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DialogInput {
    Id,
    ContextWindow,
    MaxOutput,
    LevelMap,
    MaxOutputMap,
    NewLevel,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DialogError {
    Field(DraftField),
    Duplicate,
    Backend(String),
}

impl DialogError {
    pub(crate) fn message(&self) -> String {
        let text = match self {
            Self::Field(DraftField::Id) => label("Model ID is required", "模型 ID 不能为空"),
            Self::Field(DraftField::ContextWindow) => label(
                "Context window must be a positive integer",
                "上下文窗口必须是正整数",
            ),
            Self::Field(DraftField::MaxOutputTokens) => label(
                "Max output tokens must be a positive integer",
                "最大输出 Token 必须是正整数",
            ),
            Self::Field(DraftField::InputFormat) => {
                label("Text input type is required", "输入类型必须包含文本")
            }
            Self::Field(DraftField::ReasoningLevelValues) => label(
                "Reasoning levels cannot be empty or duplicated",
                "推理档位不能为空或重复",
            ),
            Self::Field(DraftField::ReasoningLevelMap) => {
                label("Reasoning parameter mapping is invalid", "推理参数映射无效")
            }
            Self::Duplicate => label(
                "This provider already has a model with this ID",
                "该提供商已存在相同 ID 的模型",
            ),
            Self::Backend(message) => return message.clone(),
        };
        text.to_owned()
    }

    /// Errors in the collapsible part open it, like desktop `ModelEditorAdvanced`.
    fn in_advanced(&self) -> bool {
        matches!(
            self,
            Self::Field(
                DraftField::InputFormat
                    | DraftField::ReasoningLevelValues
                    | DraftField::ReasoningLevelMap
            )
        )
    }
}

pub(crate) use edit::EditedModel;

pub(crate) struct AddModelDialog {
    pub provider: String,
    pub editing: Option<EditedModel>,
    /// Element id suffix: a reopened dialog starts with fresh focus state.
    pub instance: u64,
    pub draft: ModelDraft,
    pub id: Entity<TextInput>,
    pub context_window: Entity<TextInput>,
    pub max_output: Entity<TextInput>,
    pub level_map: Entity<TextInput>,
    pub max_output_map: Entity<TextInput>,
    pub new_level: Entity<TextInput>,
    /// (model id, inheritedConfig) of the latest accepted resolution.
    resolution: Option<(String, Value)>,
    /// In-flight resolve: (tag, model id). Responses with another tag are stale.
    resolving: Option<(u64, String)>,
    save_tag: Option<u64>,
    save_after_resolve: bool,
    pub advanced: bool,
    pub error: Option<DialogError>,
    generation: u64,
    _debounce: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl AddModelDialog {
    pub(crate) fn saving(&self) -> bool {
        self.save_tag.is_some() || self.save_after_resolve
    }

    /// Smart mode is waiting for the recommended config of the current ID.
    pub(crate) fn pending(&self) -> bool {
        let id = self.draft.id.trim();
        self.draft.recommended && !id.is_empty() && self.resolved_for(id).is_none()
    }

    fn resolved_for(&self, id: &str) -> Option<&Value> {
        self.resolution
            .as_ref()
            .filter(|(resolved, _)| resolved == id)
            .map(|(_, config)| config)
    }

    /// The model the draft edits; the baseline only counts in smart mode.
    pub(crate) fn current(&self) -> CurrentModel {
        let id = self.draft.id.trim();
        let resolved = self.resolved_for(id).filter(|_| self.draft.recommended);
        match &self.editing {
            Some(edit) => edit.current(resolved),
            None => CurrentModel::for_add(resolved),
        }
    }

    /// What the controls show: untouched ones follow the baseline.
    pub(crate) fn shown(&self) -> ModelDraft {
        self.draft.project(&self.current())
    }
}

fn make_input(
    kind: DialogInput,
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> (Entity<TextInput>, Subscription) {
    let input = cx.new(|cx| {
        let input = TextInput::new(window, cx);
        match kind {
            DialogInput::Id => input.placeholder(label("Model ID", "模型 ID")),
            DialogInput::ContextWindow | DialogInput::MaxOutput => {
                input.filter(|c| c.is_ascii_digit())
            }
            DialogInput::LevelMap | DialogInput::MaxOutputMap => input.multi_line(2, 5),
            DialogInput::NewLevel => input.placeholder(label("Add level", "添加等级")),
        }
    });
    let subscription = cx.subscribe(&input, move |this: &mut RootView, _, event, cx| {
        this.on_model_dialog_input(kind, *event, cx)
    });
    (input, subscription)
}

impl RootView {
    pub(crate) fn add_model_dialog(&self) -> Option<&AddModelDialog> {
        self.settings.provider_form.as_ref()?.add_model.as_ref()
    }

    fn add_model_dialog_mut(&mut self) -> Option<&mut AddModelDialog> {
        self.settings.provider_form.as_mut()?.add_model.as_mut()
    }

    pub(crate) fn open_add_model_dialog(
        &mut self,
        provider: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = ModelDraft::from_model(&CurrentModel::for_add(None));
        self.open_model_dialog(provider, None, draft, window, cx);
    }

    /// Add and edit share one dialog; `editing` is the saved model in edit mode.
    fn open_model_dialog(
        &mut self,
        provider: &str,
        editing: Option<EditedModel>,
        draft: ModelDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.settings.provider_form.as_ref() else {
            return;
        };
        let instance = form.dialogs_opened + 1;
        let mut subscriptions = Vec::new();
        let mut input = |kind| {
            let (input, subscription) = make_input(kind, window, cx);
            subscriptions.push(subscription);
            input
        };
        let (id, context_window, max_output) = (
            input(DialogInput::Id),
            input(DialogInput::ContextWindow),
            input(DialogInput::MaxOutput),
        );
        let (level_map, max_output_map, new_level) = (
            input(DialogInput::LevelMap),
            input(DialogInput::MaxOutputMap),
            input(DialogInput::NewLevel),
        );
        max_output.update(cx, |input, cx| input.set_disabled(true, cx));
        subscriptions.push(cx.observe(&self.state, |this, _, cx| this.sync_add_model_dialog(cx)));
        let dialog = AddModelDialog {
            provider: provider.to_owned(),
            editing,
            instance,
            draft,
            id,
            context_window,
            max_output,
            level_map,
            max_output_map,
            new_level,
            resolution: None,
            resolving: None,
            save_tag: None,
            save_after_resolve: false,
            advanced: false,
            error: None,
            generation: 0,
            _debounce: None,
            _subscriptions: subscriptions,
        };
        let form = self.settings.provider_form.as_mut().unwrap();
        form.dialogs_opened = instance;
        form.add_model = Some(dialog);
        cx.notify();
    }

    /// Cancel / Escape. A save in flight finishes first (desktop ignores the
    /// close while saving so the old request cannot end a new draft).
    pub(crate) fn close_add_model_dialog(&mut self, cx: &mut Context<Self>) {
        if let Some(form) = self.settings.provider_form.as_mut()
            && form.add_model.as_ref().is_some_and(|d| !d.saving())
        {
            form.add_model = None;
            cx.notify();
        }
    }

    fn on_model_dialog_input(
        &mut self,
        kind: DialogInput,
        event: InputEvent,
        cx: &mut Context<Self>,
    ) {
        match (kind, event) {
            (DialogInput::NewLevel, InputEvent::Submit) => self.add_model_level(cx),
            (_, InputEvent::Submit) => self.save_add_model_dialog(cx),
            (DialogInput::Id, InputEvent::Blur) => self.request_model_resolution(None, cx),
            (_, InputEvent::Changed) => self.sync_model_dialog_text(kind == DialogInput::Id, cx),
            _ => {}
        }
    }

    /// Copies the text inputs into the draft; an ID change restarts the idle
    /// timer and drops the in-flight resolve.
    fn sync_model_dialog_text(&mut self, id_changed: bool, cx: &mut Context<Self>) {
        let Some(dialog) = self.add_model_dialog_mut() else {
            return;
        };
        let read = |input: &Entity<TextInput>| input.read(cx).text().to_owned();
        let id = read(&dialog.id);
        let changed = id_changed && dialog.draft.id != id;
        dialog.draft.id = id;
        dialog.draft.context_window = read(&dialog.context_window);
        dialog.draft.max_output = read(&dialog.max_output);
        dialog.draft.level_map = read(&dialog.level_map);
        dialog.draft.max_output_map = read(&dialog.max_output_map);
        if dialog
            .error
            .as_ref()
            .is_some_and(|e| !matches!(e, DialogError::Backend(_)))
            || changed
        {
            dialog.error = None;
        }
        if changed {
            let empty = dialog.draft.id.trim().is_empty();
            dialog
                .max_output
                .update(cx, |input, cx| input.set_disabled(empty, cx));
            dialog.generation += 1;
            dialog.resolving = None;
            dialog._debounce = None;
            let generation = dialog.generation;
            if dialog.pending() {
                dialog._debounce = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(RESOLVE_IDLE).await;
                    let _ = this.update(cx, |this, cx| {
                        this.request_model_resolution(Some(generation), cx)
                    });
                }));
            }
            self.refresh_model_placeholders(cx);
        }
        cx.notify();
    }

    pub(crate) fn save_add_model_dialog(&mut self, cx: &mut Context<Self>) {
        let models: Vec<String> = self
            .state
            .read(cx)
            .provider_settings
            .view
            .as_ref()
            .and_then(|view| {
                self.add_model_dialog()
                    .and_then(|d| view.provider(&d.provider))
            })
            .map(|p| p.models.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default();
        let Some(dialog) = self.add_model_dialog_mut() else {
            return;
        };
        if dialog.saving() {
            return;
        }
        let id = dialog.draft.id.trim().to_owned();
        let own_id = dialog.editing.as_ref().map(|e| e.id.as_str());
        let error = if id.is_empty() {
            Some(DialogError::Field(DraftField::Id))
        } else if models.contains(&id) && own_id != Some(id.as_str()) {
            Some(DialogError::Duplicate)
        } else {
            None
        };
        if let Some(error) = error {
            dialog.error = Some(error);
            cx.notify();
            return;
        }
        if dialog.pending() {
            // 智能模式提交必须基于当前 ID 的推荐基线；先解析，回包后继续保存。
            dialog.save_after_resolve = true;
            dialog.error = None;
            self.request_model_resolution(None, cx);
            cx.notify();
            return;
        }
        let current = dialog.current();
        let result = commit(&dialog.draft.project(&current), &current);
        match result {
            Err(field) => {
                let error = DialogError::Field(field);
                dialog.advanced |= error.in_advanced();
                dialog.error = Some(error);
            }
            Ok(saved) => {
                dialog.error = None;
                let operation = match &dialog.editing {
                    Some(edit) => json!({
                        "type": "updateModel",
                        "providerId": dialog.provider,
                        "modelId": edit.id,
                        "nextModelId": saved.model_id,
                        "config": saved.config,
                        "useRecommendedConfig": saved.use_recommended,
                        "basedOnRevision": edit.revision,
                    }),
                    None => json!({
                        "type": "addModel",
                        "providerId": dialog.provider,
                        "modelId": saved.model_id,
                        "config": saved.config,
                        "useRecommendedConfig": saved.use_recommended,
                    }),
                };
                let tag = self
                    .state
                    .update(cx, |s, cx| s.mutate_provider_settings_tagged(operation, cx));
                if let Some(dialog) = self.add_model_dialog_mut() {
                    dialog.save_tag = Some(tag);
                }
                // 无智能体时失败已同步写入 added。
                self.sync_add_model_dialog(cx);
            }
        }
        cx.notify();
    }
}

#[path = "settings_provider_model_dialog_edit.rs"]
mod edit;
#[path = "settings_provider_model_dialog_resolve.rs"]
mod resolve;
