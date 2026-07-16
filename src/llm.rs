use std::env;

use openai::{
    Credentials,
    chat::{
        ChatCompletion, ChatCompletionChoice, ChatCompletionMessage, ChatCompletionMessageRole,
    },
};

use crate::errors::AgentError;

pub struct LLMClient {
    pub base_url: String,
    pub model: String,
    pub credentials: Credentials,
    pub system_msg: Vec<ChatCompletionMessage>,
    pub user_msgs: Vec<ChatCompletionMessage>,
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
        // You are a helpful assistant.

        Ok(Self::new(
            &base_url,
            &model,
            credentials,
            vec![ChatCompletionMessage {
                role: ChatCompletionMessageRole::System,
                content: Some("You are a helpful assistant.".to_string()),
                ..Default::default()
            }],
        ))
    }

    pub fn new(
        base_url: &str,
        model: &str,
        credentials: Credentials,
        system_msg: Vec<ChatCompletionMessage>,
    ) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            credentials,
            system_msg,

            user_msgs: vec![],
        }
    }

    pub async fn send_message(
        &mut self,
        msg: &str,
    ) -> Result<Vec<ChatCompletionChoice>, AgentError> {
        if self.user_msgs.len() == 0 {
            let mut users_msg = self.system_msg.clone();
            users_msg.push(ChatCompletionMessage {
                role: ChatCompletionMessageRole::User,
                content: Some(msg.to_string()),
                ..Default::default()
            });

            self.user_msgs = users_msg;
        } else {
            self.user_msgs.push(ChatCompletionMessage {
                role: ChatCompletionMessageRole::User,
                content: Some(msg.to_string()),
                ..Default::default()
            });
        }

        let user_msgs = self.user_msgs.clone();

        let chat_completion = ChatCompletion::builder(&self.model, user_msgs)
            .credentials(self.credentials.clone())
            .create()
            .await?;

        Ok(chat_completion.choices)
    }
}
