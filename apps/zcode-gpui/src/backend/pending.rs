use crate::backend::workspace::CommandCtx;

pub(crate) enum Pending {
    SubscribeIndex,
    SubscribeConfig,
    /// Legacy `session/create` used purely to harvest the model catalog —
    /// the V4 workspace-config topic carries no model options on standalone
    /// CLI (they are host-provided). The session it creates is deleted right
    /// after (CleanupCatalog).
    ModelCatalog,
    CleanupCatalog,
    SubscribeConversation(String),
    CreateSession(crate::backend::submission::Submission),
    SendText(crate::backend::submission::Submission),
    HeldSend {
        submission: crate::backend::submission::Submission,
        trigger: crate::composer::delivery::SubmitTrigger,
    },
    Stop,
    /// Session-scoped command whose ack is checked (see backend/session_cmds.rs).
    Command(CommandCtx),
    /// Background auto-compaction of a session (app/auto_compact.rs); its
    /// failures go to the log, not the error banner.
    AutoCompact(String),
    /// `v4/conversation/resync` recovery; the ack is a subscribe ack and
    /// may carry a new subscription id / log epoch (review finding 1).
    Resync(String),
    /// `v4/conversation/rowsRange` history page for a session.
    FetchRows(String),
    /// Background freshness probe for the open conversation: a cursorless
    /// rowsRange whose response is only compared for movement (never applied).
    /// Makes sessions hosted by ANOTHER process (deltas never reach our
    /// backend) refresh through the standard resync snapshot.
    PollRows(String),
    /// `v4/usage/stats` query for token metrics and timeline.
    FetchUsageStats(String),
    /// `mcp/list` inspection of connected MCP servers.
    FetchMcpList,
    AttachmentUpload {
        upload: String,
        step: crate::backend::attachment_upload::UploadStep,
    },
    AttachmentAbort,
    SlashCatalog(Option<String>),
    ReferenceCatalog {
        session: Option<String>,
        kind: crate::composer::references::CatalogKind,
    },
    /// `plugins/overview` query for plugin marketplace catalog.
    FetchPluginsOverview,
    /// Mutating plugin operation (`install`, `uninstall`, `update`, `setEnabled`, `restoreBuiltin`, `marketplace/*`).
    PluginAction(String),
    PluginPrompt(crate::backend::plugin_prompt::PluginPrompt),
    PluginDescribe(crate::backend::plugin_detail::PluginDetailIdentity),
    PluginConfig(crate::backend::plugin_config::ConfigRequest),
    SavedWorkflowList(String),
    SavedWorkflowCreate(crate::backend::saved_workflow_cmds::SavedWorkflowLaunch),
    SavedWorkflowStart {
        launch: crate::backend::saved_workflow_cmds::SavedWorkflowLaunch,
        session: String,
    },
    SavedWorkflowCleanup,
    WorkflowDefinition {
        scope: String,
        name: String,
    },
    WorkflowPreflight(crate::backend::workflow_management::WorkflowMutationReceipt),
    WorkflowMutation(crate::backend::workflow_management::WorkflowMutationReceipt),
    WorkflowHistory {
        scope: String,
        name: String,
    },
    WorkflowArtifacts(crate::backend::workflow_artifacts::ArtifactQueryReceipt),
    WorkflowArtifactContent(crate::backend::workflow_artifact_content::ContentReceipt),
    WorkflowSettings {
        session: String,
        run: String,
    },
    QueueEdit(crate::conversation::queue_edit::QueueRestore),
    SubagentDirectory {
        session: String,
        replacing: bool,
    },
    /// GPUI-only `workspace/readSystemPrompt` (true) or `workspace/updateSystemPrompt` (false).
    SystemPrompt {
        read: bool,
    },
    /// GPUI-only `secrets/sync` (backend/secrets_cmds.rs).
    Secrets,
    /// Model settings page (provider_settings/requests.rs).
    ProviderSettings(crate::provider_settings::Request),
    /// Answer forwarded to an async caller (backend/raw_query.rs).
    RawQuery(futures::channel::oneshot::Sender<crate::backend::raw_query::QueryReply>),
}
