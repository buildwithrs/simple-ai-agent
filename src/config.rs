#[derive(Debug)]
pub struct AgentConfig {
    pub databases: Vec<String>, // can config multiple databases
    pub state_path: String,     // where to store the agent state
    pub max_iterations: u32,
    pub max_rows: u32,
}

const STATE_PATH: &'static str = ".state/state.data";

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            databases: vec![],
            state_path: STATE_PATH.to_string(),
            max_iterations: 10,
            max_rows: 500,
        }
    }
}
