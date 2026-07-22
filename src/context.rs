use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent,
};

const SYSTEM_PROMPT: &'static str = include_str!("../docs/system_prompts.md");

pub fn init_system_prompts() -> ChatCompletionRequestMessage {
    ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
        content: ChatCompletionRequestSystemMessageContent::Text(SYSTEM_PROMPT.to_string()),
        name: None,
    })
}
