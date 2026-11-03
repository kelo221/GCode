//! Model dialog edit mode (desktop `mode="edit"`, PROVIDER_SETTINGS.md
//! "Edit-model dialog") and the draft toggles: smart mode, capabilities,
//! input types and the ordered reasoning levels (desktop `updateModelDraft`).

use super::AddModelDialog;
use crate::app::root::RootView;
use crate::provider_settings::model_draft::{CurrentModel, ModelDraft};
use gpui::{Context, Window};
use serde_json::Value;

/// The saved model an edit-mode dialog opened on.
pub(crate) struct EditedModel {
    pub id: String,
    pub effective: Value,
    pub personal: Value,
    /// View revision at open; sent as `basedOnRevision` so a concurrent change
    /// is reported instead of overwritten.
    pub revision: u64,
}

impl EditedModel {
    /// The saved model with the built-in recommendation as its smart baseline.
    pub(crate) fn current(&self, inherited: Option<&Value>) -> CurrentModel {
        CurrentModel {
            config: self.effective.clone(),
            personal: self.personal.clone(),
            inherited: inherited.cloned(),
        }
    }
}

impl RootView {
    /// The model row's settings button: the add dialog, prefilled from the
    /// saved model.
    pub(crate) fn open_edit_model_dialog(
        &mut self,
        provider: &str,
        model: &crate::provider_settings::Model,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = &self.state.read(cx).provider_settings;
        let Some(revision) = state.view.as_ref().map(|v| v.revision) else {
            return;
        };
        let edit = EditedModel {
            id: model.id.clone(),
            effective: model.effective.clone(),
            personal: model.personal.clone(),
            revision,
        };
        let draft = ModelDraft::for_edit(&edit.current(None), &edit.id, model.use_recommended);
        self.open_model_dialog(provider, Some(edit), draft.clone(), window, cx);
        let Some(dialog) = self.add_model_dialog() else {
            return;
        };
        for (input, text) in [
            (&dialog.id, draft.id),
            (&dialog.context_window, draft.context_window),
            (&dialog.max_output, draft.max_output),
            (&dialog.level_map, draft.level_map),
            (&dialog.max_output_map, draft.max_output_map),
        ] {
            input.update(cx, |input, cx| input.set_text(text, cx));
        }
        dialog
            .max_output
            .update(cx, |input, cx| input.set_disabled(false, cx));
        // 内置模型不能改名（Facade 会拒绝）；直接锁定输入，不让用户填了再报错。
        dialog
            .id
            .update(cx, |input, cx| input.set_disabled(model.builtin, cx));
        self.refresh_model_placeholders(cx);
        self.request_model_resolution(None, cx);
    }

    fn edit_model_dialog(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut AddModelDialog, &mut Context<Self>),
    ) {
        let Some(dialog) = self.add_model_dialog_mut() else {
            return;
        };
        if dialog.saving() {
            return;
        }
        edit(dialog, cx);
        if dialog
            .error
            .as_ref()
            .is_some_and(|e| !matches!(e, super::DialogError::Backend(_)))
        {
            dialog.error = None;
        }
        cx.notify();
    }

    pub(crate) fn set_model_dialog_recommended(
        &mut self,
        recommended: bool,
        cx: &mut Context<Self>,
    ) {
        self.edit_model_dialog(cx, |dialog, cx| {
            // 切换模式前的基线：从智能切到固定时用当前推荐补空输入。
            let current = dialog.current();
            dialog.draft.set_recommended(&current, recommended);
            let draft = dialog.draft.clone();
            for (input, text) in [
                (&dialog.context_window, draft.context_window),
                (&dialog.max_output, draft.max_output),
                (&dialog.level_map, draft.level_map),
                (&dialog.max_output_map, draft.max_output_map),
            ] {
                input.update(cx, |input, cx| input.set_text(text, cx));
            }
        });
        self.refresh_model_placeholders(cx);
        // 切回智能模式且尚无该 ID 的推荐时立即解析，不再等空闲计时。
        if recommended {
            self.request_model_resolution(None, cx);
        }
    }

    pub(crate) fn toggle_model_dialog_capability(&mut self, index: usize, cx: &mut Context<Self>) {
        self.edit_model_dialog(cx, |dialog, _| {
            let current = dialog.current();
            dialog.draft.toggle_capability(&current, index);
        });
    }

    pub(crate) fn toggle_model_dialog_input(&mut self, key: &'static str, cx: &mut Context<Self>) {
        self.edit_model_dialog(cx, |dialog, _| {
            let current = dialog.current();
            dialog.draft.toggle_input(&current, key);
        });
    }

    pub(crate) fn toggle_model_dialog_advanced(&mut self, cx: &mut Context<Self>) {
        if let Some(dialog) = self.add_model_dialog_mut() {
            dialog.advanced = !dialog.advanced;
            cx.notify();
        }
    }

    /// Appends the typed level; blanks and duplicates are ignored like the
    /// desktop chip editor.
    pub(crate) fn add_model_level(&mut self, cx: &mut Context<Self>) {
        self.edit_model_dialog(cx, |dialog, cx| {
            let level = dialog.new_level.read(cx).text().trim().to_owned();
            let current = dialog.current();
            let mut levels = dialog.draft.project(&current).levels;
            if level.is_empty() || levels.contains(&level) {
                return;
            }
            levels.push(level);
            dialog.draft.set_levels(&current, levels);
            dialog
                .new_level
                .update(cx, |input, cx| input.set_text("", cx));
        });
    }

    pub(crate) fn remove_model_level(&mut self, index: usize, cx: &mut Context<Self>) {
        self.edit_model_dialog(cx, |dialog, _| {
            let current = dialog.current();
            let mut levels = dialog.draft.project(&current).levels;
            if index < levels.len() {
                levels.remove(index);
                dialog.draft.set_levels(&current, levels);
            }
        });
    }
}
