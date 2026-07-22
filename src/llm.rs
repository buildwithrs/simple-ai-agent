use std::env;

use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessage,
        ChatCompletionRequestAssistantMessageContent, ChatCompletionRequestMessage,
        ChatCompletionRequestToolMessage, ChatCompletionRequestToolMessageContent,
        ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
        ChatCompletionResponseMessage, ChatCompletionTools, CreateChatCompletionRequestArgs,
    },
};
use serde::Deserialize;

use crate::errors::AgentError;

#[derive(Debug, Deserialize)]
struct CompatibleChatCompletionResponse {
    choices: Vec<CompatibleChatChoice>,
}

#[derive(Debug, Deserialize)]
struct CompatibleChatChoice {
    message: ChatCompletionResponseMessage,
}

pub struct LLMClient {
    pub base_url: String,
    pub model: String,
    pub client: Client<OpenAIConfig>,
}

impl LLMClient {
    pub fn from_env() -> Result<Self, AgentError> {
        let api_key = env::var("OPENAI_API_KEY")
            .map_err(|_| AgentError::ClientConfigError("missing OPENAI_API_KEY"))?;
        let model =
            env::var("MODEL").map_err(|_| AgentError::ClientConfigError("missing MODEL"))?;
        let base_url = env::var("OPENAI_BASE_URL")
            .map_err(|_| AgentError::ClientConfigError("missing OPENAI_BASE_URL"))?;

        let config = OpenAIConfig::new()
            .with_api_key(api_key)
            .with_api_base(base_url.clone());

        let client = Client::with_config(config);
        Ok(Self::new(base_url, &model, client))
    }

    pub fn new(base_url: String, model: &str, client: Client<OpenAIConfig>) -> Self {
        Self {
            base_url,
            model: model.to_string(),
            client,
        }
    }

    pub async fn chat(
        &mut self,
        msgs: &[ChatCompletionRequestMessage],
        tools: Vec<ChatCompletionTools>,
    ) -> Result<ChatCompletionResponseMessage, AgentError> {
        let request = CreateChatCompletionRequestArgs::default()
            .max_completion_tokens(5000u32)
            .model(&self.model)
            .messages(msgs)
            .tools(tools)
            .build()?;

        let response_message = self
            .client
            .chat()
            .create_byot::<_, CompatibleChatCompletionResponse>(request)
            .await?
            .choices
            .first()
            .ok_or_else(|| AgentError::LLMNoChoice)?
            .message
            .clone();

        Ok(response_message)
    }
}

pub fn assistant_with_calls(
    content: String,
    calls: Vec<ChatCompletionMessageToolCalls>,
) -> ChatCompletionRequestMessage {
    let mut msg = ChatCompletionRequestAssistantMessage::default();
    msg.content = Some(ChatCompletionRequestAssistantMessageContent::Text(content));
    msg.tool_calls = Some(calls);

    ChatCompletionRequestMessage::Assistant(msg)
}

pub fn user_message(msg: impl Into<String>) -> ChatCompletionRequestMessage {
    ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
        content: ChatCompletionRequestUserMessageContent::Text(msg.into()),
        name: None,
    })
}

pub fn tool_result(
    tool_id: impl Into<String>,
    content: impl Into<String>,
) -> ChatCompletionRequestMessage {
    ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
        content: ChatCompletionRequestToolMessageContent::Text(content.into()),
        tool_call_id: tool_id.into(),
    })
}

pub fn strip_think(s: &str) -> &str {
    // everything after the first  tag, trimmed
    match s.split_once("</think>") {
        Some((_, rest)) => rest.trim_start(),
        None => s.trim_start(),
    }
}

#[cfg(test)]
mod tests {
    use super::CompatibleChatCompletionResponse;

    #[test]
    fn accepts_provider_specific_service_tier_values() {
        let response = r#"
        {
          "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {
              "role": "assistant",
              "content": null,
              "tool_calls": [{
                "id": "call_123",
                "type": "function",
                "function": {
                  "name": "list_tables",
                  "arguments": "{\"include_views\":true}"
                }
              }]
            }
          }],
          "service_tier": "standard"
        }
        "#;

        let response: CompatibleChatCompletionResponse = serde_json::from_str(response).unwrap();
        let tool_calls = response.choices[0]
            .message
            .tool_calls
            .as_ref()
            .expect("tool calls should be preserved");

        assert_eq!(tool_calls.len(), 1);
    }
}
