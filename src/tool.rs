use std::{collections::HashMap, sync::Arc};

use async_openai::types::chat::{ChatCompletionTool, ChatCompletionTools, FunctionObjectArgs};
use serde_json::Value;

use crate::errors::AgentError;

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    fn description(&self) -> String;
    fn parammeters_schema(&self) -> Value;
    async fn execute(&self, args: Value) -> Result<String, AgentError>;
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    pub tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name(), Arc::new(tool));
    }

    pub fn get_fn(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn to_openai_funcs(&self) -> Vec<ChatCompletionTools> {
        self.tools
            .values()
            .map(|f| {
                ChatCompletionTools::Function(ChatCompletionTool {
                    function: FunctionObjectArgs::default()
                        .name(f.name())
                        .description(f.description())
                        .parameters(f.parammeters_schema())
                        .strict(true)
                        .build()
                        .expect("failed to build function object"),
                })
            })
            .collect()
    }

    pub async fn call(&self, name: &str, args: &str) -> Result<String, AgentError> {
        let tool = self
            .get_fn(name)
            .ok_or_else(|| AgentError::ContextError(format!("unknown tool: {name}")))?;
        let args: Value =
            serde_json::from_str(args).map_err(|e| AgentError::ContextError(format!("{}", e)))?;

        tool.execute(args).await
    }
}
