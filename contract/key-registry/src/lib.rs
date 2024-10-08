use near_sdk::{
    collections::LookupMap, env, json_types::Base64VecU8, near, require, AccountId,
    BorshStorageKey, PanicOnDefault, PromiseOrValue,
};
use near_sdk_contract_tools::{event, standard::nep297::Event};

#[derive(Debug, BorshStorageKey)]
#[near]
enum StorageKey {
    KeyMap,
}

#[event(
    standard = "x-public-key-manager",
    version = "1.0.0",
    serde = "near_sdk::serde"
)]
enum PublicKeyManagerEvent {
    PublicKeyChange {
        account_id: AccountId,
        public_key: Option<Base64VecU8>,
    },
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct PublicKeyManagerContract {
    key_map: LookupMap<AccountId, Base64VecU8>,
}

#[near]
impl PublicKeyManagerContract {
    #[init]
    pub fn new() -> Self {
        Self {
            key_map: LookupMap::new(StorageKey::KeyMap),
        }
    }

    pub fn get_public_key(&self, account_id: AccountId) -> Option<Base64VecU8> {
        self.key_map.get(&account_id)
    }

    #[payable]
    pub fn set_public_key(&mut self, public_key: Option<Base64VecU8>) -> PromiseOrValue<()> {
        require!(!env::attached_deposit().is_zero(), "Requires deposit");
        let initial_storage_usage = env::storage_usage();

        let predecessor = env::predecessor_account_id();
        if let Some(public_key) = public_key.as_ref() {
            self.key_map.insert(&predecessor, public_key);
        } else {
            self.key_map.remove(&predecessor);
        }

        PublicKeyManagerEvent::PublicKeyChange {
            account_id: env::predecessor_account_id(),
            public_key,
        }
        .emit();

        if let Some(p) =
            near_sdk_contract_tools::utils::apply_storage_fee_and_refund(initial_storage_usage, 0)
        {
            PromiseOrValue::Promise(p)
        } else {
            PromiseOrValue::Value(())
        }
    }
}
