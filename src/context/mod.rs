mod expert;
mod shared;
mod store;

pub use expert::{ClaudeSession, ExpertContext, FileAnalysis, Knowledge, TaskHistoryEntry};
pub use shared::{Decision, SharedContext};
pub use store::ContextStore;
