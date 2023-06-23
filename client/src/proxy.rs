use crate::{
    message_repository::MessageRepository,
    messenger::{self, Messenger},
    network_rpc_url,
    traits::Actor,
    wallet::Wallet,
    Base64String, SequencedHashMessage,
};
use anyhow::Result;
use base64ct::{Base64, Encoding};
use near_primitives::types::AccountId;
use std::{path::PathBuf, sync::Arc};
use tokio::{
    select,
    sync::mpsc::{self, Sender},
};
use verify::{
    groth16::{prepare_verifying_key, VerifyingKey},
    Bls12, Proof,
};
use x25519_dalek::StaticSecret;

pub struct Config {
    key_file_path: PathBuf,
    verifying_key_path: PathBuf,
}

impl Config {
    pub fn new(key_file_path: &PathBuf, verifying_key_path: Option<&PathBuf>) -> Self {
        Config {
            key_file_path: key_file_path.clone(),
            verifying_key_path: verifying_key_path.unwrap_or(&"./pvk.key".into()).clone(),
        }
    }
}

#[derive(Debug)]
pub struct Message {
    pub hash: [u8; 32],
    // TODO: hexstring
    pub preimage_proof: Vec<u8>,
    pub sequenced_message: SequencedHashMessage,
}

/// The role of the proxy here is to sign messages on behalf of a user, these messages would be delegated, and the proxy would provide proof that
/// the message:
///     - came from someone within the group (same as the current known-key in current limitation, ideally move to set inclusion proof and we hide the data in the trie)
///     - the message is authentic
///     - someone who knew the current key (inclusive of the group in the current limitations)
///     
pub(crate) struct Proxy {
    messenger: Messenger,
    verifying_key: VerifyingKey<Bls12>,
}

impl Proxy {
    pub fn new(
        config: &Config,
        key_registry_account_id: &AccountId,
        message_repository_account_id: &AccountId,
        messenger_secret_key: &Base64String,
        network: Option<&String>,
    ) -> Result<Self> {
        let signer = near_crypto::InMemorySigner::from_file(&config.key_file_path)?;

        let wallet = Wallet::new(network_rpc_url(network), signer.account_id.clone(), signer);

        let messenger_secret_key: [u8; 32] = Base64::decode_vec(&messenger_secret_key)
            .expect("Failed to decode messenger_secret_key")
            .try_into()
            .expect("Failed to cast messenger_secret_key to bytes");
        log::trace!("Loaded messenger secret key: {:?}", messenger_secret_key);

        let messenger = Messenger::new(
            Arc::new(wallet),
            StaticSecret::from(messenger_secret_key),
            &key_registry_account_id,
            &message_repository_account_id,
        );

        let verifying_key = verify::read_vk(&config.verifying_key_path)?;

        Ok(Proxy {
            messenger,
            verifying_key,
        })
    }
}

impl Actor for Proxy {
    type Message = Message;

    type StartParams = ();

    fn start(self, _params: Self::StartParams) -> Result<Arc<Sender<Self::Message>>> {
        let (sender, mut receiver) = mpsc::channel::<Self::Message>(24);

        let vk = self.verifying_key.clone();
        let message_repository: Arc<MessageRepository> = self.messenger.message_repository.clone();
        Self::spawn(async move {
            select! {
                Some(message) = receiver.recv() => {
                    // TODO: right now we act as a receiver of the message, proving that we can validate the message before we act as someone else
                    // In the future we can: move the verifier onchain so that only good messages are posted OR
                    // Verify in each client before they bother to look at the message, throw it away if it isnt valid
    
    
                    // FIXME: unsafe
                    let proof = Proof::<Bls12>::read_many(&message.preimage_proof, 1).unwrap()[0].clone();
                    if !verify::verify(&proof, &message.hash[..], &prepare_verifying_key(&vk)) {
                        log::debug!(
                            "Could not verify proof for message {:?}",
                            message.sequenced_message
                        )
                    } else {
                        log::info!("Message passed validation, sending");
                        let message_repository = message_repository.clone();
                        if let Err(e) = message_repository
                            .publish_message(&*message.sequenced_message.sequence_hash, &message.sequenced_message.ciphertext)
                        .await {
                            log::error!("Failed to send raw sequenced message {:?}: {:?}", message.sequenced_message.sequence_hash, e);
                        }
                    }
    
                }
            }
        })?;
        Ok(Arc::new(sender))
    }
    
}
