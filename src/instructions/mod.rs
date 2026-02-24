pub mod agents;
pub mod defaults;
pub mod file_writer;
pub mod manifest;
mod schema;
mod template;

pub use file_writer::{
    generate_hooks_settings, write_agents_file, write_instruction_file, write_settings_file,
};
pub use template::load_instruction_with_template;
// Re-export InstructionResult for external use if needed
#[allow(unused_imports)]
pub use template::InstructionResult;
