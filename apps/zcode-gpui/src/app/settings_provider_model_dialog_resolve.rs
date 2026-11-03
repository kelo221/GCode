//! Add-model dialog: recommended-config resolution and the agent answers
//! addressed to the dialog (matched by request tag; stale ones are dropped).

use super::DialogError;
use crate::app::root::RootView;
use crate::provider_settings::model_draft::placeholders;
use gpui::Context;

impl RootView {
    /// Sends `resolveModelConfig` for the current ID unless one is in flight
    /// or already accepted. `generation` drops an idle timer the user outran.
    pub(crate) fn request_model_resolution(
        &mut self,
        generation: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.add_model_dialog_mut() else {
            return;
        };
        let id = dialog.draft.id.trim().to_owned();
        if generation.is_some_and(|g| g != dialog.generation)
            || !dialog.pending()
            || dialog.resolving.as_ref().is_some_and(|(_, r)| *r == id)
        {
            return;
        }
        dialog._debounce = None;
        let provider = dialog.provider.clone();
        let editing = dialog.editing.is_some();
        let tag = self.state.update(cx, |s, cx| {
            s.resolve_provider_model_config(&provider, &id, editing, cx)
        });
        if let Some(dialog) = self.add_model_dialog_mut() {
            dialog.resolving = Some((tag, id));
        }
        // 无智能体时回包已同步写入 resolved；立即消费以免保存永远等待。
        self.sync_add_model_dialog(cx);
    }

    /// Consumes the agent's answers addressed to this dialog (by tag).
    pub(crate) fn sync_add_model_dialog(&mut self, cx: &mut Context<Self>) {
        let (resolved, added) = {
            let state = &self.state.read(cx).provider_settings;
            (state.resolved.clone(), state.added.clone())
        };
        let Some(dialog) = self.add_model_dialog_mut() else {
            return;
        };
        let mut save = false;
        if let (Some((tag, outcome)), Some((wanted, id))) = (resolved, dialog.resolving.clone())
            && tag == wanted
        {
            dialog.resolving = None;
            match outcome {
                Ok(inherited) => {
                    dialog.resolution = Some((id, inherited));
                    save = std::mem::take(&mut dialog.save_after_resolve);
                }
                Err(message) => {
                    if std::mem::take(&mut dialog.save_after_resolve) {
                        dialog.error = Some(DialogError::Backend(message));
                    }
                }
            }
            self.refresh_model_placeholders(cx);
        }
        let Some(dialog) = self.add_model_dialog_mut() else {
            return;
        };
        let mut close = false;
        if let (Some((tag, outcome)), Some(wanted)) = (added, dialog.save_tag)
            && tag == wanted
        {
            dialog.save_tag = None;
            match outcome {
                // 以 agent 实际写入成功作为关闭边界；失败时保留输入并在弹窗内展示错误。
                Ok(()) => close = true,
                Err(message) => dialog.error = Some(DialogError::Backend(message)),
            }
        }
        if close && let Some(form) = self.settings.provider_form.as_mut() {
            form.add_model = None;
        }
        if save {
            self.save_add_model_dialog(cx);
        }
        cx.notify();
    }

    pub(crate) fn refresh_model_placeholders(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.add_model_dialog() else {
            return;
        };
        let shown = placeholders(&dialog.draft, &dialog.current());
        for (input, text) in [
            (&dialog.context_window, shown.context_window),
            (&dialog.max_output, shown.max_output),
            (&dialog.level_map, shown.level_map),
            (&dialog.max_output_map, shown.max_output_map),
        ] {
            input.update(cx, |input, cx| {
                input.set_placeholder(text.unwrap_or_default(), cx)
            });
        }
    }
}
