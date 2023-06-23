use std::sync::Arc;

use near_primitives::types::AccountId;

use crate::{
    message_repository::MessageRepository,
    messenger::{DecryptedMessage, MessageStream},
};

struct BufferedMessageStream<'a> {
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
            let next_message_timestamp = if let Some(next_message) = &stream.next_message {
                Some(next_message.block_timestamp_ms)
            } else {
                let next_message = stream.stream.receive_next(&self.message_repository).await?;
                if let Some(next_message) = next_message {
                    let timestamp = next_message.block_timestamp_ms;
                    stream.next_message = Some(next_message);
                    Some(timestamp)
                } else {
                    None
                }
            };

            if let Some(next_message_timestamp) = next_message_timestamp {
                match stream_index_with_oldest_message {
                    Some((oldest_timestamp, _)) if oldest_timestamp < next_message_timestamp => {}
                    _ => {
                        stream_index_with_oldest_message = Some((next_message_timestamp, i));
                    }
                }
            }
        }

        Ok(stream_index_with_oldest_message.map(|(_, i)| {
            let stream = &mut self.streams[i];
            let next_message = stream.next_message.take().unwrap();
            (&stream.stream.sender, next_message)
        }))
    }
}
