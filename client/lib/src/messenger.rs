use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, bail};
use near_primitives::types::AccountId;
use tokio::sync::RwLock; // TODO: can we remove?
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    channel::{Channel, PairChannel, SequenceHash, SequenceHashProducer},
    key_registry::KeyRegistry,
    message_repository::MessageRepository,
    wallet::Wallet,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecryptedMessage {
    pub block_timestamp_ms: u64,
    pub message: Vec<u8>,
}

pub struct Messenger {
    wallet: Arc<Wallet>,
    secret_key: StaticSecret,
    key_registry: KeyRegistry,
    pub message_repository: MessageRepository,
    conversations: Arc<RwLock<HashMap<AccountId, Arc<Conversation>>>>,
}

impl Messenger {
    pub fn new(
        wallet: Arc<Wallet>,
        messenger_secret_key: StaticSecret,
        key_registry_account_id: &AccountId,
        message_repository_account_id: &AccountId,
    ) -> Self {
        Self {
            secret_key: messenger_secret_key,
            key_registry: KeyRegistry::new(Arc::clone(&wallet), key_registry_account_id),
            message_repository: MessageRepository::new(
                Arc::clone(&wallet),
                message_repository_account_id,
            ),
            conversations: Default::default(),
            wallet,
        }
    }

    pub async fn sync_key(&self) -> anyhow::Result<()> {
        self.key_registry
            .set_my_key(&PublicKey::from(&self.secret_key))
            .await
    }

    pub async fn register_correspondent(&self, account_id: &AccountId) -> anyhow::Result<()> {
        let correspondent_public_key = self.key_registry.get_key_for(account_id).await?;
        let correspondent_public_key: [u8; 32] = match correspondent_public_key.try_into() {
            Ok(a) => a,
            Err(e) => bail!("Invalid key length {}", e.len()),
        };
        let (send, recv) = PairChannel::pair(&self.secret_key, &correspondent_public_key.into());
        let send = MessageStream {
            channel: send,
            sender: self.wallet.account_id.clone(),
            next_nonce: Default::default(),
        };
        let recv = MessageStream {
            channel: recv,
            sender: account_id.clone(),
            next_nonce: Default::default(),
        };

        self.conversations
            .write()
            .await
            .insert(account_id.clone(), Arc::new(Conversation { send, recv }));
        Ok(())
    }

    // pub async fn send(&self, recipient_id: &AccountId, text: &str) -> anyhow::Result<()> {}

    pub async fn send_raw(
        &self,
        recipient_id: &AccountId,
        cleartext: impl AsRef<[u8]>,
    ) -> anyhow::Result<()> {
        let conversations = self.conversations.read().await;
        let conversation = conversations
            .get(recipient_id)
            .ok_or_else(|| anyhow!("Unknown recipient: {}", recipient_id))?;
        let nonce = conversation.send.get_next_nonce();
        let sequence_hash = conversation.send.get_next_sequence_hash();
        let ciphertext = conversation
            .send
            .channel
            .encrypt(nonce, cleartext.as_ref())?;
        // use base64ct::{Base64, Encoding};
        // use console::style;
        // eprintln!(
        //     "Sending message with sequence hash {}",
        //     style(Base64::encode_string(&*sequence_hash)).yellow()
        // );
        self.message_repository
            .publish_message(&*sequence_hash, &ciphertext)
            .await?;

        Ok(())
    }

    pub async fn conversation(
        &self,
        correspondent_id: &AccountId,
    ) -> anyhow::Result<Arc<Conversation>> {
        let conversations = self.conversations.read().await;
        let conversation = conversations
            .get(correspondent_id)
            .ok_or_else(|| anyhow!("Unknown correspondent: {}", correspondent_id))?;

        Ok(Arc::clone(conversation))
    }
}

#[derive(Clone, Debug)]
pub struct MessageStream {
    channel: PairChannel,
    pub sender: AccountId,
    next_nonce: Arc<Mutex<u32>>,
}

impl MessageStream {
    pub async fn synchronize_nonce(
        &self,
        message_repository: &MessageRepository,
    ) -> anyhow::Result<()> {
        *self.next_nonce.lock().unwrap() = message_repository
            .discover_first_unused_nonce(&self.channel)
            .await?;
        Ok(())
    }

    pub async fn receive_next(
        &self,
        message_repository: &MessageRepository,
    ) -> anyhow::Result<Option<DecryptedMessage>> {
        let nonce = self.get_next_nonce();
        let sequence_hash = self.get_next_sequence_hash();

        let response = message_repository.get_message(&*sequence_hash).await?;

        let ciphertext = match response {
            Some(m) => m,
            None => return Ok(None),
        };

        let cleartext = self.channel.decrypt(nonce, &ciphertext.message)?;

        self.advance_nonce();

        Ok(Some(DecryptedMessage {
            message: cleartext,
            block_timestamp_ms: ciphertext.block_timestamp_ms,
        }))
    }

    pub fn get_next_sequence_hash(&self) -> SequenceHash {
        self.channel.sequence_hash(self.get_next_nonce())
    }

    pub fn get_next_nonce(&self) -> u32 {
        *self.next_nonce.lock().unwrap()
    }

    pub fn advance_nonce(&self) {
        *self.next_nonce.lock().unwrap() += 1;
    }
}

pub struct Conversation {
    pub send: MessageStream,
    pub recv: MessageStream,
}
