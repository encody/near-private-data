use crate::{
    channel::{Channel, PairChannel, SequenceHash, SequenceHashProducer},
    combined::CombinedMessageStream,
    config::Environment,
    draw, highlight,
    key_registry::KeyRegistry,
    message_repository::MessageRepository,
    network_rpc_url, proxy,
    traits::Actor,
    wallet::Wallet,
};
use anyhow::{anyhow, bail, Result};
use base64ct::{Base64, Encoding};
use console::style;
use near_primitives::types::AccountId;
use sha2::Digest;
use sha2::Sha256;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::{self, Sender};
use verify::{read_params, ShaPreimageProver};
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Debug)]
pub enum Message {
    RawSequenced(SequencedHashMessage),
    Clear(AccountId, String),
    Hidden(AccountId, String),
    RegisterCorrespondent(AccountId),
}

#[derive(Debug)]
pub struct SequencedHashMessage {
    pub sequence_hash: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecryptedMessage {
    pub block_timestamp_ms: u64,
    pub message: Vec<u8>,
}

// TODO: improve the locking here, this is single threaded so can be a manager for a set of messages
pub struct Messenger {
    pub wallet: Arc<Wallet>,
    secret_key: StaticSecret,
    key_registry: KeyRegistry,
    pub message_repository: Arc<MessageRepository>,
    conversations: HashMap<AccountId, Arc<Conversation>>,
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
            message_repository: Arc::new(MessageRepository::new(
                Arc::clone(&wallet),
                message_repository_account_id,
            )),
            conversations: Default::default(),
            wallet,
        }
    }

    pub async fn sync_key(&self) -> anyhow::Result<()> {
        self.key_registry
            .set_my_key(&PublicKey::from(&self.secret_key))
            .await
    }

    pub async fn register_correspondent(&mut self, account_id: &AccountId) -> anyhow::Result<()> {
        log::info!("Registering correspondence with {:?}", account_id);
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
            .insert(account_id.clone(), Arc::new(Conversation { send, recv }));
        Ok(())
    }

    pub async fn send(
        &self,
        recipient_id: &AccountId,
        cleartext: impl AsRef<[u8]>,
    ) -> anyhow::Result<()> {
        let (sequence_hash, ciphertext) = self.encrypt(recipient_id, cleartext)?;
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

    pub fn encrypt(
        &self,
        recipient_id: &AccountId,
        cleartext: impl AsRef<[u8]>,
    ) -> Result<(SequenceHash, Vec<u8>)> {
        let conversation = self
            .conversations
            .get(recipient_id)
            .ok_or_else(|| anyhow!("Unknown recipient: {}", recipient_id))?;
        let nonce = conversation.send.get_next_nonce();
        let sequence_hash = conversation.send.get_next_sequence_hash();
        let ciphertext = conversation
            .send
            .channel
            .encrypt(nonce, cleartext.as_ref())?;
        Ok((sequence_hash, ciphertext))
    }

    pub fn conversation(&self, correspondent_id: &AccountId) -> anyhow::Result<&Arc<Conversation>> {
        self.conversations
            .get(correspondent_id)
            .ok_or_else(|| anyhow!("Unknown correspondent: {}", correspondent_id))
    }

    pub fn init(config: &Environment) -> Result<Self> {
        let signer = near_crypto::InMemorySigner::from_file(&config.key_file_path)?;
        let wallet = Arc::new(Wallet::new(
            network_rpc_url(config.network.as_ref()),
            signer.account_id.clone(),
            signer,
        ));

        let messenger_secret_key: [u8; 32] = Base64::decode_vec(&config.messenger_secret_key)
            .expect("Failed to decode messenger_secret_key")
            .try_into()
            .expect("Failed to cast messenger_secret_key to bytes");
        log::debug!(
            "Loaded messenger secret key: {}",
            config.messenger_secret_key
        );

        Ok(Messenger::new(
            Arc::clone(&wallet),
            StaticSecret::from(messenger_secret_key),
            &config.key_registry_account_id,
            &config.message_repository_account_id,
        ))
    }
}

impl Actor for Messenger {
    type Message = Message;

