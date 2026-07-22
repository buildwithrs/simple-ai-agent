use openai::chat::{ChatCompletionMessage, ChatCompletionMessageRole};


const SYSTEM_PROMPT: &'static str = include_str!("../docs/system_prompts.md");


pub fn init_system_prompts() -> ChatCompletionMessage {
    let mut msg = ChatCompletionMessage::default();
    msg.role = ChatCompletionMessageRole::System;
    msg.content = Some(SYSTEM_PROMPT.to_string());
    
    msg
}