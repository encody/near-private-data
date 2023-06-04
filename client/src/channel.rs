use std::ops::Deref;

use anyhow::bail;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use sha2::{Digest, Sha256};

pub type SequenceNumber = u32;

macro_rules! thin_marker {
    ($name: ident, $target: ty, $as_ref: ty) => {
        #[derive(
            serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, PartialOrd, Hash,
        )]
        pub struct $name($target);

        impl Deref for $name {
            type Target = $target;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl AsRef<$as_ref> for $name {
            fn as_ref(&self) -> &$as_ref {
                &self.0
            }
        }

        impl From<$target> for $name {
            fn from(value: $target) -> Self {
                Self(value)
            }
        }
    };
}

thin_marker!(CorrespondentId, [u8; 32], [u8]);
thin_marker!(SequenceHash, [u8; 32], [u8]);

pub trait Channel {
    fn encrypt(&self, nonce: u32, message: &[u8]) -> anyhow::Result<Vec<u8>>;

    fn decrypt(&self, nonce: u32, message: &[u8]) -> anyhow::Result<Vec<u8>>;

    fn secret_identifier(&self) -> &[u8; 256];
}

pub trait SequenceHashProducer {
    fn sequence_hash(&self, sequence_number: SequenceNumber) -> SequenceHash;
}

impl<T: Channel> SequenceHashProducer for T {
    fn sequence_hash(&self, sequence_number: SequenceNumber) -> SequenceHash {
        let hash_bytes: [u8; 32] = Sha256::new()
            .chain_update(sequence_number.to_le_bytes())
            .chain_update(self.secret_identifier())
            .finalize()
            .into();

        SequenceHash(hash_bytes)
    }
}

#[derive(Clone, Debug)]
pub struct PairChannel {
    pub sender_id: CorrespondentId,
    pub receiver_id: CorrespondentId,
    shared_secret: [u8; 32],
    identifier: [u8; 256],
}

impl Channel for PairChannel {
    fn secret_identifier(&self) -> &[u8; 256] {
        &self.identifier
    }

    fn encrypt(&self, nonce: u32, message: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.shared_secret)?;
        let nonce = u32_to_nonce(nonce);
        let ciphertext = match cipher.encrypt(&nonce, message) {
            Ok(c) => c,
            Err(e) => bail!(e),
        };
        Ok(ciphertext)
    }

    fn decrypt(&self, nonce: u32, message: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(&self.shared_secret)?;
        let nonce = u32_to_nonce(nonce);
        let cleartext = match cipher.decrypt(&nonce, message) {
            Ok(c) => c,
            Err(e) => bail!(e),
        };
        Ok(cleartext)
    }
}

impl PairChannel {
    fn new(
        sender_id: &x25519_dalek::PublicKey,
        receiver_id: &x25519_dalek::PublicKey,
        shared_secret: [u8; 32],
    ) -> Self {
        let sender_id = sender_id.to_bytes();
        let receiver_id = receiver_id.to_bytes();
        let mut identifier = [0u8; 256];
        identifier[..32].copy_from_slice(&sender_id);
        identifier[32..64].copy_from_slice(&receiver_id);
        identifier[64..96].copy_from_slice(&shared_secret);
        Self {
            sender_id: sender_id.into(),
            receiver_id: receiver_id.into(),
            shared_secret,
            identifier,
        }
    }

    pub fn pair(
        sender_secret: &x25519_dalek::StaticSecret,
        receiver_public_key: &x25519_dalek::PublicKey,
    ) -> (Self, Self) {
        let shared_secret = sender_secret.diffie_hellman(receiver_public_key).to_bytes();
        let sender_public_key = x25519_dalek::PublicKey::from(sender_secret);

        let send_channel = Self::new(&sender_public_key, receiver_public_key, shared_secret);
        let receive_channel = Self::new(receiver_public_key, &sender_public_key, shared_secret);

        (send_channel, receive_channel)
    }
}

fn u32_to_nonce(u: u32) -> Nonce {
    Nonce::from_exact_iter([u.to_le_bytes(), [0u8; 4], [0u8; 4]].concat().into_iter()).unwrap()
}

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;

    use crate::channel::{Channel, PairChannel, SequenceHashProducer};

    #[test]
    fn encryption_decryption() -> anyhow::Result<()> {
        let rng = OsRng;

        let alice = x25519_dalek::StaticSecret::new(rng);
        let bob = x25519_dalek::StaticSecret::new(rng);

        let alice_pub = x25519_dalek::PublicKey::from(&alice);
        let bob_pub = x25519_dalek::PublicKey::from(&bob);

        let (alice_send, alice_recv) = PairChannel::pair(&alice, &bob_pub);
        let (bob_send, bob_recv) = PairChannel::pair(&bob, &alice_pub);

        let cleartext = b"hello, world";

        let alice_sends_ciphertext = alice_send.encrypt(0, cleartext)?;
        let bob_receives_cleartext = bob_recv.decrypt(0, &alice_sends_ciphertext)?;

        assert_eq!(&bob_receives_cleartext, cleartext);

        let cleartext = b"once upon a time";

        let bob_sends_ciphertext = bob_send.encrypt(1, cleartext)?;
        let alice_receives_cleartext = alice_recv.decrypt(1, &bob_sends_ciphertext)?;

        assert_eq!(&alice_receives_cleartext, cleartext);

        Ok(())
    }

    #[test]
    fn sequence_hashes() {
        let rng = OsRng;

        let alice = x25519_dalek::StaticSecret::new(rng);
        let bob = x25519_dalek::StaticSecret::new(rng);

        let alice_pub = x25519_dalek::PublicKey::from(&alice);
        let bob_pub = x25519_dalek::PublicKey::from(&bob);

        let (alice_send, alice_recv) = PairChannel::pair(&alice, &bob_pub);
        let (bob_send, bob_recv) = PairChannel::pair(&bob, &alice_pub);

        assert_eq!(alice_send.sequence_hash(0), bob_recv.sequence_hash(0));
        assert_eq!(alice_send.sequence_hash(1), bob_recv.sequence_hash(1));
        assert_eq!(alice_send.sequence_hash(2), bob_recv.sequence_hash(2));
        assert_eq!(alice_send.sequence_hash(3), bob_recv.sequence_hash(3));
        assert_eq!(alice_send.sequence_hash(4), bob_recv.sequence_hash(4));

        assert_eq!(bob_send.sequence_hash(0), alice_recv.sequence_hash(0));
        assert_eq!(bob_send.sequence_hash(1), alice_recv.sequence_hash(1));
        assert_eq!(bob_send.sequence_hash(2), alice_recv.sequence_hash(2));
        assert_eq!(bob_send.sequence_hash(3), alice_recv.sequence_hash(3));
        assert_eq!(bob_send.sequence_hash(4), alice_recv.sequence_hash(4));

        assert_ne!(alice_send.sequence_hash(0), bob_recv.sequence_hash(1));
        assert_ne!(bob_send.sequence_hash(0), alice_recv.sequence_hash(1));
        assert_ne!(alice_send.sequence_hash(0), alice_recv.sequence_hash(0));
        assert_ne!(alice_send.sequence_hash(0), bob_send.sequence_hash(0));
    }
}
