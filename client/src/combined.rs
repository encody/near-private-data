use near_primitives::types::AccountId;
use std::{ops::DerefMut, pin::Pin, sync::Arc};

use crate::{
    message_repository::MessageRepository,
    messenger::{DecryptedMessage, MessageStream},
};

pub(crate) struct BufferedMessageStream<'a> {
    stream: &'a MessageStream,
    next_message: Option<DecryptedMessage>,
}

pub struct CombinedMessageStream<'a> {
    message_repository: Arc<MessageRepository>,
    streams: Vec<BufferedMessageStream<'a>>,
}

impl<'a> CombinedMessageStream<'a> {
    pub fn new(
        message_repository: Arc<MessageRepository>,
        streams: impl AsRef<[&'a MessageStream]>,
    ) -> Self {
        Self {
            message_repository,
            streams: streams
                .as_ref()
                .iter()
                .map(|stream| BufferedMessageStream {
                    stream,
                    next_message: None,
                })
                .collect(),
        }
    }

    pub async fn next(&mut self) -> anyhow::Result<Option<(&'a AccountId, DecryptedMessage)>> {
        let mut stream_index_with_oldest_message = None;

        for (i, stream) in self.streams.iter_mut().enumerate() {
            set_oldest_message(
                &mut stream_index_with_oldest_message,
                i,
                stream,
                stream.stream.receive_next(&self.message_repository),
            )
            .await?
        }

        // for (i, stream) in self.streams.iter_mut().enumerate() {
        //     let next_message_timestamp = if let Some(next_message) = &stream.next_message {
        //         Some(next_message.block_timestamp_ms)
        //     } else {
        //         if let Some(next_message) =
        //             stream.stream.receive_next(&self.message_repository).await?
        //         {
        //             let timestamp = next_message.block_timestamp_ms;
        //             stream.next_message = Some(next_message);
        //             Some(timestamp)
        //         } else {
        //             None
        //         }
        //     };

        //     if let Some(next_message_timestamp) = next_message_timestamp {
        //         match stream_index_with_oldest_message {
        //             Some((oldest_timestamp, _)) if oldest_timestamp < next_message_timestamp => {}
        //             _ => {
        //                 stream_index_with_oldest_message = Some((next_message_timestamp, i));
        //             }
        //         }
        //     }
        // }

        Ok(stream_index_with_oldest_message.map(|(_, i)| {
            let stream = &mut self.streams[i];
            get_stream_message(i, stream)
        }))
    }
}

pub(crate) fn get_stream_message<'a>(
    i: usize,
    stream: &mut BufferedMessageStream<'a>,
) -> (&'a AccountId, DecryptedMessage) {
    let next_message = stream.next_message.take().unwrap();
    let stream_name = if i == 0 { "send" } else { "recv" };
    log::debug!("[{}] next msg: {:?}", stream_name, next_message);
    (&stream.stream.sender, next_message)
}
pub(crate) async fn set_oldest_message<'a, F>(
    stream_index_with_oldest_message: &mut Option<(u64, usize)>,
    i: usize,
    stream: &mut BufferedMessageStream<'a>,
    get_next: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<Option<DecryptedMessage>>>,
{
    let next_message_timestamp = if let Some(next_message) = &stream.next_message {
        Some(next_message.block_timestamp_ms)
    } else {
        if let Some(next_message) = get_next.await? {
            let timestamp = next_message.block_timestamp_ms;
            stream.next_message = Some(next_message);
            Some(timestamp)
        } else {
            None
        }
    };
    println!("Next {:?}", next_message_timestamp);

    if let Some(next_message_timestamp) = next_message_timestamp {
        match stream_index_with_oldest_message {
            Some((oldest_timestamp, _)) if *oldest_timestamp < next_message_timestamp => {}
            _ => {
                *stream_index_with_oldest_message = Some((next_message_timestamp, i));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::PairChannel;
    use rand::{distributions::Standard, Rng};
    use std::sync::Mutex;
    use x25519_dalek::PublicKey;

    pub struct PairPkey {
        account: AccountId,
        pkey: PublicKey,
    }

    pub fn pair() -> (PairPkey, PairPkey) {
        let alice = PairPkey {
            account: "alice.near".parse().unwrap(),
            pkey: [0u8; 32].into(),
        };
        let bob = PairPkey {
            account: "bob.near".parse().unwrap(),
            pkey: [1u8; 32].into(),
        };

        (alice, bob)
    }

    pub fn new_message(i: usize) -> DecryptedMessage {
        DecryptedMessage {
            block_timestamp_ms: i as u64,
            message: vec![i as u8],
        }
    }

    #[tokio::test]
    async fn test_set_oldest_message() {
        let (alice, bob) = pair();
        let secret = [3u8; 32];
        let dummy_msg_stream = MessageStream::new(
            PairChannel::new(&alice.pkey, &bob.pkey, secret),
            alice.account,
            Arc::new(Mutex::new(0)),
        );

        let mut stream_index_with_oldest_message = None;

        let check_inner =
            |nts: u64, nindex: usize, stream_index_with_oldest_message: Option<(u64, usize)>| {
                let (ts, i) = stream_index_with_oldest_message.unwrap();
                assert_eq!(ts, nts);
                assert_eq!(i, nindex);
            };

        let mut stream = BufferedMessageStream {
            stream: &dummy_msg_stream.clone(),
            next_message: None,
        };

        // No awaiting, no next message, oldest message is none
        set_oldest_message(
            &mut stream_index_with_oldest_message,
            0,
            &mut stream,
            async { Ok(None) },
        )
        .await
        .unwrap();
        assert!(stream_index_with_oldest_message.is_none());

        // No awaiting, next message 1, oldest message should be 1
        let next_message = new_message(1);
        stream.next_message = Some(next_message.clone());
        set_oldest_message(
            &mut stream_index_with_oldest_message,
            1,
            &mut stream,
            async { Ok(None) },
        )
        .await
        .unwrap();
        check_inner(
            next_message.block_timestamp_ms,
            1,
            stream_index_with_oldest_message.clone(),
        );

        // Prev message 1, next message 2, oldest is 1
        let next_message = new_message(2);
        stream.next_message = Some(next_message.clone());
        set_oldest_message(
            &mut stream_index_with_oldest_message,
            2,
            &mut stream,
            async { Ok(None) },
        )
        .await
        .unwrap();
        check_inner(1, 1, stream_index_with_oldest_message.clone());

        // Prev message 2, new message, oldest is 1
        set_oldest_message(
            &mut stream_index_with_oldest_message,
            2,
            &mut stream,
            async { Ok(Some(new_message(3))) },
        )
        .await
        .unwrap();
        check_inner(1, 1, stream_index_with_oldest_message.clone());
    }

    #[tokio::test]
    async fn test_combined_streams_black_box() {
        let (alice, bob) = pair();
        let secret = [3u8; 32];

        let mut stream_index_with_oldest_message = None;

        let check_inner =
            |nts: u64, nindex: usize, stream_index_with_oldest_message: Option<(u64, usize)>| {
                let (ts, i) = stream_index_with_oldest_message.unwrap();
                assert_eq!(ts, nts);
                assert_eq!(i, nindex);
            };

        let mut send = BufferedMessageStream {
            stream: &MessageStream::new(
                PairChannel::new(&alice.pkey, &bob.pkey, secret),
                alice.account,
                Arc::new(Mutex::new(0)),
            )
            .clone(),
            next_message: None,
        };

        let mut recv = BufferedMessageStream {
            stream: &MessageStream::new(
                PairChannel::new(&bob.pkey, &alice.pkey, secret),
                bob.account,
                Arc::new(Mutex::new(0)),
            )
            .clone(),
            next_message: None,
        };

        let mut rng = rand::thread_rng();
        let num: Vec<u32> = rng.sample_iter(Standard).take(100).collect();
        let mut bootstrap: Vec<DecryptedMessage> =
            num.iter().map(|i| new_message(*i as usize)).collect();
        let mut bootstrap_smallest = num.clone();
        let lowest_number = bootstrap_smallest.iter().min();

        let mut streams = vec![send, recv];

        for (msg) in bootstrap.iter() {
            for (i, stream) in streams.iter_mut().enumerate() {
                set_oldest_message(&mut stream_index_with_oldest_message, i, stream, async {
                    Ok(Some(msg.clone()))
                })
                .await
                .unwrap()
            }

            println!(
                "Stream with oldest message {:?}",
                stream_index_with_oldest_message
            );

            let next = stream_index_with_oldest_message.map(|(_, i)| {
                let stream = &mut streams[i];
                get_stream_message(i, stream)
            });
        }

        assert_eq!(stream_index_with_oldest_message.unwrap().0, (*lowest_number.unwrap()) as u64);
    }
}
