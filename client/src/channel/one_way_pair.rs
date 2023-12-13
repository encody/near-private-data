use chacha20poly1305::ChaCha20Poly1305;

use crate::channel::{Channel, CorrespondentId};

#[derive(Clone, Debug)]
pub struct OneWayPair {
    pub sender_id: CorrespondentId,
    pub receiver_id: CorrespondentId,
    shared_secret: [u8; 32],
    identifier: [u8; 256],
}

impl Channel for OneWayPair {
    type Cipher = ChaCha20Poly1305;

    fn shared_secret(&self) -> &[u8; 32] {
        &self.shared_secret
    }

    fn secret_identifier(&self) -> &[u8; 256] {
        &self.identifier
    }
}

impl OneWayPair {
    pub(crate) fn new(
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

#[cfg(test)]
mod tests {
    use rand::rngs::OsRng;

    use crate::channel::{one_way_pair::OneWayPair, Channel, SequenceHashProducer};

    #[test]
    fn encryption_decryption() -> anyhow::Result<()> {
        let rng = OsRng;

        let alice = x25519_dalek::StaticSecret::new(rng);
        let bob = x25519_dalek::StaticSecret::new(rng);

        let alice_pub = x25519_dalek::PublicKey::from(&alice);
        let bob_pub = x25519_dalek::PublicKey::from(&bob);

        let (alice_send, alice_recv) = OneWayPair::pair(&alice, &bob_pub);
        let (bob_send, bob_recv) = OneWayPair::pair(&bob, &alice_pub);

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

        let (alice_send, alice_recv) = OneWayPair::pair(&alice, &bob_pub);
        let (bob_send, bob_recv) = OneWayPair::pair(&bob, &alice_pub);

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
