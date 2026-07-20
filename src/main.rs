use rustyline::DefaultEditor;
use simple_pg_agent::llm::{LLMClient, to_chat_message};
use termimad::MadSkin;

const CMD_HIS: &'static str = ".history/history.txt";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut rl = DefaultEditor::new()?;
    rl.load_history(CMD_HIS)?;

    dotenv::dotenv().ok();
    let mut client = LLMClient::from_env()?;

    let skin = MadSkin::default();

    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                
                let msg = &[to_chat_message(&line)];
                match client.chat(msg, &[]).await {
                    Ok(res) => {
                        let content = res.content.unwrap_or_default();
                        let answer = strip_think(&content);
                        // let fmt_ans = FmtText::from(&skin, answer, None);

                        skin.write_text(answer)?;
                        // println!("{}", fmt_ans);
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

fn strip_think(s: &str) -> &str {
    // everything after the first  tag, trimmed
    match s.split_once("</think>") {
        Some((_, rest)) => rest.trim_start(),
        None => s.trim_start(),
    }
}
