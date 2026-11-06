//! Review: git status, per-file diffs, stage / commit / push.

pub(crate) mod agent_branches;
mod agent_branches_view;
pub(crate) mod agent_card;
pub(crate) mod agents_section;
pub(crate) mod controls;
mod ended_agents;
pub(crate) mod git;
pub(crate) mod git_state;
mod git_summary;
mod nested_agents;
pub(crate) mod pane;
pub(crate) mod rows;
