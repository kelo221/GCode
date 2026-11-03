//! Transcript: virtualized chat list, row rendering and scroll behavior
//! (follow-bottom, history anchoring, middle-click autoscroll).

pub(crate) mod chat;
pub(crate) mod edit_diagnostics;
pub(crate) mod image_viewer;
#[cfg(test)]
mod keyboard_scroll_tests;
pub(crate) mod list;
pub(crate) mod message_actions;
pub(crate) mod noise_free;
pub(crate) mod reasoning;
pub(crate) mod scroll;
#[cfg(test)]
mod scroll_tests;
pub(crate) mod speak_text;
pub(crate) mod subagent_card;
#[cfg(test)]
mod subagent_card_tests;
pub(crate) mod tagged_text;
pub(crate) mod tool_card;
#[cfg(test)]
mod tool_card_tests;
pub(crate) mod tool_images;
pub(crate) mod tool_inputs;
pub(crate) mod tool_results;
pub(crate) mod user_row;
