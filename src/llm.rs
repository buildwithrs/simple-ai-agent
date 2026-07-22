use std::env;

use openai::{
    Credentials,
    chat::{
        ChatCompletion, ChatCompletionFunctionDefinition, ChatCompletionMessage,
        ChatCompletionMessageRole::{Tool as ToolRole, User},
    },
};

use crate::errors::AgentError;

pub struct LLMClient {
    pub base_url: String,
    pub model: String,
    pub credentials: Credentials,
}

impl LLMClient {
    pub fn from_env() -> Result<Self, AgentError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AgentError::ClientConfigError("missing OPENAI_API_KEY"))?;
        let model =
            env::var("MODEL").map_err(|_| AgentError::ClientConfigError("missing MODEL"))?;
        let base_url = env::var("OPENAI_BASE_URL")
            .map_err(|_| AgentError::ClientConfigError("missing OPENAI_BASE_URL"))?;

        let credentials = Credentials::new(api_key, base_url.clone());
        Ok(Self::new(&base_url, &model, credentials))
    }

    pub fn new(base_url: &str, model: &str, credentials: Credentials) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            credentials,
        }
    }

    pub async fn chat(
        &mut self,
        msgs: &[ChatCompletionMessage],
        funcs: &[ChatCompletionFunctionDefinition],
    ) -> Result<ChatCompletionMessage, AgentError> {
        let chat_completion = ChatCompletion::builder(&self.model, msgs)
            .credentials(self.credentials.clone())
            .functions(funcs)
            .create()
            .await?;

        let choice = chat_completion
            .choices
            .into_iter()
            .next()
            .ok_or(AgentError::LLMNoChoice)?;
        Ok(choice.message)
    }
}

pub fn to_chat_message(msg: &str) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: User,
        content: Some(msg.to_string()),
        name: None,
        function_call: None,
        tool_call_id: None,
        tool_calls: None,
    }
}

pub fn tool_result(tool_id: impl Into<String>, content: impl Into<String>) -> ChatCompletionMessage {
    ChatCompletionMessage {
        role: ToolRole,
        content: Some(content.into()),
        name: None,
        function_call: None,
        tool_call_id: Some(tool_id.into()),
        tool_calls: None,
    }
}

pub fn strip_think(s: &str) -> &str {
    // everything after the first  tag, trimmed
    match s.split_once("</think>") {
        Some((_, rest)) => rest.trim_start(),
        None => s.trim_start(),
    }
}
