use std::path::PathBuf;

use near_jsonrpc_client::{methods, JsonRpcClient, NEAR_MAINNET_RPC_URL, NEAR_TESTNET_RPC_URL};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::{
    types::{BlockReference, Finality},
    views::QueryRequest,
};
use serde::{Deserialize, Serialize};

pub mod channel;

#[derive(Serialize, Deserialize, Debug)]
struct Environment {
    key_file_path: PathBuf,
    network: Option<String>,
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
async fn main() {
    dotenvy::dotenv().ok();

    let env: Environment = envy::from_env().expect("Failed to read environment variables");

    let signer = near_crypto::InMemorySigner::from_file(&env.key_file_path)
        .expect("Failed to load key file");

    let client = JsonRpcClient::connect(network_rpc_url(env.network));

    let response = client
        .call(methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::Final),
            request: QueryRequest::CallFunction {
                account_id: "wrap.testnet".parse().unwrap(),
                method_name: "ft_metadata".to_string(),
                args: vec![].into(),
            },
        })
        .await
        .unwrap();

    match response.kind {
        QueryResponseKind::CallResult(r) => {
            let v: serde_json::Value = serde_json::from_slice(&r.result).unwrap();
            println!("Response: {:?}", v);
        }
        _ => {
            panic!("Wrong response: {response:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test() {
        use ed25519_dalek::Keypair;
        use rand::rngs::OsRng;

        let mut csprng = OsRng {};
        let keypair: Keypair = Keypair::generate(&mut csprng);

        println!("{:?}", keypair);
    }
}
