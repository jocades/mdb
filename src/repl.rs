use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::path::{Path, PathBuf};

pub struct Repl {
    rl: DefaultEditor,
    buf: String,
    history_path: PathBuf,
}

const DEFAULT_HISTORY_PATH: &str = "history.txt";

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

    pub fn scan(&mut self) -> Option<String> {
        loop {
            let prompt = if self.buf.is_empty() { "sql> " } else { "...> " };
            match self.rl.readline(prompt) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    self.buf.push_str(&line);
                    if trimmed.ends_with(";") {
                        _ = self.rl.add_history_entry(&line);
                        return Some(std::mem::take(&mut self.buf));
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
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.scan()
    }
}

pub struct Lines<'a> {
    repl: &'a mut Repl,
}

impl<'a> Iterator for Lines<'a> {
    type Item = String;

    fn next(&mut self) -> Option<Self::Item> {
        self.repl.scan()
    }
}
