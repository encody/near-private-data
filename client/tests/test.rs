use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use near_workspaces::{network::Sandbox, Account, AccountId, Worker};
use serde_json::json;
use tokio::sync::{OnceCell, RwLock};

static WASM_CACHE: LazyLock<RwLock<HashMap<&str, Arc<OnceCell<&'static [u8]>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

async fn load_wasm(path: &'static str) -> &'static [u8] {
    let read_lock = WASM_CACHE.read().await;
    let cell_option = read_lock.get(path).cloned();
    drop(read_lock);

    let cell = if let Some(cell) = cell_option {
        cell
    } else {
        let cell = Arc::new(OnceCell::new());
        let mut write_lock = WASM_CACHE.write().await;
        write_lock.insert(path, Arc::clone(&cell));
        drop(write_lock);
        cell
    };

    *cell
        .get_or_init(|| async {
            Box::new(near_workspaces::compile_project(path).await.unwrap()).leak()
                as &'static [u8]
        })
        .await
}

async fn prefixed_account(worker: &Worker<Sandbox>, prefix: &str) -> Account {
    // pub async fn dev_generate(&self) -> (AccountId, SecretKey) {
    //     let id = crate::rpc::tool::random_account_id();
    //     let sk = SecretKey::from_seed(KeyType::ED25519, DEV_ACCOUNT_SEED);
    //     (id, sk)
    // }

    // pub async fn dev_create_account(&self) -> Result<Account> {
    //     let (id, sk) = self.dev_generate().await;
    //     let account = self.create_tla(id.clone(), sk).await?;
    //     Ok(account.into_result()?)
    // }

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

#[tokio::test]
async fn happy_path() {
    let (worker, message_repository_wasm, key_registry_wasm) = tokio::join!(
        async { near_workspaces::sandbox().await.unwrap() },
        load_wasm("../message-repository/"),
        load_wasm("../key-registry/"),
    );

    let (message_repository, key_registry) = tokio::join!(
        async {
            let c = prefixed_account(&worker, "msgrepo")
                .await
                .deploy(message_repository_wasm)
                .await
                .unwrap()
                .result;

            c.call("new")
                .args_json(json!({}))
                .transact()
                .await
                .unwrap()
                .unwrap();

            c
        },
        async {
            let c = prefixed_account(&worker, "keyreg")
                .await
                .deploy(key_registry_wasm)
                .await
                .unwrap()
                .result;

            c.call("new")
                .args_json(json!({}))
                .transact()
                .await
                .unwrap()
                .unwrap();

            c
        },
    );

    println!("Message Repository: {}", message_repository.id());
    println!("Key Registry: {}", key_registry.id());
}
