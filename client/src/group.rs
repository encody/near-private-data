use crate::channel::PairChannel;
use ed25519_dalek::{Signature, Signer};

pub struct Group {
    channel: PairChannel,
}
