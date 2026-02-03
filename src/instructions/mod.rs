pub mod defaults;
mod schema;
mod template;

pub use template::load_instruction_with_template;
// Re-export InstructionResult for external use if needed
#[allow(unused_imports)]
pub use template::InstructionResult;
