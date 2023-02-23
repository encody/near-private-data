use std::path::PathBuf;

use anyhow::{anyhow, bail};
use base64ct::{Base64, Encoding};
use ed25519_dalek::Keypair;
use near_crypto::Signer;
use near_jsonrpc_client::{methods, JsonRpcClient, NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    hash::CryptoHash,
    transaction::FunctionCallAction,
    types::{AccountId, BlockReference, Finality},
    views::{AccessKeyView, QueryRequest},
};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub mod channel;

const ONE_TERAGAS: u64 = 10u64.pow(12);
const ONE_NEAR: u128 = 10u128.pow(24);

#[derive(Serialize, Deserialize, Debug)]
struct Environment {
    key_file_path: PathBuf,
    network: Option<String>,
    key_registry_account_id: AccountId,
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

async fn get_current_nonce(
    client: &near_jsonrpc_client::JsonRpcClient,
    account_id: AccountId,
    public_key: near_crypto::PublicKey,
) -> anyhow::Result<(u64, CryptoHash)> {
    let response = client
        .call(methods::query::RpcQueryRequest {
            block_reference: BlockReference::latest(),
            request: QueryRequest::ViewAccessKey {
                account_id,
                public_key,
            },
        })
        .await?;

    match response.kind {
        QueryResponseKind::AccessKey(AccessKeyView { nonce, .. }) => {
            Ok((nonce, response.block_hash))
        }
        _ => Err(anyhow!("Invalid response from RPC")),
    }
}

fn public_key_to_string(public_key: &ed25519_dalek::PublicKey) -> String {
    Base64::encode_string(public_key.as_bytes())
}

fn public_key_from_string(s: &str) -> Option<ed25519_dalek::PublicKey> {
    Base64::decode_vec(s)
        .ok()
        .and_then(|bytes| ed25519_dalek::PublicKey::from_bytes(&bytes).ok())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;

    let env: Environment = envy::from_env()?;

    let signer = near_crypto::InMemorySigner::from_file(&env.key_file_path)?;

    let client = JsonRpcClient::connect(network_rpc_url(env.network));

    let (current_nonce, block_hash) =
        get_current_nonce(&client, signer.account_id.clone(), signer.public_key()).await?;

    let mut rng = OsRng;

    let message_keypair: Keypair = Keypair::generate(&mut rng);

    let public_key_string = public_key_to_string(&message_keypair.public);

    println!("Generated key: {public_key_string}");

    let transaction = near_primitives::transaction::Transaction {
        nonce: current_nonce + 1,
        block_hash,
        public_key: signer.public_key(),
        signer_id: signer.account_id.clone(),
        receiver_id: env.key_registry_account_id.clone(),
        actions: vec![near_primitives::transaction::Action::FunctionCall(
            FunctionCallAction {
                method_name: "set_public_key".to_string(),
                args: json!({
                    "public_key": public_key_string,
                })
                .to_string()
                .into_bytes(),
                gas: 3 * ONE_TERAGAS,
                deposit: ONE_NEAR >> 1,
            },
        )],
    };

    let signed_transaction = transaction.sign(&signer);

    client
        .call(methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest { signed_transaction })
        .await?;

    let response = client
        .call(methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::CallFunction {
                account_id: env.key_registry_account_id.clone(),
                method_name: "get_public_key".to_string(),
                args: json!({"account_id": signer.account_id.clone()})
                    .to_string()
                    .into_bytes()
                    .into(),
            },
        })
        .await
        .unwrap();

    let response = match response.kind {
        QueryResponseKind::CallResult(r) => serde_json::from_slice::<String>(&r.result)?,
        _ => bail!("Wrong response: {response:?}"),
    };

    println!("Response from contract: {response}");

    let response = Base64::decode_vec(&response).unwrap();

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
