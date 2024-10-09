use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::{
    channel::{Channel, CorrespondentId, SequenceHashProducer},
    message_repository::MessageRepository,
    messenger::{DecryptedMessage, MessageStream},
};

pub struct Group {
    message_repository: Arc<MessageRepository>,
    send_messages_from_member_index: usize,
    members: Vec<CorrespondentId>,
    next_message_index: RwLock<Vec<u32>>,
    shared_secret: [u8; 32],
    identifier: [u8; 256],
}

impl Group {
    pub fn new(
        message_repository: Arc<MessageRepository>,
        send_messages_from_member: CorrespondentId,
        mut other_members: Vec<CorrespondentId>,
        shared_secret: [u8; 32],
        context: &[u8],
    ) -> Self {
        other_members.push(send_messages_from_member.clone());
        let mut members = other_members;
        members.sort();
        let send_messages_from_member_index = members
            .iter()
            .position(|m| m == &send_messages_from_member)
            .unwrap(); // unwrap ok because we know this item exists in the vec

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

        let next_message_index =
            RwLock::new(members.iter().enumerate().map(|(i, _)| i as u32).collect());

        Self {
            message_repository,
            members,
            send_messages_from_member_index,
            next_message_index,
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

    pub async fn receive_next_for(
        &self,
        correspondent_index: u32,
    ) -> anyhow::Result<Option<DecryptedMessage>> {
        let message_index = self.next_message_index.read().await[correspondent_index as usize];
        let nonce = self.get_nonce_for_message(message_index, correspondent_index);
        let sequence_hash = self.sequence_hash(nonce);

        let response = self.message_repository.get_message(&*sequence_hash).await?;

        let Some(ciphertext) = response else {
            return Ok(None);
        };

        let cleartext = self.decrypt(nonce, &ciphertext.message)?;

        self.next_message_index.write().await[correspondent_index as usize] += 1;

        Ok(Some(DecryptedMessage {
            message: cleartext,
            block_timestamp_ms: ciphertext.block_timestamp_ms,
        }))
    }

    pub fn streams(&self) -> Vec<GroupStream> {
        self.members
            .iter()
            .enumerate()
            .map(|(i, _)| GroupStream {
                group: self,
                target_correspondent_index: i as u32,
            })
            .collect()
    }

    pub async fn send(
        &self,
        cleartext: impl AsRef<[u8]>,
    ) -> anyhow::Result<()> {
        let message_index = self.next_message_index.read().await[self.send_messages_from_member_index];
        let nonce = self.get_nonce_for_message(message_index, self.send_messages_from_member_index as u32);
        let sequence_hash = self.sequence_hash(nonce);
        let ciphertext = self.encrypt(nonce, cleartext.as_ref())?;
        self.message_repository
            .publish_message(&*sequence_hash, &ciphertext)
            .await?;
        Ok(())
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

pub struct GroupStream<'a> {
    group: &'a Group,
    target_correspondent_index: u32,
}

impl<'a> MessageStream for GroupStream<'a> {
    async fn receive_next(&self) -> anyhow::Result<Option<DecryptedMessage>> {
        self.group
            .receive_next_for(self.target_correspondent_index)
            .await
    }
}

impl<'a> GroupStream<'a> {
    pub fn correspondent_id(&self) -> &CorrespondentId {
        &self.group.members[self.target_correspondent_index as usize]
    }
}
