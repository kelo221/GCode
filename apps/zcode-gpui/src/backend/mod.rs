//! Agent backend: process launch per workspace, the V4 NDJSON wire codec,
//! inbound event routing / response correlation and outbound commands.

pub(crate) mod attachment_file;
#[cfg(test)]
mod attachment_stage_tests;
pub(crate) mod attachment_upload;
#[cfg(test)]
mod attachment_upload_capacity_tests;
mod attachment_upload_cmds;
#[cfg(test)]
mod attachment_upload_tests;
#[cfg(test)]
mod attachment_upload_validation_tests;
pub(crate) mod backlog;
pub(crate) mod commands;
pub(crate) mod compiled_backend;
pub(crate) mod conn;
pub(crate) mod events;
pub(crate) mod flow_cmds;
mod goal_submission;
pub(crate) mod inspection;
pub(crate) mod launcher;
pub(crate) mod mcp_authorization;
#[cfg(test)]
mod mcp_authorization_tests;
mod pending;
pub(crate) mod plugin_cmds;
pub(crate) mod plugin_config;
mod plugin_config_cmds;
#[cfg(test)]
mod plugin_config_error_tests;
#[cfg(test)]
mod plugin_config_isolation_tests;
#[cfg(test)]
mod plugin_config_tests;
pub(crate) mod plugin_detail;
#[cfg(test)]
mod plugin_detail_isolation_tests;
#[cfg(test)]
mod plugin_detail_tests;
pub(crate) mod plugin_payloads;
pub(crate) mod plugin_prompt;
#[cfg(test)]
mod plugin_prompt_failure_tests;
#[cfg(test)]
mod plugin_prompt_tests;
pub(crate) mod plugin_source_cmds;
#[cfg(test)]
mod plugin_source_tests;
pub(crate) mod preload;
pub(crate) mod query_cmds;
pub(crate) mod raw_query;
pub(crate) mod reference_cmds;
pub(crate) mod responses;
pub(crate) mod reverse_rpc;
pub(crate) mod route;
pub(crate) mod route_rules;
pub(crate) mod saved_workflow_cmds;
#[cfg(test)]
mod saved_workflow_safety_tests;
#[cfg(test)]
mod saved_workflow_tests;
mod secrets_cmds;
#[cfg(test)]
mod secrets_tests;
mod send_cmds;
pub(crate) mod services_launch;
pub(crate) mod services_rpc;
pub(crate) mod session_cmds;
mod slash_catalog;
#[cfg(test)]
mod slash_submission_tests;
pub(crate) mod status;
pub(crate) mod subagent_cmds;
pub(crate) mod submission;
mod subscription_ack;
mod system_prompt_cmds;
#[cfg(test)]
mod system_prompt_tests;
pub(crate) mod transport_state;
pub(crate) mod wire;
pub(crate) mod workflow_artifact_content;
#[cfg(test)]
mod workflow_artifact_content_isolation_tests;
#[cfg(test)]
mod workflow_artifact_content_tests;
pub(crate) mod workflow_artifacts;
#[cfg(test)]
mod workflow_artifacts_capacity_tests;
#[cfg(test)]
mod workflow_artifacts_tests;
pub(crate) mod workflow_cmds;
pub(crate) mod workflow_definition_cmds;
#[cfg(test)]
mod workflow_definition_tests;
pub(crate) mod workflow_history_cmds;
#[cfg(test)]
mod workflow_history_tests;
pub(crate) mod workflow_management;
mod workflow_management_cmds;
#[cfg(test)]
mod workflow_management_isolation_tests;
#[cfg(test)]
mod workflow_management_tests;
pub(crate) mod workflow_navigation;
pub(crate) mod workflow_settings;
mod workflow_settings_sync;
#[cfg(test)]
mod workflow_settings_sync_tests;
#[cfg(test)]
mod workflow_settings_tests;
pub(crate) mod workspace;
pub(crate) mod workspace_paths;
mod workspace_plugins;
mod workspace_workflows;
