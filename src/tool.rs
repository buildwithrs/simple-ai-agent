use std::{collections::HashMap, sync::Arc};

use openai::chat::ChatCompletionFunctionDefinition;
use serde_json::Value;

use crate::errors::AgentError;

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> String;
    fn decription(&self) -> String;
    fn parammeters_schema(&self) -> Value;
    async fn execute(&self, args: Value) -> Result<String, AgentError>;
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    pub tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name(), Arc::new(tool));
    }

    pub fn get_fn(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn to_openai_func(&self) -> Vec<ChatCompletionFunctionDefinition> {
        self.tools
            .values()
            .map(|f| ChatCompletionFunctionDefinition {
                name: f.name(),
                parameters: Some(f.parammeters_schema()),
                description: Some(f.decription()),
            })
            .collect()
    }

    pub async fn run(&self, name: &str, args: &str) -> Result<String, AgentError> {
        let tool = self
            .get_fn(name)
            .ok_or_else(|| AgentError::ContextError(format!("unknown tool: {name}")))?;
        let args: Value =
            serde_json::from_str(args).map_err(|e| AgentError::ContextError(format!("{}", e)))?;

        tool.execute(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Mock tool that echoes back its received arguments as a JSON string,
    /// letting us assert that the registry forwarded `args` unchanged.
    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> String {
            "echo".to_string()
        }

        fn decription(&self) -> String {
            "echoes back the provided arguments".to_string()
        }

        fn parammeters_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"],
            })
        }

        async fn execute(&self, args: Value) -> Result<String, AgentError> {
            Ok(args.to_string())
        }
    }

    /// Mock tool that always errors — used to verify error propagation
    /// from `execute` through `run` is preserved.
    struct FailingTool;

    /// Second tool reporting the same `name` as `EchoTool` but with a
    /// distinct description — lets us exercise the overwrite path of
    /// `register` deterministically.
    struct EchoToolV2;

    #[async_trait::async_trait]
    impl Tool for EchoToolV2 {
        fn name(&self) -> String {
            "echo".to_string()
        }

        fn decription(&self) -> String {
            "second registration wins".to_string()
        }

        fn parammeters_schema(&self) -> Value {
            json!({ "type": "object" })
        }

        async fn execute(&self, args: Value) -> Result<String, AgentError> {
            Ok(format!("v2:{}", args))
        }
    }

    #[async_trait::async_trait]
    impl Tool for FailingTool {
        fn name(&self) -> String {
            "failing".to_string()
        }

        fn decription(&self) -> String {
            "always fails".to_string()
        }

        fn parammeters_schema(&self) -> Value {
            json!({ "type": "object" })
        }

        async fn execute(&self, _args: Value) -> Result<String, AgentError> {
            Err(AgentError::ContextError("boom".to_string()))
        }
    }

    #[test]
    fn register_inserts_tool_lookupable_by_name() {
        let mut registry = ToolRegistry::default();
        registry.register(EchoTool);

        assert!(
            registry.get_fn("echo").is_some(),
            "echo should be registered"
        );
        assert!(
            registry.get_fn("missing").is_none(),
            "unknown tools should return None"
        );
    }

    #[test]
    fn registering_two_tools_with_distinct_names_keeps_both() {
        let mut registry = ToolRegistry::default();
        registry.register(EchoTool);
        registry.register(FailingTool);

        assert!(registry.get_fn("echo").is_some());
        assert!(registry.get_fn("failing").is_some());
        assert_eq!(registry.tools.len(), 2);
    }

    #[test]
    fn register_with_same_name_overwrites_previous_tool() {
        let mut registry = ToolRegistry::default();
        registry.register(EchoTool);
        registry.register(EchoToolV2);

        // Last write wins — the v1 tool is replaced by v2, but the
        // entry under the same key is still resolvable.
        let stored = registry.get_fn("echo").expect("echo entry present");
        assert_eq!(stored.decription(), "second registration wins");
        assert_eq!(registry.tools.len(), 1);
    }

    #[test]
    fn to_openai_func_emits_one_definition_per_tool() {
        let mut registry = ToolRegistry::default();
        registry.register(EchoTool);
        registry.register(FailingTool);

        let fns = registry.to_openai_func();
        assert_eq!(fns.len(), 2);

        let echo = fns.iter().find(|f| f.name == "echo").expect("echo fn");
        assert_eq!(
            echo.description.as_deref(),
            Some("echoes back the provided arguments")
        );
        assert_eq!(
            echo.parameters.as_ref().unwrap()["required"][0],
            json!("message")
        );

        let failing = fns
            .iter()
            .find(|f| f.name == "failing")
            .expect("failing fn");
        assert_eq!(failing.description.as_deref(), Some("always fails"));
    }

    #[tokio::test]
    async fn run_executes_registered_tool_with_parsed_args() {
        let mut registry = ToolRegistry::default();
        registry.register(EchoTool);

        let out = registry
            .run("echo", r#"{"message":"hello"}"#)
            .await
            .expect("run should succeed");

        // EchoTool serializes the parsed Value back as JSON, so the
        // string content comes from Value::to_string, not raw input.
        let parsed: Value = serde_json::from_str(&out).expect("echo returns JSON");
        assert_eq!(parsed["message"], "hello");
    }

    #[tokio::test]
    async fn run_returns_context_error_for_unknown_tool() {
        let registry = ToolRegistry::default();

        let err = registry
            .run("nope", "{}")
            .await
            .expect_err("unknown tool should error");

        match err {
            AgentError::ContextError(msg) => {
                assert!(msg.contains("unknown tool"), "msg was: {msg}");
                assert!(msg.contains("nope"), "msg should include tool name: {msg}");
            }
            other => panic!("expected ContextError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_returns_context_error_for_invalid_json_args() {
        let mut registry = ToolRegistry::default();
        registry.register(EchoTool);

        let err = registry
            .run("echo", "not-json")
            .await
            .expect_err("bad args should error");

        match err {
            AgentError::ContextError(_) => {}
            other => panic!("expected ContextError, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_propagates_errors_from_tool_execute() {
        let mut registry = ToolRegistry::default();
        registry.register(FailingTool);

        let err = registry
            .run("failing", "{}")
            .await
            .expect_err("failing tool should surface its error");

        match err {
            AgentError::ContextError(msg) => assert_eq!(msg, "boom"),
            other => panic!("expected ContextError, got {other:?}"),
        }
    }
}
