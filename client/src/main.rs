use std::{
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use base64ct::{Base64, Encoding};

use console::style;
use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
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

// message receiver thread
fn receiver(messenger: Arc<Messenger>, sender_id: AccountId) -> impl Fn() {
    let alive = Arc::new(AtomicBool::new(true));

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
                    messenger.receive_one_from(&sender_id).await.unwrap()
                {
                    eprintln!(
                        "{}: {}",
                        style(&sender_id).red().bright(),
                        String::from_utf8_lossy(&received_message)
                    );
                }
            }
        }
    });

    kill
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

    eprintln!("Welcome to the {} (test version)", style("NEAR Private Data Messenger").magenta());

    eprint!("Syncing public key with key repository...");
    messenger.sync_key().await?;
    eprintln!("done.");

    loop {
        println!(
            "You are logged in as {}.",
            style(&wallet.account_id).cyan().bright(),
        );
        println!("Enter {} to exit.", style("/quit").bright().bold());

        let correspondent: String = dialoguer::Input::new()
            .with_prompt("Chat with")
            .validate_with(|s: &String| {
                if s == "/quit" || AccountId::from_str(s).is_ok() {
                    Ok(())
                } else {
                    Err("Invalid")
                }
            })
            .interact_text()
            .unwrap();

        if correspondent == "/quit" {
            return Ok(());
        }

        let correspondent = AccountId::from_str(&correspondent).unwrap();

        eprintln!("Enter {} to leave chat.", style("/leave").bright().bold());

        messenger
            .register_correspondent(&correspondent)
            .await
            .unwrap();

        let kill = receiver(Arc::clone(&messenger), correspondent.clone());

        loop {
            let message: String = dialoguer::Input::new()
                .with_prompt("say")
                .with_post_completion_text("you")
                .interact_text()
                .unwrap();

            if message == "/leave" {
                kill();
                break;
            }

            messenger.send(&correspondent, message).await.unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use base64ct::{Base64, Encoding};
    use rand::rngs::OsRng;

    #[test]
    fn generate_messenger_secret_key() {
        let messenger_secret_key = x25519_dalek::StaticSecret::new(OsRng);
        let secret_key_b64 = Base64::encode_string(&messenger_secret_key.to_bytes());
        println!("\"{secret_key_b64}\"");
    }
}
