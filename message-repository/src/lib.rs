use near_sdk::{
    borsh::{self, BorshDeserialize, BorshSerialize},
    near_bindgen, BorshStorageKey, PanicOnDefault,
};
use near_sdk_contract_tools::event;

#[allow(unused)]
#[derive(BorshStorageKey, BorshSerialize)]
enum StorageKey {
    KeyMap,
}

#[event(standard = "x-", version = "1.0.0", serde = "near_sdk::serde")]
enum ContractEvent {
    Event,
}

#[near_bindgen]
#[derive(PanicOnDefault, BorshDeserialize, BorshSerialize)]
pub struct Contract {}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new() -> Self {
        Self {}
    }
}
