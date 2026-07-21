pub mod context; // agent context builder
pub mod llm; // LLM Client
pub mod errors; // AI Agent Errors
pub mod tool; // Tool Define, Tool Registry
pub mod db; // Postgres DB Related Tools
pub mod config;

pub const SYS_PROMPTS: &'static str = include_str!("../docs/system_prompts.md");