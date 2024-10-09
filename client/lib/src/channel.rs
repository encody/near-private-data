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
            serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
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

fn u32_to_nonce(u: u32) -> Nonce {
    Nonce::from_exact_iter([u.to_le_bytes(), [0u8; 4], [0u8; 4]].concat()).unwrap()
}

pub trait Channel {
    fn encrypt(&self, nonce: u32, message: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(self.shared_secret())?;
        let nonce = u32_to_nonce(nonce);
        let ciphertext = match cipher.encrypt(&nonce, message) {
            Ok(c) => c,
            Err(e) => bail!(e),
        };
        Ok(ciphertext)
    }

    fn decrypt(&self, nonce: u32, message: &[u8]) -> anyhow::Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(self.shared_secret())?;
        let nonce = u32_to_nonce(nonce);
        let cleartext = match cipher.decrypt(&nonce, message) {
            Ok(c) => c,
            Err(e) => bail!(e),
        };
        Ok(cleartext)
    }

    fn secret_identifier(&self) -> &[u8; 256];

    fn shared_secret(&self) -> &[u8; 32];
}

pub trait SequenceHashProducer {
    fn sequence_hash(&self, sequence_number: SequenceNumber) -> SequenceHash;
}

impl<T: Channel> SequenceHashProducer for T {
    fn sequence_hash(&self, sequence_number: SequenceNumber) -> SequenceHash {
        let hash_bytes: [u8; 32] = <Sha256 as Digest>::new()
            .chain_update(sequence_number.to_le_bytes())
            .chain_update(self.secret_identifier())
            .finalize()
            .into();

        SequenceHash(hash_bytes)
    }
}
