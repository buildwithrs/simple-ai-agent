use rustyline::DefaultEditor;
use termimad::MadSkin;

use crate::{
    errors::AgentError,
    llm::{LLMClient, strip_think, to_chat_message},
    tool::ToolRegistry,
};

const CMD_HIS: &'static str = ".history/history.txt";

pub struct PGAgent {
    pub tool_registry: ToolRegistry,
    pub llm_cli: LLMClient,
}

impl PGAgent {
    pub fn new(tool_reg: ToolRegistry, llm_cli: LLMClient) -> Self {
        Self {
            tool_registry: tool_reg,
            llm_cli,
        }
    }

    pub async fn handle_input(&mut self, msg: &str) -> Result<(), AgentError> {
        let msg = &[to_chat_message(msg)];
        let res = &self.llm_cli.chat(msg, &[]).await?;

        let skin = MadSkin::default();
        match &res.content {
            Some(c) => {
                let answer = strip_think(&c);
                skin.write_text(answer)?;
                Ok(())
            }
            None => return Ok(()),
        }
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
