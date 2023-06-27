use crate::Base64String;
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug)]
pub struct Environment {
    pub key_file_path: PathBuf,
    pub network: Option<String>,
    pub messenger_secret_key: Base64String,
    pub key_registry_account_id: AccountId,
    pub message_repository_account_id: AccountId,
    pub proxy_key_file_path: PathBuf,
    pub proxy_verifying_key_path: Option<PathBuf>,
    pub proxy_messenger_secret_key: Base64String,
}
