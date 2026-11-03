//! App shell: the root window view composing every slice, the right dock
//! container, and `AppState` (per-workspace agents + mirrored conversations).

pub(crate) mod activity;
#[cfg(test)]
mod activity_tests;
#[cfg(test)]
mod attachment_upload_controls_tests;
mod attachment_upload_view;
pub(crate) mod auto_compact;
#[cfg(test)]
mod clipboard_draft_controls_tests;
#[cfg(test)]
mod composer_controls_tests;
#[cfg(test)]
mod connection_tests;
pub(crate) mod conversation_release;
#[cfg(test)]
mod delivery_controls_tests;
pub(crate) mod dock;
pub(crate) mod draft_greeting;
#[cfg(test)]
mod draft_model_tests;
pub(crate) mod error_banner;
#[cfg(test)]
mod files_image_controls_tests;
pub(crate) mod header;
#[cfg(test)]
mod held_queue_controls_tests;
mod held_queue_view;
mod inspection_view;
pub(crate) mod layout;
pub(crate) mod maintenance;
#[cfg(test)]
mod mcp_authorization_controls_tests;
mod mcp_authorization_view;
pub(crate) mod mcp_pane;
pub(crate) mod mobile_agents;
pub(crate) mod mobile_shell;
pub(crate) mod navigation;
pub(crate) mod os_lifecycle;
pub(crate) mod parts;
pub(crate) mod plugin_cards;
#[cfg(test)]
mod plugin_config_controls_tests;
mod plugin_config_view;
pub(crate) mod plugin_controls;
#[cfg(test)]
mod plugin_controls_tests;
#[cfg(test)]
mod plugin_detail_controls_tests;
mod plugin_detail_view;
pub(crate) mod plugin_pane;
pub(crate) mod plugin_source_input;
pub(crate) mod plugin_sources;
mod profile_cmds;
#[cfg(test)]
mod profile_host_failure_tests;
#[cfg(test)]
mod profile_host_plugin_tests;
#[cfg(test)]
mod profile_host_support;
#[cfg(test)]
mod profile_host_tests;
mod profile_lifecycle;
#[cfg(test)]
mod profile_lifecycle_tests;
mod profile_preflight;
#[cfg(test)]
mod profile_preflight_tests;
mod profile_recovery;
#[cfg(test)]
mod profile_recovery_tests;
mod profile_results;
pub(crate) mod profile_state;
#[cfg(test)]
mod profile_state_tests;
mod project_order;
pub(crate) mod quickpick;
pub(crate) mod quickpick_items;
pub(crate) mod root;
#[cfg(test)]
mod safety_tests;
#[cfg(test)]
mod saved_workflow_controls_tests;
pub(crate) mod saved_workflow_form;
pub(crate) mod saved_workflows_pane;
pub(crate) mod settings;
pub(crate) mod settings_auto_compact;
mod settings_browser;
mod settings_code_search;
#[cfg(test)]
mod settings_code_search_tests;
#[cfg(test)]
mod settings_language_tests;
mod settings_lsp_diagnostics;
#[cfg(test)]
mod settings_lsp_diagnostics_tests;
#[cfg(test)]
mod settings_memory_tests;
mod settings_models;
mod settings_models_catalog;
mod settings_navigation;
#[cfg(test)]
mod settings_navigation_tests;
mod settings_phone;
mod settings_prompt_cache;
mod settings_provider_add;
mod settings_provider_card;
pub(crate) mod settings_provider_form;
pub(crate) mod settings_provider_model_dialog;
#[cfg(test)]
mod settings_provider_model_dialog_tests;
mod settings_provider_model_dialog_view;
mod settings_provider_models;
#[cfg(test)]
mod settings_provider_tests;
mod settings_rows;
pub(crate) mod settings_secrets;
#[cfg(test)]
mod settings_shell_tests;
#[cfg(test)]
mod settings_shortcut_bindings_tests;
pub(crate) mod settings_shortcuts;
mod settings_system_prompt;
#[cfg(test)]
mod settings_system_prompt_tests;
#[cfg(test)]
mod settings_tests;
mod settings_tool_search;
#[cfg(test)]
mod settings_tool_search_tests;
mod settings_voice;
mod shortcut_actions;
mod sidebar_warm;
#[cfg(test)]
mod slash_catalog_controls_tests;
mod stop_button;
pub(crate) mod store;
mod store_workspaces;
pub(crate) mod subagent_profiles;
#[cfg(test)]
mod subagents_controls_tests;
mod subagents_form_cmds;
mod subagents_form_footer;
mod subagents_form_view;
mod subagents_inline;
#[cfg(test)]
mod subagents_inline_tests;
mod subagents_rows;
mod subagents_settings;
mod subagents_setup;
#[cfg(test)]
mod submission_tests;
#[cfg(test)]
mod terminal_selection_controls_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod thinking_selector_controls_tests;
pub(crate) mod usage_pane;
mod workflow_artifact_content_view;
#[cfg(test)]
mod workflow_artifacts_controls_tests;
mod workflow_artifacts_view;
mod workflow_definition_view;
mod workflow_history_view;
#[cfg(test)]
mod workflow_management_controls_tests;
mod workflow_management_view;
#[cfg(test)]
mod workflow_move_controls_tests;
#[cfg(test)]
mod workflow_navigation_controls_tests;
mod workflow_navigation_view;
#[cfg(test)]
mod workflow_settings_controls_tests;
pub(crate) mod workflow_settings_view;
pub(crate) mod workspace_panel;
