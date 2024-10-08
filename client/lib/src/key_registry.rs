use std::sync::Arc;

use anyhow::bail;
use data_encoding::BASE64;
use near_primitives::{
    transaction::{Action, FunctionCallAction},
    types::AccountId,
};
use serde_json::json;

use crate::wallet::{Wallet, ONE_NEAR, ONE_TERAGAS};

fn public_key_to_string(public_key: &x25519_dalek::PublicKey) -> String {
    BASE64.encode(public_key.as_bytes())
}

pub struct KeyRegistry {
    wallet: Arc<Wallet>,
    account_id: AccountId,
}

impl KeyRegistry {
    pub fn new(wallet: Arc<Wallet>, account_id: &'_ AccountId) -> Self {
        Self {
            wallet,
            account_id: account_id.clone(),
        }
    }

    pub async fn get_my_key(&self) -> anyhow::Result<Vec<u8>> {
        self.get_key_for(&self.wallet.account_id).await
    }

    pub async fn get_key_for(&self, account_id: &AccountId) -> anyhow::Result<Vec<u8>> {
        let response: String = self
            .wallet
            .view(
                self.account_id.clone(),
                "get_public_key",
                json!({ "account_id": account_id }),
            )
            .await?;

        let response = match BASE64.decode(response.as_bytes()) {
            Ok(v) => v,
            Err(e) => bail!("Could not decode: {}", e),
        };

        Ok(response)
    }

    pub async fn set_my_key(&self, public_key: &x25519_dalek::PublicKey) -> anyhow::Result<()> {
        self.wallet
            .transact(
                self.account_id.clone(),
                vec![Action::FunctionCall(Box::new(FunctionCallAction {
                    method_name: "set_public_key".to_string(),
                    args: json!({
                        "public_key": public_key_to_string(public_key),
                    })
                    .to_string()
                    .into_bytes(),
                    gas: 3 * ONE_TERAGAS,
                    deposit: ONE_NEAR / 2,
                }))],
            )
            .await?;

        Ok(())
    }
}
