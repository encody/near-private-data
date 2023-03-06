use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use base64ct::{Base64, Encoding};

use chrono::{Local, NaiveDateTime, TimeZone};
use console::style;
use messenger::DecryptedMessage;
use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
use tokio::{select, sync::mpsc};
use x25519_dalek::StaticSecret;

use crate::{messenger::Messenger, wallet::Wallet};

pub mod channel;
pub mod key_registry;
pub mod message_repository;
pub mod messenger;
pub mod wallet;

#[derive(Serialize, Deserialize, Debug)]
struct Environment {
    key_file_path: PathBuf,
    network: Option<String>,
    messenger_secret_key: String,
    key_registry_account_id: AccountId,
    message_repository_account_id: AccountId,
}

fn network_rpc_url(network: Option<String>) -> String {
    network
        .map(|network| match &network.to_lowercase()[..] {
            "mainnet" => NEAR_MAINNET_RPC_URL.to_string(),
            "testnet" => NEAR_TESTNET_RPC_URL.to_string(),
            _ => network, // assume it's a URL
        })
        .unwrap_or_else(|| NEAR_TESTNET_RPC_URL.to_string())
}

fn monitor_conversation(
    messenger: Arc<Messenger>,
    sender_id: AccountId,
) -> (
    impl Fn(),
    tokio::sync::mpsc::Receiver<(AccountId, DecryptedMessage)>,
) {
    let alive = Arc::new(AtomicBool::new(true));
    let (send, recv) = tokio::sync::mpsc::channel(1);

    let kill = {
        let alive = Arc::clone(&alive);
        move || {
            alive.store(false, Ordering::SeqCst);
        }
    };

    tokio::spawn({
        async move {
            while alive.load(Ordering::SeqCst) {
                if let Some(received_message) =
                    messenger.ordered_conversation(&sender_id).await.unwrap()
                {
                    send.send(received_message).await.unwrap();
                }
            }
        }
    });

    (kill, recv)
}

struct LineEditor {
    pub recv: mpsc::Receiver<String>,
    pub prompt: Arc<Mutex<String>>,
    buffer: Arc<Mutex<String>>,
}

impl LineEditor {
    fn prompt(prompt: &str, buffer: &str) -> String {
        let buffer_line_styled = buffer
            .split_once(' ')
            .map(|(command, tail)| format!("{} {}", highlight::text::command(command), tail))
            .unwrap_or_else(|| highlight::text::command(buffer));

        format!("{prompt}{buffer_line_styled}")
    }

    fn redraw_prompt(&self) {
        eprint!(
            "\r{}",
            LineEditor::prompt(&self.prompt.lock().unwrap(), &self.buffer.lock().unwrap()),
        );
    }

