use rustyline::DefaultEditor;

const CMD_HIS: &'static str = ".history/history.txt";

fn main() -> anyhow::Result<()> {
    let mut rl = DefaultEditor::new()?;
    rl.load_history(CMD_HIS).ok();

    loop {
        let readline = rl.readline(">> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str())?;
                println!("You entered: {}", line);
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
