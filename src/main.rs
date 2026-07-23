use simple_pg_agent::{
    agent::PGAgent,
    db::{establish_connection, register_db_tools},
    llm::LLMClient,
    tool::ToolRegistry,
};

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    dotenv::dotenv().ok();

    let llm_cli = LLMClient::from_env()?;
    let mut tool_registry = ToolRegistry::new();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = establish_connection(&database_url).await?;

    register_db_tools(&mut tool_registry, pool);

    let mut agent = PGAgent::new(tool_registry, llm_cli);
    agent.run().await?;

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,simple_pg_agent::agent=debug"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
