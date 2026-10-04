#[macro_use]
mod macros;
mod bytes;
mod page;
mod pager;
mod sql;

fn run(source: &str) {
    match sql::parser::parse(&source) {
        Ok(stmts) => println!("{stmts:?}"),
        Err(e) => eprintln!("error: {e:?}"),
    }
}

// fn repl() {
//     use std::io::{self, BufRead};
//     let stdin = io::stdin().lock();
//     for line in stdin.lines() {
//         run(&line.unwrap());
//     }
// }

fn main() {
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;

    const HIST_FILE: &str = "history.txt";

    let mut rl = DefaultEditor::new().unwrap();
    _ = rl.load_history(HIST_FILE);

    let mut buf = String::new();

    loop {
        let prompt = if buf.is_empty() { "sql> " } else { "...> " };

        match rl.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                buf.push_str(&line);

                if trimmed.ends_with(";") {
                    _ = rl.add_history_entry(&line);
                    run(&buf);
                    buf.clear();
                } else {
                    buf.push('\n');
                }
            }
            Err(ReadlineError::Interrupted) => buf.clear(),
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        }
    }

    _ = rl.save_history(HIST_FILE);
}
