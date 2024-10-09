use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{
    channel::{Channel, CorrespondentId, SequenceHash, SequenceHashProducer},
    message_repository::MessageRepository,
    messenger::{DecryptedMessage, MessageStream},
};

pub struct Group {
    members: Vec<CorrespondentId>,
    shared_secret: [u8; 32],
    identifier: [u8; 256],
}

impl Group {
    pub fn new(mut members: Vec<CorrespondentId>, shared_secret: [u8; 32], context: &[u8]) -> Self {
        members.sort();

        let mut members_hash = <Sha256 as Digest>::new();
        for member in members.iter() {
            members_hash.update(member);
        }
        let members_hash = members_hash.finalize();

        let context_hash = <Sha256 as Digest>::new().chain_update(context).finalize();

        let mut identifier = [0u8; 256];
        identifier[0..32].copy_from_slice(&members_hash);
        identifier[64..96].copy_from_slice(&shared_secret);
        identifier[96..128].copy_from_slice(&context_hash);

        Self {
            members,
            shared_secret,
            identifier,
        }
    }

    pub fn get_correspondent_index(&self, correspondent_id: &CorrespondentId) -> Option<u32> {
        self.members
            .iter()
            .position(|m| m == correspondent_id)
            .map(|i| i as u32)
    }

    pub fn get_nonce_for_message(&self, message_index: u32, correspondent_index: u32) -> u32 {
        self.members.len() as u32 * message_index + correspondent_index
    }

    pub fn streams(
        self: Arc<Self>,
        message_repository: Arc<MessageRepository>,
    ) -> Vec<GroupStream> {
        self.members
            .iter()
            .enumerate()
            .map(|(i, _c)| GroupStream {
                group: Arc::clone(&self),
                target_correspondent_index: i as u32,
                next_message_index: Default::default(),
                message_repository: Arc::clone(&message_repository),
            })
            .collect()
    }
}

impl Channel for Group {
    fn secret_identifier(&self) -> &[u8; 256] {
        &self.identifier
    }

    fn shared_secret(&self) -> &[u8; 32] {
        &self.shared_secret
    }
}

pub struct GroupStream {
    group: Arc<Group>,
    target_correspondent_index: u32,
    next_message_index: Arc<Mutex<u32>>,
    message_repository: Arc<MessageRepository>,
}

impl MessageStream for GroupStream {
    async fn receive_next(&self) -> anyhow::Result<Option<DecryptedMessage>> {
        let nonce = self.get_next_nonce().await;
        let sequence_hash = self.get_sequence_hash(nonce).await;

        let response = self.message_repository.get_message(&*sequence_hash).await?;

        let Some(ciphertext) = response else {
            return Ok(None);
        };

        let cleartext = self.group.decrypt(nonce, &ciphertext.message)?;

        self.next_nonce().await;

        Ok(Some(DecryptedMessage {
            message: cleartext,
            block_timestamp_ms: ciphertext.block_timestamp_ms,
        }))
    }
}

impl GroupStream {
    async fn get_sequence_hash(&self, nonce: u32) -> SequenceHash {
        self.group.sequence_hash(nonce)
    }

    async fn get_next_nonce(&self) -> u32 {
        let message_index = *self.next_message_index.lock().await;
        self.group
            .get_nonce_for_message(message_index, self.target_correspondent_index)
    }

    async fn next_nonce(&self) {
        *self.next_message_index.lock().await += 1;
    }
}
