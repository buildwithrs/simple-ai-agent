use openai::OpenAiError;
use rustyline::error::ReadlineError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("read line error: {0}")]
    ReadLineError(#[from] ReadlineError),

    #[error("terminal error: {0}")]
    TerminalError(#[from] termimad::Error),

    #[error("Memory Error: {0}")]
    MemoryError(String),

    #[error("client config error: {0}")]
    ClientConfigError(&'static str),

    #[error("no choice from llm response")]
    LLMNoChoice,

    #[error("llm error: {0}")]
    OpenAIError(#[from] OpenAiError),

    #[error("Context Error: {0}")]
    ContextError(String),

    #[error("Tool Error: {0}")]
    ToolError(String),

    #[error("Plan Error: {0}")]
    PlanError(String),
    #[error("State Error: {0}")]
    StateError(String),
}
