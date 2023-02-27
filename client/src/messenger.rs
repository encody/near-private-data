use std::collections::HashMap;

use anyhow::{anyhow, bail};
use near_primitives::types::AccountId;
use tokio::try_join;
use x25519_dalek::StaticSecret;

use crate::{
    channel::{Channel, SequenceHash},
    key_registry::KeyRegistry,
    message_repository::MessageRepository,
    wallet::Wallet,
};

pub struct Messenger<'a> {
    secret_key: StaticSecret,
    key_registry: KeyRegistry<'a>,
    message_repository: MessageRepository<'a>,
    conversations: HashMap<AccountId, Conversation>,
}

impl<'a> Messenger<'a> {
    pub fn new<'b>(
        wallet: &'a Wallet,
        messenger_secret_key: StaticSecret,
        key_registry_account_id: &AccountId,
        message_repository_account_id: &AccountId,
    ) -> Self {
        Self {
            secret_key: messenger_secret_key,
            key_registry: KeyRegistry::new(wallet, key_registry_account_id),
            message_repository: MessageRepository::new(wallet, message_repository_account_id),
            conversations: HashMap::new(),
        }
    }

    pub async fn register_correspondent(&mut self, account_id: &AccountId) -> anyhow::Result<()> {
        let correspondent_public_key = self.key_registry.get_key_for(account_id).await?;
        let correspondent_public_key: [u8; 32] = match correspondent_public_key.try_into() {
            Ok(a) => a,
            Err(e) => bail!("Invalid key length {}", e.len()),
        };
        let (send, recv) = Channel::pair(&self.secret_key, &correspondent_public_key.into());
        let mut send = Thread {
            channel: send,
            _correspondent: account_id.clone(),
            next_nonce: 0,
        };
        let mut recv = Thread {
            channel: recv,
            _correspondent: account_id.clone(),
            next_nonce: 0,
        };

        try_join!(
            send.sync(&self.message_repository),
            recv.sync(&self.message_repository),
        )?;

        self.conversations
            .insert(account_id.clone(), Conversation { send, recv });
        Ok(())
    }

    pub async fn send(
        &mut self,
        recipient_id: &AccountId,
        cleartext: impl AsRef<[u8]>,
    ) -> anyhow::Result<()> {
        let conversation = self
            .conversations
            .get_mut(recipient_id)
            .ok_or_else(|| anyhow!("Unknown recipient: {}", recipient_id))?;
        let nonce = conversation.send.next_nonce;
        let sequence_hash = conversation.send.next_sequence_hash();
        let ciphertext = conversation
            .send
            .channel
            .encrypt(nonce, cleartext.as_ref())?;
        self.message_repository
            .publish_message(&*sequence_hash, &ciphertext)
            .await?;

        conversation.send.next_nonce += 1;

        Ok(())
    }

    pub async fn check_received_one_from(
        &mut self,
        sender_id: &AccountId,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let conversation = self
            .conversations
            .get_mut(sender_id)
            .ok_or_else(|| anyhow!("Unknown recipient: {}", sender_id))?;
        let nonce = conversation.recv.next_nonce;
        let sequence_hash = conversation.recv.next_sequence_hash();

        let response = self.message_repository.get_message(&*sequence_hash).await?;

        let ciphertext = match response {
            Some(m) => m,
            None => return Ok(None),
        };

        let cleartext = conversation.recv.channel.decrypt(nonce, &ciphertext)?;

        conversation.recv.next_nonce += 1;

        Ok(Some(cleartext))
    }
}

pub struct Thread {
    channel: Channel,
    _correspondent: AccountId,
    next_nonce: u32,
}

impl Thread {
    pub async fn sync(&mut self, message_repository: &MessageRepository<'_>) -> anyhow::Result<()> {
        self.next_nonce = message_repository
            .discover_first_unused_nonce(&self.channel)
            .await?;
        Ok(())
    }

    pub fn next_sequence_hash(&self) -> SequenceHash {
        self.channel.sequence_hash(self.next_nonce)
    }
}

pub struct Conversation {
    send: Thread,
    recv: Thread,
}
