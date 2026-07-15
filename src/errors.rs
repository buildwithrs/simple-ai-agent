use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Memory Error: {0}")]
    MemoryError(String),
    #[error("LLM Error: {0}")]
    LlmError(String),
    #[error("Context Error: {0}")]
    ContextError(String),
    #[error("Plan Error: {0}")]
    PlanError(String),
    #[error("State Error: {0}")]
    StateError(String),
}