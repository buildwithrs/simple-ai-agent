#[derive(Debug)]
pub struct AgentConfig {
    pub databases: Vec<String>, // can config multiple databases
    pub state_path: String,     // where to store the agent state
}
