use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::path::{Path, PathBuf};

pub struct Repl {
    rl: DefaultEditor,
    buf: String,
    history_path: PathBuf,
}

const DEFAULT_HISTORY_PATH: &str = "history.txt";

pub enum Input {
    Code(String),
    Command(Result<Command, String>),
}

pub enum Command {
    Catalog,
}

pub fn input() -> Repl {
    Repl::new()
}

impl Repl {
    pub fn new() -> Self {
        Self::with_history(DEFAULT_HISTORY_PATH)
    }

    pub fn with_history(path: impl AsRef<Path>) -> Self {
        let mut rl = DefaultEditor::new().unwrap();
        _ = rl.load_history(&path);

        Self {
            rl,
            buf: String::new(),
            history_path: path.as_ref().into(),
        }
    }

    pub fn save_history(&mut self) {
        _ = self.rl.save_history(&self.history_path);
    }

    pub fn scan(&mut self) -> Option<Input> {
        loop {
            let prompt = if self.buf.is_empty() { "sql> " } else { "...> " };
            match self.rl.readline(prompt) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    if self.buf.is_empty() && trimmed.starts_with("/") {
                        _ = self.rl.add_history_entry(&line);
                        match line.as_ref() {
                            "/catalog" => return Some(Input::Command(Ok(Command::Catalog))),
                            _ => return Some(Input::Command(Err(line))),
                        }
                    }

                    self.buf.push_str(&line);
                    _ = self.rl.add_history_entry(&line);

                    if trimmed.ends_with(";") {
                        return Some(Input::Code(std::mem::take(&mut self.buf)));
                    }

                    self.buf.push('\n');
                }
                Err(ReadlineError::Interrupted) => self.buf.clear(),
                Err(ReadlineError::Eof) => return None,
                Err(e) => {
                    panic!("{e}");
                }
            }
        }
    }

    pub fn lines(&mut self) -> Lines<'_> {
        Lines { repl: self }
    }
}

impl Drop for Repl {
    fn drop(&mut self) {
        self.save_history();
    }
}

impl Iterator for Repl {
    type Item = Input;

    fn next(&mut self) -> Option<Self::Item> {
        self.scan()
    }
}

pub struct Lines<'a> {
    repl: &'a mut Repl,
}

impl<'a> Iterator for Lines<'a> {
    type Item = Input;

    fn next(&mut self) -> Option<Self::Item> {
        self.repl.scan()
    }
}
