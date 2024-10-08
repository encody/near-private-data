use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use data_encoding::BASE64;
use fc_client::{messenger::Messenger, wallet::Wallet};
use near_workspaces::{network::Sandbox, Account, AccountId, Contract, Worker};
use rand::rngs::OsRng;
use serde_json::json;
use tokio::sync::{OnceCell, RwLock};

enum ContractWasm {
    MessageRepository,
    KeyRegistry,
}

impl ContractWasm {
    async fn load(&self) -> &'static [u8] {
        static MESSAGE_REPOSITORY_WASM: OnceCell<&'static [u8]> = OnceCell::const_new();
        static KEY_REGISTRY_WASM: OnceCell<&'static [u8]> = OnceCell::const_new();

        let (cell, path) = match self {
            ContractWasm::MessageRepository => (
                &MESSAGE_REPOSITORY_WASM,
                "../../contract/message-repository/",
            ),
            ContractWasm::KeyRegistry => (&KEY_REGISTRY_WASM, "../../contract/key-registry/"),
        };

        cell.get_or_init(|| async {
            Box::new(near_workspaces::compile_project(path).await.unwrap()).leak()
                as &'static [u8]
        })
        .await
    }
}

async fn prefixed_account(worker: &Worker<Sandbox>, prefix: &str) -> Account {
    assert!(
        prefix.len() <= AccountId::MAX_LEN,
        "prefix longer than max account ID length",
    );

    let (old_id, sk) = worker.dev_generate().await;
    let id: AccountId = format!("{prefix}{}", &old_id.as_str()[prefix.len()..])
        .parse()
        .unwrap();

    worker.create_tla(id, sk).await.unwrap().result
}

async fn deploy_with_prefix_and_init(
    worker: &Worker<Sandbox>,
    prefix: &str,
    wasm: &[u8],
) -> Contract {
    let contract = prefixed_account(worker, prefix)
        .await
        .deploy(wasm)
        .await
        .unwrap()
        .result;

    contract
        .call("new")
        .args_json(json!({}))
        .transact()
        .await
        .unwrap()
        .unwrap();

    contract
}

#[tokio::test]
async fn happy_path() {
    let (worker, message_repository_wasm, key_registry_wasm) = tokio::join!(
        async { near_workspaces::sandbox().await.unwrap() },
        async { ContractWasm::MessageRepository.load().await },
        async { ContractWasm::KeyRegistry.load().await },
    );

    let (message_repository_contract, key_registry_contract, alice) = tokio::join!(
        deploy_with_prefix_and_init(&worker, "msgrepo", message_repository_wasm),
        deploy_with_prefix_and_init(&worker, "keyreg", key_registry_wasm),
        prefixed_account(&worker, "alice"),
    );

    let signer = near_crypto::InMemorySigner::from_secret_key(
        alice.id().clone(),
        alice.secret_key().to_string().parse().unwrap(),
    );

    let wallet = Arc::new(Wallet::new(
        worker.rpc_addr(),
        signer.account_id.clone(),
        signer.into(),
    ));

    let alice_messenger_key = x25519_dalek::StaticSecret::random_from_rng(OsRng);

    let messenger = Arc::new(Messenger::new(
        Arc::clone(&wallet),
        alice_messenger_key.clone(),
        key_registry_contract.id(),
        message_repository_contract.id(),
    ));

    println!("Messenger created!");
    println!("Syncing key...");

    messenger.sync_key().await.unwrap();

    let key_registry = KeyRegistry::new(&key_registry_contract);
    assert_eq!(
        key_registry.get_public_key(alice.id()).await,
        x25519_dalek::PublicKey::from(&alice_messenger_key).as_bytes(),
        "Could not retrieve key from key registry",
    );
    println!("Key synced!");
}

struct KeyRegistry<'a> {
    contract: &'a Contract,
}

impl<'a> KeyRegistry<'a> {
    pub fn new(contract: &'a Contract) -> Self {
        Self { contract }
    }

    pub async fn get_public_key(&self, account_id: &AccountId) -> Vec<u8> {
        let encoded = self
            .contract
            .view("get_public_key")
            .args_json(json!({
                "account_id": account_id,
            }))
            .await
            .unwrap()
            .json::<String>()
            .unwrap();

        BASE64.decode(encoded.as_bytes()).unwrap()
    }

    pub async fn set_public_key(&self, account: &Account, public_key: Option<&[u8]>) {
        account
            .call(self.contract.id(), "set_public_key")
            .args_json(json!({
                "public_key": public_key.map(|k| {
                    data_encoding::BASE64.encode(k)
                }),
            }))
            .transact()
            .await
            .unwrap()
            .unwrap();
    }
}
