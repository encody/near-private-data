use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    env, near_bindgen,
    store::LookupMap,
    AccountId, BorshStorageKey, PanicOnDefault, Promise, PromiseOrValue,
};
use near_sdk_contract_tools::{event, standard::nep297::Event};

#[allow(unused)]
#[derive(BorshStorageKey, BorshSerialize)]
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
        public_key: Option<String>,
    },
}

#[near_bindgen]
#[derive(PanicOnDefault, BorshDeserialize, BorshSerialize)]
pub struct PublicKeyManagerContract {
    key_map: LookupMap<AccountId, String>,
}

#[near_bindgen]
impl PublicKeyManagerContract {
    #[init]
    pub fn new() -> Self {
        Self {
            key_map: LookupMap::new(StorageKey::KeyMap),
        }
    }

    pub fn get_public_key(&self, account_id: AccountId) -> Option<&String> {
        self.key_map.get(&account_id)
    }

    #[payable]
    pub fn set_public_key(&mut self, public_key: Option<String>) -> PromiseOrValue<()> {
        near_sdk::assert_one_yocto();
        let initial_storage_usage = env::storage_usage();

        self.key_map
            .set(env::predecessor_account_id(), public_key.clone());
        self.key_map.flush();

        PublicKeyManagerEvent::PublicKeyChange {
            account_id: env::predecessor_account_id(),
            public_key,
        }
        .emit();

        let final_storage_usage = env::storage_usage();

        let credit = if final_storage_usage < initial_storage_usage {
            env::attached_deposit()
                + (initial_storage_usage - final_storage_usage) as u128 * env::storage_byte_cost()
        } else {
            let charge =
                (final_storage_usage - initial_storage_usage) as u128 * env::storage_byte_cost();
            env::attached_deposit()
                .checked_sub(charge)
                .unwrap_or_else(|| {
                    env::panic_str(&format!("Requires deposit of at least {charge} yoctoNEAR"))
                })
        };

        if credit > 0 {
            Promise::new(env::predecessor_account_id())
                .transfer(credit)
                .into()
        } else {
            PromiseOrValue::Value(())
        }
    }
}
