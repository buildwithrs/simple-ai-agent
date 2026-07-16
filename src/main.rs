use rustyline::DefaultEditor;
use simple_ai_agent::llm::LLMClient;

const CMD_HIS: &'static str = ".history/history.txt";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rl = DefaultEditor::new()?;
    rl.load_history(CMD_HIS)?;

    dotenv::dotenv().ok();
    let mut client = LLMClient::from_env()?;

    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                println!("You entered: {}", line);
                match client.send_message(&line).await {
                    Ok(res) => {
                        let first_msg = res.first().unwrap().message.content.clone();
                        println!("{:?}", first_msg.unwrap());
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                    }
                }
                
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
