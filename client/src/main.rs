use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use base64ct::{Base64, Encoding};
use chrono::{Local, NaiveDateTime, TimeZone};
use console::style;
use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
use tokio::{select, time::sleep};
use x25519_dalek::StaticSecret;

use crate::{
    combined::CombinedMessageStream,
    line_editor::LineEditor,
    messenger::{DecryptedMessage, Messenger},
    wallet::Wallet,
};

pub mod channel;
pub mod combined;
pub mod group;
pub mod highlight;
pub mod key_registry;
pub mod line_editor;
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
            let conversation = messenger.conversation(&sender_id).await.unwrap();
            let mut messages = CombinedMessageStream::new(
                &messenger.message_repository,
                [&conversation.send, &conversation.recv],
            );

            while alive.load(Ordering::SeqCst) {
                if let Some((sender, message)) = messages.next().await.unwrap() {
                    send.send((sender.clone(), message)).await.unwrap();
                } else {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });

    (kill, recv)
}

fn format_time(epoch_ms: i64) -> String {
    Local
        .from_utc_datetime(&NaiveDateTime::from_timestamp_millis(epoch_ms).unwrap())
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match std::env::var("ENV") {
        Ok(path) => dotenvy::from_path(path)?,
        _ => {
            dotenvy::dotenv()?;
        }
    }

    let env: Environment = envy::from_env()?;

    let signer = near_crypto::InMemorySigner::from_file(&env.key_file_path)?;

    let wallet = Arc::new(Wallet::new(
        network_rpc_url(env.network.clone()),
        signer.account_id.clone(),
        signer.into(),
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

    let stdout = console::Term::stdout();

    writeln!(
        &stdout,
        "Welcome to the {} (test version)",
        style("NEAR Private Data Messenger").magenta(),
    )
    .unwrap();

    write!(&stdout, "Syncing public key with key repository...").unwrap();
    messenger.sync_key().await?;
    writeln!(&stdout, "done.").unwrap();

    let mut line_editor = LineEditor::new("");

    loop {
        writeln!(
            &stdout,
            "\rYou are logged in as {}.",
            highlight::account::me(&wallet.account_id),
        )
        .unwrap();
        writeln!(&stdout, "\r{} to exit.", highlight::text::command("/quit")).unwrap();

        line_editor.set_prompt("Chat with: ");
        line_editor.redraw_prompt();
        let correspondent: AccountId = loop {
            let input = line_editor.recv.recv().await.unwrap();
            if input == "/quit" || input == "/exit" {
                return Ok(());
            }
            if let Ok(account_id) = input.parse() {
                break account_id;
            }
        };

        writeln!(
            &stdout,
            "{} to say, {} to leave.",
            highlight::text::command("/say"),
            highlight::text::command("/leave"),
        )
        .unwrap();

        messenger
            .register_correspondent(&correspondent)
            .await
            .unwrap();

        let (kill, mut recv) = monitor_conversation(Arc::clone(&messenger), correspondent.clone());

        line_editor.set_prompt(format!("{}> ", highlight::account::me(&wallet.account_id)));

        loop {
            line_editor.redraw_prompt();

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
                            writeln!(&stdout, "\r{}.", highlight::text::control("Exiting chat")).unwrap();
                            kill();
                            break;
                        }
                        _ => {
                            writeln!(&stdout, "\r{}", highlight::text::error(format!("Unknown command: {}", command))).unwrap();
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
                        writeln!(&stdout, "\r[{time_styled}] {sender_styled}: {message_string}").unwrap();
                    } else {
                        writeln!(&stdout, "{}", highlight::text::error("Error connecting to message repository.")).unwrap();
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

    #[test]
    #[ignore = "Use to generate test keys"]
    fn generate_messenger_secret_key() {
        let messenger_secret_key = x25519_dalek::StaticSecret::new(OsRng);
        let secret_key_b64 = Base64::encode_string(&messenger_secret_key.to_bytes());
        println!("\"{secret_key_b64}\"");
    }
}
