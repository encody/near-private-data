use std::path::PathBuf;

use base64ct::{Base64, Encoding};
use ed25519_dalek::Keypair;

use near_jsonrpc_client::{NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_primitives::types::AccountId;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::{key_registry::KeyRegistry, wallet::Wallet};

pub mod channel;
pub mod key_registry;
pub mod wallet;

#[derive(Serialize, Deserialize, Debug)]
struct Environment {
    key_file_path: PathBuf,
    network: Option<String>,
    key_registry_account_id: AccountId,
    // message_repository_account_id: AccountId,
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

fn public_key_to_string(public_key: &ed25519_dalek::PublicKey) -> String {
    Base64::encode_string(public_key.as_bytes())
}

// fn public_key_from_string(s: &str) -> Option<ed25519_dalek::PublicKey> {
//     Base64::decode_vec(s)
//         .ok()
//         .and_then(|bytes| ed25519_dalek::PublicKey::from_bytes(&bytes).ok())
// }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;

    let env: Environment = envy::from_env()?;

    let signer = near_crypto::InMemorySigner::from_file(&env.key_file_path)?;

    let wallet = Wallet::new(
        network_rpc_url(env.network.clone()),
        signer.account_id.clone(),
        signer,
    );

    let mut rng = OsRng;

    let message_keypair: Keypair = Keypair::generate(&mut rng);

    let keyreg = KeyRegistry::new(&wallet, env.key_registry_account_id);

    let public_key_string = public_key_to_string(&message_keypair.public);

    println!("Generated key: {public_key_string}");

    keyreg.set_my_key(&message_keypair.public).await?;

    let response = keyreg.get_my_key().await?;

    println!(
        "Response from contract: {}",
        Base64::encode_string(&response),
    );

    assert_eq!(message_keypair.public.as_bytes() as &[u8], &response);

    Ok(())
}

#[cfg(test)]
mod tests {
    use base64ct::{Base64, Encoding};
    use ed25519_dalek::Keypair;
    use rand::rngs::OsRng;

    #[test]
    fn test() {
        let mut csprng = OsRng {};
        let keypair: Keypair = Keypair::generate(&mut csprng);

        println!(
            "ed25519:{}",
            Base64::encode_string(keypair.public.as_bytes())
        );
    }
}
