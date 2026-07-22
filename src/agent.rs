use std::sync::Arc;

use async_openai::types::chat::{
    ChatCompletionMessageToolCalls, ChatCompletionRequestAssistantMessageArgs,
    ChatCompletionRequestMessage, ChatCompletionRequestToolMessage,
};
use rustyline::DefaultEditor;
use termimad::MadSkin;

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
                println!("execute tool calls");
                println!("tool_calls: {:?}", tool_calls);

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
                for (handle, tool_call_clone) in handles {
                    if let Ok(response_content) = handle.await {
                        let resp = response_content?;
                        function_responses.push((tool_call_clone, resp));
                    }
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
            }
        }

        Err(AgentError::ExceedMaxIter)
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
