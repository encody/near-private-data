use anyhow::bail;
use base64ct::{Base64, Encoding};
use near_primitives::{
    transaction::{Action, FunctionCallAction},
    types::AccountId,
};
use serde_json::json;

use crate::wallet::{Wallet, ONE_NEAR, ONE_TERAGAS};

pub struct MessageRepository<'a> {
    wallet: &'a Wallet,
    account_id: AccountId,
}

impl<'a> MessageRepository<'a> {
    pub fn new(wallet: &'a Wallet, account_id: &'_ AccountId) -> Self {
        Self {
            wallet,
            account_id: account_id.clone(),
        }
    }

    pub async fn get_message(&self, sequence_hash: &[u8]) -> anyhow::Result<Vec<u8>> {
        let base64_encoded_message: Option<String> = self
            .wallet
            .view::<Option<String>>(
                self.account_id.clone(),
                "get_message",
                json!({ "sequence_hash": Base64::encode_string(sequence_hash) }),
            )
            .await?;

        let base64_encoded_message = match base64_encoded_message {
            Some(r) => r,
            _ => bail!("Message not found"),
        };

        let message = match Base64::decode_vec(&base64_encoded_message) {
            Ok(d) => d,
            Err(e) => bail!("Error decoding from base64: {}", e),
        };

        Ok(message)
    }

    pub async fn publish_message(
        &self,
        sequence_hash: &[u8],
        message: &[u8],
    ) -> anyhow::Result<()> {
        self.wallet
            .transact(
                self.account_id.clone(),
                vec![Action::FunctionCall(FunctionCallAction {
                    method_name: "publish".to_string(),
                    args: json!({
                        "sequence_hash": Base64::encode_string(sequence_hash),
                        "message": Base64::encode_string(message),
                    })
                    .to_string()
                    .into_bytes()
                    .into(),
                    gas: 300 * ONE_TERAGAS,
                    deposit: ONE_NEAR,
                })],
            )
            .await?;

        Ok(())
    }
}
