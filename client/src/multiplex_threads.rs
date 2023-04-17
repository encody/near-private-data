use near_primitives::types::AccountId;

use crate::{
    message_repository::MessageRepository,
    messenger::{DecryptedMessage, Thread},
};

struct ThreadAndNextMessage<'a> {
    thread: &'a Thread,
    next_message: Option<DecryptedMessage>,
}

pub struct MultiplexedThreads<'a> {
    message_repository: &'a MessageRepository,
    threads: Vec<ThreadAndNextMessage<'a>>,
}

impl<'a> MultiplexedThreads<'a> {
    pub async fn start(
        message_repository: &'a MessageRepository,
        threads: impl AsRef<[&'a Thread]>,
    ) -> anyhow::Result<MultiplexedThreads<'a>> {
        let threads = futures::future::try_join_all(threads.as_ref().iter().map(|thread| async {
            Ok::<_, anyhow::Error>(ThreadAndNextMessage {
                thread,
                next_message: thread.receive_next(message_repository).await?,
            })
        }))
        .await?;

        Ok(Self {
            message_repository,
            threads,
        })
    }

    pub async fn next(&mut self) -> anyhow::Result<Option<(AccountId, DecryptedMessage)>> {
        let mut thread_with_oldest_message = None;

        for (i, thread) in self.threads.iter_mut().enumerate() {
            let thread_timestamp = if let Some(next_message) = &thread.next_message {
                Some(next_message.block_timestamp_ms)
            } else {
                let next_message = thread.thread.receive_next(self.message_repository).await?;
                if let Some(next_message) = next_message {
                    let timestamp = next_message.block_timestamp_ms;
                    thread.next_message = Some(next_message);
                    Some(timestamp)
                } else {
                    None
                }
            };

            if let Some(thread_timestamp) = thread_timestamp {
                match thread_with_oldest_message {
                    Some((oldest_timestamp, _)) if oldest_timestamp < thread_timestamp => {}
                    _ => {
                        thread_with_oldest_message = Some((thread_timestamp, i));
                    }
                }
            }
        }

        Ok(thread_with_oldest_message.map(|(_, i)| {
            let thread = &mut self.threads[i];
            let next_message = thread.next_message.take().unwrap();
            (thread.thread.sender.clone(), next_message)
        }))
    }
}
