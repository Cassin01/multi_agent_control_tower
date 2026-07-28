mod claude;
mod detector;
pub(crate) mod tmux;
mod worktree;

pub use claude::{ClaudeManager, READY_MARKER};
pub use detector::ExpertStateDetector;
#[allow(unused_imports)]
pub use tmux::{SessionMetadata, TmuxManager, TmuxSender, TmuxWindowSpawner};
pub use worktree::{WorktreeLaunchResult, WorktreeLaunchState, WorktreeManager};
