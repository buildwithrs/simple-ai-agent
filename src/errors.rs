use openai::OpenAiError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Memory Error: {0}")]
    MemoryError(String),

    #[error("client config error: {0}")]
    ClientConfigError(&'static str),

    #[error("llm error: {0}")]
    OpenAIError(#[from] OpenAiError),

    #[error("Context Error: {0}")]
    ContextError(String),
    #[error("Plan Error: {0}")]
    PlanError(String),
    #[error("State Error: {0}")]
    StateError(String),
}