    pub fn draw_prompt(&self) {
        eprint!(
            "{}",
            LineEditor::prompt(&self.prompt.lock().unwrap(), &self.buffer.lock().unwrap()),
        );
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
                            eprintln!();
                            send.blocking_send(s).unwrap();
                        }
                        console::Key::Backspace => {
                            let mut buffer = buffer.lock().unwrap();
                            buffer.pop();
                            stdout.clear_line().unwrap();
                            eprint!("\r{}", LineEditor::prompt(&prompt.lock().unwrap(), &buffer));
                        }
                        console::Key::Char(c) => {
                            let mut buffer = buffer.lock().unwrap();
                            buffer.push(c);
                            eprint!("\r{}", LineEditor::prompt(&prompt.lock().unwrap(), &buffer));
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

mod highlight {
    pub mod account {
        use console::style;

        pub fn me(s: impl AsRef<str>) -> String {
            style(s.as_ref()).cyan().bold().bright().to_string()
        }

        pub fn other(s: impl AsRef<str>) -> String {
            style(s.as_ref()).magenta().bold().bright().to_string()
        }
    }

    pub mod text {
        use console::style;

        pub fn dim(s: impl AsRef<str>) -> String {
            style(s.as_ref()).black().bright().to_string()
        }

        pub fn error(s: impl AsRef<str>) -> String {
            style(s.as_ref()).red().to_string()
        }

        pub fn control(s: impl AsRef<str>) -> String {
            style(s.as_ref()).green().to_string()
        }

        pub fn command(s: impl AsRef<str>) -> String {
            style(s.as_ref()).green().bold().to_string()
        }
    }
}

fn format_time(epoch_ms: i64) -> String {
    Local
        .from_utc_datetime(&NaiveDateTime::from_timestamp_millis(epoch_ms).unwrap())
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;

    let env: Environment = envy::from_env()?;

    let signer = near_crypto::InMemorySigner::from_file(&env.key_file_path)?;

    let wallet = Arc::new(Wallet::new(
        network_rpc_url(env.network.clone()),
        signer.account_id.clone(),
        signer,
    ));

    let messenger_secret_key: [u8; 32] = Base64::decode_vec(&env.messenger_secret_key)
        .unwrap()
        .try_into()
        .unwrap();

    let messenger = Arc::new(Messenger::new(
        Arc::clone(&wallet),
        StaticSecret::from(messenger_secret_key),
        &env.key_registry_account_id,
        &env.message_repository_account_id,
    ));

    eprintln!(
        "Welcome to the {} (test version)",
        style("NEAR Private Data Messenger").magenta(),
    );

    eprint!("Syncing public key with key repository...");
    messenger.sync_key().await?;
    eprintln!("done.");

    let mut line_editor = LineEditor::new("");

    loop {
        eprintln!(
            "You are logged in as {}.",
            highlight::account::me(&wallet.account_id),
        );
        eprintln!("{} to exit.", highlight::text::command("/quit"));

        line_editor.set_prompt("Chat with: ");
        line_editor.redraw_prompt();
        let correspondent: AccountId = loop {
            let input = line_editor.recv.recv().await.unwrap();
            if input == "/quit" {
                return Ok(());
            }
            if let Ok(account_id) = input.parse() {
                break account_id;
            }
        };

        eprintln!(
            "{} to say, {} to leave.",
            highlight::text::command("/say"),
            highlight::text::command("/leave"),
        );

        messenger
            .register_correspondent(&correspondent)
            .await
            .unwrap();

        let (kill, mut recv) = monitor_conversation(Arc::clone(&messenger), correspondent.clone());

        line_editor.set_prompt(format!("{}> ", highlight::account::me(&wallet.account_id)));

        loop {
            line_editor.draw_prompt();

            select! {
                input_string = line_editor.recv.recv() => {
                    let send_message = input_string.unwrap();
                    let send_message = send_message
                        .strip_suffix("\r\n")
                        .or(send_message.strip_suffix('\n'))
                        .unwrap_or(&send_message);
                    let (command, tail) = send_message
                        .split_once(' ')
                        .unwrap_or((send_message, ""));

                    match command {
                        "/say" => {
                            messenger.send(&correspondent, tail).await.unwrap();
                        }
                        "/leave" => {
                            eprintln!("{}", highlight::text::control("Exiting chat."));
                            kill();
                            break;
                        }
                        _ => {
                            eprintln!("{}", highlight::text::error(format!("Unknown command: {}", command)));
                        }
                    }
                },
                recv_message = recv.recv() => {
                    if let Some((sender_id, recv_message)) = recv_message {
                        let sender_styled = if sender_id == wallet.account_id {
                            highlight::account::me(&sender_id)
                        } else {
                            highlight::account::other(&sender_id)
                        };
                        let time_styled = highlight::text::dim(format_time(recv_message.block_timestamp_ms as i64));
                        let message_string = String::from_utf8_lossy(&recv_message.message);
                        eprintln!("\r[{time_styled}] {sender_styled}: {message_string}");
                    } else {
                        eprintln!("\r{}", highlight::text::error("Error connecting to message repository."));
                        kill();
                        break;
                    }
                },
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use base64ct::{Base64, Encoding};
    use rand::rngs::OsRng;

    #[ignore = "Use to generate test keys"]
    #[test]
    fn generate_messenger_secret_key() {
        let messenger_secret_key = x25519_dalek::StaticSecret::new(OsRng);
        let secret_key_b64 = Base64::encode_string(&messenger_secret_key.to_bytes());
        println!("\"{secret_key_b64}\"");
    }
}
