use std::sync::Arc;

use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestToolMessage,
};
use rustyline::DefaultEditor;
use termimad::MadSkin;
use tracing::info;

use crate::{
    config::AgentConfig,
    context::init_system_prompts,
    errors::AgentError::{self},
    llm::{LLMClient, user_message},
    tool::ToolRegistry,
};

const CMD_HIS: &'static str = ".history/history.txt";

pub struct PGAgent {
    pub tool_registry: Arc<ToolRegistry>,
    pub llm_cli: LLMClient,
    pub messages: Vec<ChatCompletionRequestMessage>,
    pub config: AgentConfig,
}

impl PGAgent {
    pub fn new(tool_reg: ToolRegistry, llm_cli: LLMClient) -> Self {
        Self {
            tool_registry: Arc::new(tool_reg),
            llm_cli,
            messages: vec![init_system_prompts()],
            config: AgentConfig::default(),
        }
    }

    pub async fn handle_input(&mut self, msg: &str) -> Result<(), AgentError> {
        self.messages.push(user_message(msg));

        let skin = MadSkin::default();
        for _ in 0..self.config.max_iterations {
            let res = self
                .llm_cli
                .chat(&self.messages, self.tool_registry.to_openai_funcs())
                .await?;

            let tool_calls = res.tool_calls;
            println!("[handle_input] tool_calls: {:?}", tool_calls);

            if tool_calls.is_none() {
                let message = res.content.unwrap();
                skin.write_text(&message)?;
                return Ok(());
            }

            if let Some(tool_calls) = tool_calls {
                self.execute_tool_calls(tool_calls).await?;
            }
        }

        Err(AgentError::ExceedMaxIter)
    }