    type StartParams = (
        Arc<Sender<draw::Message>>,
        Arc<Sender<(AccountId, DecryptedMessage)>>,
        Arc<Sender<proxy::Message>>,
        Arc<Sender<bool>>,
    );

    fn start(mut self, params: Self::StartParams) -> Result<Arc<Sender<Self::Message>>> {
        let (sender, mut receiver) = mpsc::channel::<Self::Message>(4);
        let (draw_tx, messages_tx, proxy_tx, kill_tx) = params;
        let message_repository: Arc<MessageRepository> = self.message_repository.clone();

        Self::spawn(async move {
            let dtx_send = |str| async {
                draw_tx.send(str).await;
            };

            dtx_send(format!(
                "Welcome to the {} (test version)",
                style("NEAR Private Data Messenger").magenta(),
            ))
            .await;

            dtx_send("Syncing public key with key repository...".to_string()).await;
            if let Err(e) = self.sync_key().await {
                log::error!("Failed to sync key {:?}", e);
                kill_tx.send(true).await;
            };
            dtx_send("Done".to_string()).await;

            dtx_send(format!(
                "\rYou are logged in as {}.",
                highlight::account::me(&self.wallet.account_id),
            ))
            .await;

            dtx_send(format!("\r{} to exit.", highlight::text::command("/quit"))).await;

            loop {
                tokio::select! {
                    Some(msg) = receiver.recv() => {
                        match msg {
                            Message::RegisterCorrespondent(correspondent) => {
                                if let Err(e) = self.register_correspondent(&correspondent).await {
                                    log::error!("Failed to register correspondent {:?}", e);
                                } else {
                                    log::info!("Correspondent {:?} registered", correspondent);
                                    let message_repository: Arc<MessageRepository> = message_repository.clone();
                                    let messages_tx = messages_tx.clone();
                                    let conversation = self.conversation(&correspondent).unwrap().clone();
                                    tokio::spawn(async move {
                                        let mut messages = CombinedMessageStream::new(
                                            message_repository,
                                            [&conversation.send, &conversation.recv],
                                        );
                                        log::info!("Conversation initialized");
                                        loop {
                                            if let Some((sender, message)) = messages.next().await.unwrap() {
                                                messages_tx.send((sender.clone(), message)).await.unwrap();
                                            }
                                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                        }
                                    });
                                    dtx_send(format!("Ready to talk with {:?}", correspondent)).await;
                                }
                            },
                            Message::Clear(correspondent, msg) => {
                                if let Err(e) = self.send(&correspondent, msg).await {
                                    log::error!("Failed to send message to {:?}: {:?}", correspondent, e);
                                }
                            },
                            Message::Hidden(correspondent, msg) => {
                                let (sequence_hash, ciphertext) = self.encrypt(&correspondent, msg).unwrap();
                                let preimage = self.secret_key.to_bytes();
                                let hash: [u8; 32] = Sha256::digest(&Sha256::digest(&preimage)).into();
                                let params = read_params(&"/Users/geralt/projects/near-private-data/params.key".into(), false).unwrap();
                                let prover = ShaPreimageProver::<32>::new(preimage, Some(params));
                                let mut preimage_proof: Vec<u8> = vec![];
                                let proof = prover.prove();
                                proof.write(&mut preimage_proof).unwrap();
                                let msg = proxy::Message {
                                    hash,
                                    sequenced_message: SequencedHashMessage { sequence_hash: (*sequence_hash).to_vec(), ciphertext },
                                    preimage_proof
                                };
                                if let Err(e) = proxy_tx.send(msg).await {
                                    log::error!("Failed to send message to proxy: {:?}", e);
                                }
                            },
                            Message::RawSequenced(SequencedHashMessage {sequence_hash, ciphertext}) => {
                                let message_repository = message_repository.clone();
                                if let Err(e) = message_repository
                                    .publish_message(&*sequence_hash, &ciphertext)
                                .await {
                                    log::error!("Failed to send raw sequenced message {:?}: {:?}", sequence_hash, e);
                                }
                            }
                        }

                    }
                }
            }
        })?;
        Ok(Arc::new(sender))
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
