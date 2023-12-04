use crate::{
    config::Environment, draw::Draw, kill::Kill, messenger::Messenger, proxy::Proxy, traits::Actor,
};
use chrono::{Local, NaiveDateTime, TimeZone};
use messenger::{DecryptedMessage, SequencedHashMessage};
use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::{select, sync::mpsc};

pub mod channel;
pub mod combined;
pub mod config;
pub mod draw;
pub mod group;
pub mod highlight;
pub mod key_registry;
pub mod kill;
pub mod message_repository;
pub mod messenger;
pub mod proxy;
pub mod traits;
pub mod wallet;

pub type Base64String = String;

fn network_rpc_url(network: Option<&String>) -> String {
    network
        .map(|network| match &network.to_lowercase()[..] {
            "mainnet" => NEAR_MAINNET_RPC_URL.to_string(),
            "testnet" => NEAR_TESTNET_RPC_URL.to_string(),
            _ => network.clone(), // assume it's a URL
        })
        .unwrap_or_else(|| NEAR_TESTNET_RPC_URL.to_string())
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

fn format_time(epoch_ms: i64) -> String {
    Local
        .from_utc_datetime(&NaiveDateTime::from_timestamp_millis(epoch_ms).unwrap())
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    match std::env::var("ENV") {
        Ok(path) => dotenvy::from_path(path).unwrap(),
        _ => {
            dotenvy::dotenv().unwrap();
        }
    }
    let env: Environment = envy::from_env().unwrap();
    let env = Arc::new(env);
    log::debug!("Environment: {:#?}", env);

    // Setup the channels
    // TODO: use ctrlc
    let kill_tx = Kill.start(()).unwrap();
    let kill = || {
        let kill_tx = kill_tx.clone();
        tokio::spawn(async move {
            kill_tx.send(()).await.unwrap();
        });
    };

    let (messages_tx, mut messages_rx) = mpsc::channel::<(AccountId, DecryptedMessage)>(24);
    let messages_tx = Arc::new(messages_tx);

    // Setup drawing components
    let draw_tx = Draw.start(()).unwrap();
    let mut line_editor = LineEditor::new("");
    let draw = |str| async {
        draw_tx.send(str).await.unwrap();
    };

    let proxy_tx = Proxy::new(
        &proxy::Config::new(
            &env.proxy_key_file_path,
            env.proxy_verifying_key_path.as_ref(),
            &env.proxy_messenger_secret_key,
        ),
        &env.key_registry_account_id,
        &env.message_repository_account_id,
        env.network.as_ref(),
    )
    .unwrap()
    .start(())
    .unwrap();

    let messenger = Messenger::init(&env).unwrap();
    let me = messenger.wallet.account_id.clone();
    let messenger_tx = messenger
        .start((
            draw_tx.clone(),
            messages_tx.clone(),
            proxy_tx.clone(),
            kill_tx.clone(),
        ))
        .unwrap();

    thread::sleep(Duration::from_secs(2));

    draw(format!(
        "{} to say, {} to start a conversation, {} to leave.",
        highlight::text::command("/say {correspondent}"),
        highlight::text::command("/invite {correspondent}"),
        highlight::text::command("/leave"),
    ))
    .await;

    line_editor.set_prompt(format!("{}> ", highlight::account::me(&me)));

    loop {
        line_editor.redraw_prompt();

        if Kill::should_die() {
            break;
        }

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
                        let (correspondent, tail) = tail.split_once(' ').unwrap_or(("", tail));
                        let correspondent: AccountId = correspondent.parse().unwrap();
                        messenger_tx.send(messenger::Message::Clear(correspondent, tail.to_string())).await.unwrap();
                    }
                    "/proxied" => {
                        let (correspondent, tail) = tail.split_once(' ').unwrap_or(("", tail));
                        let correspondent: AccountId = correspondent.parse().unwrap();
                        messenger_tx.send(messenger::Message::Hidden(correspondent, tail.to_string())).await.unwrap();
                    }
                    "/leave" => {
                        draw(format!("\r{}.", highlight::text::control("Exiting chat"))).await;
                        kill();
                        break;
                    }
                    "/quit" | "/exit" => {
                        kill();
                        return;
                    }
                    "/invite" => {
                        // Parse the account id from the user
                        if let Ok(account_id) = tail.parse() {
                                messenger_tx
                                    .send(messenger::Message::RegisterCorrespondent(account_id))
                                    .await
                                    .unwrap();
                        }
                    }
                    _ => {
                        draw(format!("\r{}", highlight::text::error(format!("Unknown command: {}", command)))).await;
                    }
                }
            },
            recv_message = messages_rx.recv() => {
                if let Some((sender_id, recv_message)) = recv_message {
                    let sender_styled = if sender_id == me.clone() {
                        highlight::account::me(&sender_id)
                    } else {
                        highlight::account::other(&sender_id)
                    };
                    let time_styled = highlight::text::dim(format_time(recv_message.block_timestamp_ms as i64));
                    let message_string = String::from_utf8_lossy(&recv_message.message);
                    draw(format!("\r[{time_styled}] {sender_styled}: {message_string}")).await;
                } else {
                    draw(format!("{}", highlight::text::error("Error connecting to message repository."))).await;
                    kill();
                    break;
                }
            }
        };
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