    async fn execute_tool_calls(
        &mut self,
        tool_calls: Vec<ChatCompletionMessageToolCalls>,
    ) -> Result<(), AgentError> {
        info!("[execute_tool_calls] executing tool calls");
        info!("[execute_tool_calls] tool_calls: {:?}", tool_calls);

        let mut handles = Vec::new();
        for tool_call_enum in tool_calls {
            // Extract the function tool call from the enum
            if let ChatCompletionMessageToolCalls::Function(tool_call) = tool_call_enum {
                let name = tool_call.function.name.clone();
                let args = tool_call.function.arguments.clone();
                let tool_call_clone = tool_call.clone();

                let tool_reg = self.tool_registry.clone();
                let handle = tokio::spawn(async move { tool_reg.call(&name, &args).await });
                handles.push((handle, tool_call_clone));
            }
        }

        let mut function_responses = Vec::new();
        let mut recovered = 0usize;
        for (handle, tool_call_clone) in handles {
            let tool_call_id = tool_call_clone.id.as_str();
            let tool_name = tool_call_clone.function.name.as_str();
            let response_content = match handle.await {
                Ok(Ok(content)) => content,
                Ok(Err(e)) => {
                    recovered += 1;
                    tracing::warn!(
                        tool_call_id = %tool_call_id,
                        tool_name = %tool_name,
                        error = %e,
                        "tool returned error; surfacing as tool result so the model can self-correct"
                    );
                    format!("Error: {e}")
                }
                Err(join_err) => {
                    recovered += 1;
                    tracing::error!(
                        tool_call_id = %tool_call_id,
                        tool_name = %tool_name,
                        error = %join_err,
                        "tool task panicked; surfacing panic as tool result"
                    );
                    format!("Error: tool task panicked: {join_err}")
                }
            };
            function_responses.push((tool_call_clone, response_content));
        }

        if recovered > 0 {
            tracing::debug!(
                tool_count = function_responses.len(),
                recovered_errors = recovered,
                "tool dispatch complete with recovered errors; LLM will see them as tool results"
            );
        }

        // Convert ChatCompletionMessageToolCall to ChatCompletionMessageToolCalls enum
        let tool_calls: Vec<ChatCompletionMessageToolCalls> = function_responses
            .iter()
            .map(|(tool_call, _response_content)| {
                ChatCompletionMessageToolCalls::Function(tool_call.clone())
            })
            .collect();

        let assistant_messages: ChatCompletionRequestMessage =
            ChatCompletionRequestAssistantMessageArgs::default()
                .tool_calls(tool_calls)
                .build()?
                .into();

        let tool_messages: Vec<ChatCompletionRequestMessage> = function_responses
            .iter()
            .map(|(tool_call, response_content)| {
                ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                    content: response_content.to_string().into(),
                    tool_call_id: tool_call.id.clone(),
                })
            })
            .collect();

        self.messages.push(assistant_messages);
        self.messages.extend(tool_messages);

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), AgentError> {
        let mut rl = DefaultEditor::new()?;
        rl.load_history(CMD_HIS)?;

        loop {
            let readline = rl.readline(">> ");
            match readline {
                Ok(line) => {
                    rl.add_history_entry(line.as_str())?;
                    self.handle_input(&line).await?;
                }

                Err(rustyline::error::ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }

                Err(rustyline::error::ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }

        rl.save_history(CMD_HIS).ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;
    use async_openai::types::chat::{
        ChatCompletionMessageToolCall, ChatCompletionRequestToolMessageContent, FunctionCall,
    };
    use async_trait::async_trait;
    use serde_json::{Value, json};

    struct FailingTool;

    #[async_trait]
    impl Tool for FailingTool {
        fn name(&self) -> String {
            "failing_tool".to_string()
        }

        fn description(&self) -> String {
            "always fails".to_string()
        }

        fn parammeters_schema(&self) -> Value {
            json!({})
        }

        async fn execute(&self, _args: Value) -> Result<String, AgentError> {
            Err(AgentError::ToolError(
                "aggregate function calls cannot be nested".into(),
            ))
        }
    }

    fn make_call(id: &str, name: &str) -> ChatCompletionMessageToolCalls {
        ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
            id: id.to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: "{}".to_string(),
            },
        })
    }

    fn build_agent() -> PGAgent {
        let mut registry = ToolRegistry::new();
        registry.register(FailingTool);
        let llm = LLMClient::new(
            "http://localhost".to_string(),
            "test-model",
            async_openai::Client::new(),
        );
        PGAgent::new(registry, llm)
    }

    #[tokio::test]
    async fn execute_tool_calls_returns_errors_as_tool_results() {
        let mut agent = build_agent();
        let calls = vec![make_call("call_test", "failing_tool")];

        let result = agent.execute_tool_calls(calls).await;
        assert!(
            result.is_ok(),
            "execute_tool_calls must not propagate AgentError; got {:?}",
            result.err()
        );

        let tool_msg = agent
            .messages
            .iter()
            .find_map(|m| match m {
                ChatCompletionRequestMessage::Tool(t) => Some(t),
                _ => None,
            })
            .expect("expected a tool result message in the conversation");

        assert_eq!(tool_msg.tool_call_id, "call_test");

        let content = match &tool_msg.content {
            ChatCompletionRequestToolMessageContent::Text(t) => t.clone(),
            other => panic!("expected text content, got {:?}", other),
        };
        assert!(
            content.contains("aggregate function calls cannot be nested"),
            "expected original SQL error in tool result, got: {content}"
        );
    }

    #[tokio::test]
    async fn execute_tool_calls_handles_multiple_calls_with_mixed_outcomes() {
        let mut agent = build_agent();
        let calls = vec![
            make_call("call_a", "failing_tool"),
            make_call("call_b", "failing_tool"),
        ];

        let result = agent.execute_tool_calls(calls).await;
        assert!(result.is_ok());

        let tool_msgs: Vec<_> = agent
            .messages
            .iter()
            .filter_map(|m| match m {
                ChatCompletionRequestMessage::Tool(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(
            tool_msgs.len(),
            2,
            "every tool_call_id must produce a tool message"
        );

        let ids: std::collections::HashSet<_> =
            tool_msgs.iter().map(|t| t.tool_call_id.clone()).collect();
        assert!(ids.contains("call_a"));
        assert!(ids.contains("call_b"));
    }
}
