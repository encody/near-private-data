use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, bail};
use near_primitives::types::AccountId;
use tokio::sync::RwLock; // TODO: can we remove?
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    channel::{Channel, CorrespondentId, PairChannel, SequenceHash, SequenceHashProducer},
    key_registry::KeyRegistry,
    message_repository::MessageRepository,
    wallet::Wallet,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecryptedMessage {
    pub block_timestamp_ms: u64,
    pub message: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredMessage<const KEY_SIZE: usize> {
    pub next_key: [u8; KEY_SIZE],
    pub contents: String,
}

impl<const KEY_SIZE: usize> StructuredMessage<KEY_SIZE> {
    pub const HEADER_MAGIC: [u8; 4] = [88, 88, 88, 88];

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.contents.len());

        buf.extend(Self::HEADER_MAGIC);
        buf.extend(&self.next_key);
        buf.extend(self.contents.as_bytes());

        buf
    }

    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        let header = &bytes[0..4];

        if header != Self::HEADER_MAGIC {
            return None;
        }

        Some(Self {
            next_key: bytes[4..4 + KEY_SIZE].try_into().ok()?,
            contents: String::from_utf8(bytes[4 + KEY_SIZE..].to_vec()).ok()?,
        })
    }
}

#[cfg(test)]
#[test]
fn structured_message_serialization() {
    let sm = StructuredMessage {
        next_key: [0u8; 32],
        contents: "hello".to_string(),
    };

    let bytes = sm.to_bytes();

    assert_eq!(bytes[0..4], StructuredMessage::<32>::HEADER_MAGIC);

    let deserialized = StructuredMessage::try_from_bytes(&bytes).unwrap();

    assert_eq!(sm, deserialized);
}

pub struct Messenger {
    wallet: Arc<Wallet>,
    secret_key: StaticSecret,
    key_registry: KeyRegistry,
    correspondent_map: Arc<RwLock<HashMap<CorrespondentId, AccountId>>>,
    pub message_repository: Arc<MessageRepository>,
    direct_messages: Arc<RwLock<HashMap<AccountId, Arc<DirectMessageConversation>>>>,
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
            correspondent_map: Default::default(),
            message_repository: Arc::new(MessageRepository::new(
                Arc::clone(&wallet),
                message_repository_account_id,
            )),
            direct_messages: Default::default(),
            wallet,
        }
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey::from(&self.secret_key)
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
        let correspondent_id: CorrespondentId = correspondent_public_key.into();
        self.correspondent_map
            .write()
            .await
            .insert(correspondent_id, account_id.clone());
        let (send, recv) = PairChannel::pair(&self.secret_key, &correspondent_public_key.into());
        let send = PairStream {
            channel: send,
            sender: self.wallet.account_id.clone(),
            next_nonce: Default::default(),
            message_repository: Arc::clone(&self.message_repository),
        };
        let recv = PairStream {
            channel: recv,
            sender: account_id.clone(),
            next_nonce: Default::default(),
            message_repository: Arc::clone(&self.message_repository),
        };

        self.direct_messages.write().await.insert(
            account_id.clone(),
            Arc::new(DirectMessageConversation { send, recv }),
        );
        Ok(())
    }

    // pub async fn send(&self, recipient_id: &AccountId, text: &str) -> anyhow::Result<()> {}

    pub async fn send_raw(
        &self,
        recipient_id: &AccountId,
        cleartext: impl AsRef<[u8]>,
    ) -> anyhow::Result<()> {
        let conversations = self.direct_messages.read().await;
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

    pub async fn direct_message(
        &self,
        correspondent_id: &AccountId,
    ) -> anyhow::Result<Arc<DirectMessageConversation>> {
        let conversations = self.direct_messages.read().await;
        let conversation = conversations
            .get(correspondent_id)
            .ok_or_else(|| anyhow!("Unknown correspondent: {}", correspondent_id))?;

        Ok(Arc::clone(conversation))
    }
}

pub trait MessageStream {
    fn receive_next(
        &self,
    ) -> impl std::future::Future<Output = anyhow::Result<Option<DecryptedMessage>>> + Send;
}

#[derive(Clone, Debug)]
pub struct PairStream {
    channel: PairChannel,
    pub sender: AccountId,
    next_nonce: Arc<Mutex<u32>>,
    message_repository: Arc<MessageRepository>,
}

impl MessageStream for PairStream {
    async fn receive_next(&self) -> anyhow::Result<Option<DecryptedMessage>> {
        let nonce = self.get_next_nonce();
        let sequence_hash = self.get_next_sequence_hash();

        let response = self.message_repository.get_message(&*sequence_hash).await?;

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
}

impl PairStream {
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

pub struct DirectMessageConversation {
    pub send: PairStream,
    pub recv: PairStream,
}
