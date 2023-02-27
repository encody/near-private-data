use std::path::PathBuf;

use base64ct::{Base64, Encoding};

use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::{key_registry::KeyRegistry, messenger::Messenger, wallet::Wallet};

pub mod channel;
pub mod key_registry;
pub mod message_repository;
pub mod messenger;
pub mod wallet;

#[derive(Serialize, Deserialize, Debug)]
struct Environment {
    key_file_path: PathBuf,
    key2_file_path: PathBuf,
    network: Option<String>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;

    let env: Environment = envy::from_env()?;

    let signer = near_crypto::InMemorySigner::from_file(&env.key_file_path)?;
    let signer2 = near_crypto::InMemorySigner::from_file(&env.key2_file_path)?;

    let wallet1 = Wallet::new(
        network_rpc_url(env.network.clone()),
        signer.account_id.clone(),
        signer,
    );
    let wallet2 = Wallet::new(
        network_rpc_url(env.network.clone()),
        signer2.account_id.clone(),
        signer2,
    );

    let keyreg1 = KeyRegistry::new(&wallet1, &env.key_registry_account_id);
    let keyreg2 = KeyRegistry::new(&wallet2, &env.key_registry_account_id);

    let mut rng = OsRng;

    let secret_key1 = x25519_dalek::StaticSecret::new(&mut rng);
    let public_key = x25519_dalek::PublicKey::from(&secret_key1);

    let public_key_string = Base64::encode_string(public_key.as_bytes());

    println!("Generated my key: {public_key_string}");

    print!("Setting my key in registry...");
    keyreg1.set_my_key(&public_key).await?;
    println!("done.");

    print!("Retrieving my key from registry...");
    let response = keyreg1.get_my_key().await?;
    println!("done.");

    println!(
        "Response from contract: {}",
        Base64::encode_string(&response),
    );

    assert_eq!(public_key.as_bytes() as &[u8], &response);

    println!("Key is correct.");

    print!("Setting second account key in registry...");
    let secret_key2 = x25519_dalek::StaticSecret::new(&mut rng);
    let public_key2 = x25519_dalek::PublicKey::from(&secret_key2);
    keyreg2.set_my_key(&public_key2).await?;
    println!("done.");

    let message = b"first to second";

    print!("Creating messengers...");
    let mut messenger1 = Messenger::new(
        &wallet1,
        secret_key1,
        &env.key_registry_account_id,
        &env.message_repository_account_id,
    );
    let mut messenger2 = Messenger::new(
        &wallet2,
        secret_key2,
        &env.key_registry_account_id,
        &env.message_repository_account_id,
    );
    println!("done.");

    print!("Registering correspondents...");
    messenger2
        .register_correspondent(&wallet1.account_id)
        .await?;
    messenger1
        .register_correspondent(&wallet2.account_id)
        .await?;
    println!("done.");

    print!(
        "Sending \"{}\" from {} to {}...",
        String::from_utf8_lossy(message),
        &wallet1.account_id,
        &wallet2.account_id
    );
    messenger1.send(&wallet2.account_id, message).await?;
    println!("done.");

    print!("Checking for new messages to {}...", &wallet2.account_id);
    let received_message = loop {
        if let Some(m) = messenger2
            .check_received_one_from(&wallet1.account_id)
            .await?
        {
            break m;
        }
    };
    println!("done.");

    println!("Received: \"{}\"", String::from_utf8_lossy(&received_message));

    Ok(())
}
