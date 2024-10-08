use std::sync::Arc;

use anyhow::bail;
use data_encoding::BASE64;
use near_primitives::{
    transaction::{Action, FunctionCallAction},
    types::AccountId,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    channel::{PairChannel, SequenceHashProducer},
    wallet::{Wallet, ONE_NEAR, ONE_TERAGAS},
};

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EncryptedMessage {
    pub message: Vec<u8>,
    pub block_timestamp_ms: u64,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EncryptedMessageBase64 {
    pub message: String,
    pub block_timestamp_ms: u64,
}

pub struct MessageRepository {
    wallet: Arc<Wallet>,
    account_id: AccountId,
}

impl MessageRepository {
    pub fn new(wallet: Arc<Wallet>, account_id: &'_ AccountId) -> Self {
        Self {
            wallet,
            account_id: account_id.clone(),
        }
    }

    pub async fn get_message(
        &self,
        sequence_hash: &[u8],
    ) -> anyhow::Result<Option<EncryptedMessage>> {
        let base64_encoded_message: Option<EncryptedMessageBase64> = self
            .wallet
            .view(
                self.account_id.clone(),
                "get_message",
                json!({ "sequence_hash": BASE64.encode(sequence_hash) }),
            )
            .await?;

        let base64_encoded_message = match base64_encoded_message {
            Some(r) => r,
            _ => return Ok(None),
        };

        let message = match BASE64.decode(base64_encoded_message.message.as_bytes()) {
            Ok(d) => d,
            Err(e) => bail!("Error decoding from base64: {}", e),
        };

        Ok(Some(EncryptedMessage {
            message,
            block_timestamp_ms: base64_encoded_message.block_timestamp_ms,
        }))
    }

    pub async fn publish_message(
        &self,
        sequence_hash: &[u8],
        ciphertext: &[u8],
    ) -> anyhow::Result<()> {
        self.wallet
            .transact(
                self.account_id.clone(),
                vec![Action::FunctionCall(Box::new(FunctionCallAction {
                    method_name: "publish".to_string(),
                    args: json!({
                        "sequence_hash": BASE64.encode(sequence_hash),
                        "message": BASE64.encode(ciphertext),
                    })
                    .to_string()
                    .into_bytes(),
                    gas: 300 * ONE_TERAGAS,
                    deposit: ONE_NEAR,
                }))],
            )
            .await?;

        Ok(())
    }

    pub async fn discover_first_unused_nonce(&self, channel: &PairChannel) -> anyhow::Result<u32> {
        // stupid linear search for now.
        // obviously should use some sort of exponential bounds discovery and then binary search,
        // but too lazy to do that now.
        for i in 0.. {
            let sequence_hash = channel.sequence_hash(i);
            if self.get_message(&*sequence_hash).await?.is_none() {
                return Ok(i);
            }
        }

        bail!("Somehow you've sent {} messages", u32::MAX);
    }
}
