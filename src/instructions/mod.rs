pub mod agents;
pub mod defaults;
pub mod file_writer;
mod schema;
mod template;

pub use file_writer::{write_agents_file, write_instruction_file};
pub use template::load_instruction_with_template;
// Re-export InstructionResult for external use if needed
#[allow(unused_imports)]
pub use template::InstructionResult;
