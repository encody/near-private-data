use chacha20poly1305::ChaCha20Poly1305;
use ed25519_dalek::{Signature, Signer};

use crate::channel::{Channel, CorrespondentId};

pub struct Group {
    participants: Vec<CorrespondentId>,
    secret: [u8; 32],
    identifier: [u8; 256],
}

impl Group {
    pub fn new(participants: Vec<CorrespondentId>, secret: [u8; 32]) -> Self {
        // Construct identifier from Merkle tree of participants
    }
}

impl Channel for Group {
    type Cipher = ChaCha20Poly1305;

    fn shared_secret(&self) -> &[u8; 32] {
        &self.secret
    }

    fn secret_identifier(&self) -> &[u8; 256] {
        &self.identifier
    }
}
