use std::{
    io::Write,
    sync::{Arc, Mutex},
    thread,
};

use tokio::sync::mpsc;

use crate::highlight;

pub struct LineEditor {
    pub recv: mpsc::Receiver<String>,
    pub prompt: Arc<Mutex<String>>,
    buffer: Arc<Mutex<String>>,
}

impl LineEditor {
    fn prompt(prompt: &str, buffer: &str) -> String {
        let buffer_line_styled = buffer
            .split_once(' ')
            .map(|(command, tail)| format!("{} {}", highlight::text::command(command), tail))
            .unwrap_or_else(|| highlight::text::command(buffer).to_string());

        format!("{prompt}{buffer_line_styled}")
    }

    pub fn redraw_prompt(&self) {
        let stdout = console::Term::stdout();
        stdout.clear_line().unwrap();
        write!(
            &stdout,
            "\r{}",
            LineEditor::prompt(&self.prompt.lock().unwrap(), &self.buffer.lock().unwrap()),
        )
        .unwrap();
    }

    pub fn set_prompt(&mut self, prompt: impl ToString) {
        *self.prompt.lock().unwrap() = prompt.to_string();
    }

    pub fn new(prompt: &str) -> Self {
        let (send, recv) = mpsc::channel(2);
        let buffer = Arc::new(Mutex::new(String::new()));
        let prompt = Arc::new(Mutex::new(prompt.to_string()));

        thread::spawn({
            let buffer = Arc::clone(&buffer);
            let prompt = Arc::clone(&prompt);
            move || {
                let stdout = console::Term::stdout();
                loop {
                    let k = stdout.read_key().unwrap();
                    match k {
                        console::Key::Enter => {
                            let mut b = buffer.lock().unwrap();
                            let s = b.to_string();
                            b.clear();
                            drop(b);
                            writeln!(&stdout).unwrap();
                            send.blocking_send(s).unwrap();
                        }
                        console::Key::Backspace => {
                            let mut buffer = buffer.lock().unwrap();
                            buffer.pop();
                            stdout.clear_line().unwrap();
                            write!(
                                &stdout,
                                "\r{}",
                                LineEditor::prompt(&prompt.lock().unwrap(), &buffer)
                            )
                            .unwrap();
                        }
                        console::Key::Char(c) => {
                            let mut buffer = buffer.lock().unwrap();
                            buffer.push(c);
                            write!(
                                &stdout,
                                "\r{}",
                                LineEditor::prompt(&prompt.lock().unwrap(), &buffer)
                            )
                            .unwrap();
                        }
                        _ => {}
                    }
                }
            }
        });

        let le = Self {
            recv,
            prompt,
            buffer,
        };

        le.redraw_prompt();

        le
    }
}
