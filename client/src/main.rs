use std::path::PathBuf;

use base64ct::{Base64, Encoding};

use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::{
    channel::Channel, key_registry::KeyRegistry, message_repository::MessageRepository,
    wallet::Wallet,
};

pub mod channel;
pub mod key_registry;
pub mod message_repository;
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

    let wallet = Wallet::new(
        network_rpc_url(env.network.clone()),
        signer.account_id.clone(),
        signer,
    );
    let wallet2 = Wallet::new(
        network_rpc_url(env.network.clone()),
        signer2.account_id.clone(),
        signer2,
    );

    let keyreg = KeyRegistry::new(&wallet, &env.key_registry_account_id);
    let keyreg2 = KeyRegistry::new(&wallet, &env.key_registry_account_id);

    let mut rng = OsRng;

    let secret_key = x25519_dalek::StaticSecret::new(&mut rng);
    let public_key = x25519_dalek::PublicKey::from(&secret_key);

    let public_key_string = Base64::encode_string(public_key.as_bytes());

    println!("Generated my key: {public_key_string}");

    print!("Setting my key in registry...");
    keyreg.set_my_key(&public_key).await?;
    println!("Done.");

    print!("Retrieving my key from registry...");
    let response = keyreg.get_my_key().await?;
    println!("Done.");

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
    println!("Done.");

    let message = b"first to second";
    let message_seq = 0;

    // first key sends message to second key
    {
        let (first_send, _first_recv) = Channel::pair(
            &secret_key.to_bytes().into(),
            &public_key2.to_bytes().into(),
        );
        let msgrep = MessageRepository::new(&wallet, &env.message_repository_account_id);
        let sequence_hash = &*first_send.sequence_hash(message_seq);
        println!(
            "Message sequence hash: {}",
            Base64::encode_string(sequence_hash),
        );
        print!("Publishing message (first -> second) to message repository...");
        msgrep
            .publish_message(sequence_hash, &first_send.encrypt(message_seq, message)?)
            .await?;
        println!("Done.");
    }

    // second key receives first key's message
    {
        let (_second_send, second_recv) = Channel::pair(
            &secret_key2.to_bytes().into(),
            &public_key.to_bytes().into(),
        );
        let msgrep2 = MessageRepository::new(&wallet2, &env.message_repository_account_id);
        let sequence_hash = &*second_recv.sequence_hash(message_seq);
        println!(
            "Message sequence hash: {}",
            Base64::encode_string(sequence_hash),
        );
        print!("Retrieving message from message repository...");
        let ciphertext = msgrep2.get_message(sequence_hash).await?;
        println!("Done.");
        print!("Decrypting...");
        let received_cleartext = second_recv.decrypt(message_seq, &ciphertext)?;
        println!("Done.");

        print!("Testing equality...");
        assert_eq!(received_cleartext, message);
        println!("Done.");
    }

    Ok(())
}
