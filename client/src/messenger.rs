use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, bail};
use near_primitives::types::AccountId;
use tokio::{sync::RwLock, try_join};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    channel::{Channel, SequenceHash},
    key_registry::KeyRegistry,
    message_repository::MessageRepository,
    wallet::Wallet,
};

pub struct Messenger {
    secret_key: StaticSecret,
    key_registry: KeyRegistry,
    message_repository: MessageRepository,
    conversations: Arc<RwLock<HashMap<AccountId, Conversation>>>,
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
            message_repository: MessageRepository::new(wallet, message_repository_account_id),
            conversations: Default::default(),
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
        let (send, recv) = Channel::pair(&self.secret_key, &correspondent_public_key.into());
        let send = Thread {
            channel: send,
            _correspondent: account_id.clone(),
            next_nonce: Default::default(),
        };
        let recv = Thread {
            channel: recv,
            _correspondent: account_id.clone(),
            next_nonce: Default::default(),
        };

        try_join!(
            send.sync(&self.message_repository),
            recv.sync(&self.message_repository),
        )?;

        self.conversations
            .write()
            .await
            .insert(account_id.clone(), Conversation { send, recv });
        Ok(())
    }

    pub async fn send(
        &self,
        recipient_id: &AccountId,
        cleartext: impl AsRef<[u8]>,
    ) -> anyhow::Result<()> {
        let conversations = self.conversations.read().await;
        let conversation = conversations
            .get(recipient_id)
            .ok_or_else(|| anyhow!("Unknown recipient: {}", recipient_id))?;
        let nonce = conversation.send.next_nonce();
        let sequence_hash = conversation.send.next_sequence_hash();
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

        conversation.send.advance_nonce();

        Ok(())
    }

    pub async fn receive_one_from(&self, sender_id: &AccountId) -> anyhow::Result<Option<Vec<u8>>> {
        let conversations = self.conversations.read().await;
        let conversation = conversations
            .get(sender_id)
            .ok_or_else(|| anyhow!("Unknown recipient: {}", sender_id))?;
        let nonce = conversation.recv.next_nonce();
        let sequence_hash = conversation.recv.next_sequence_hash();

        let response = self.message_repository.get_message(&*sequence_hash).await?;

        let ciphertext = match response {
            Some(m) => m,
            None => return Ok(None),
        };

        let cleartext = conversation.recv.channel.decrypt(nonce, &ciphertext)?;

        conversation.recv.advance_nonce();

        Ok(Some(cleartext))
    }
}

pub struct Thread {
    channel: Channel,
    _correspondent: AccountId,
    next_nonce: Arc<Mutex<u32>>,
}

impl Thread {
    pub async fn sync(&self, message_repository: &MessageRepository) -> anyhow::Result<()> {
        *self.next_nonce.lock().unwrap() = message_repository
            .discover_first_unused_nonce(&self.channel)
            .await?;
        Ok(())
    }

    pub fn next_sequence_hash(&self) -> SequenceHash {
        self.channel.sequence_hash(self.next_nonce())
    }

    pub fn next_nonce(&self) -> u32 {
        *self.next_nonce.lock().unwrap()
    }

    pub fn advance_nonce(&self) {
        *self.next_nonce.lock().unwrap() += 1;
    }
}

pub struct Conversation {
    send: Thread,
    recv: Thread,
}
