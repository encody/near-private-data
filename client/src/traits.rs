use anyhow::Result;
use std::{future::Future, sync::Arc};
use tokio::sync::mpsc::Sender;

pub trait Actor {
    type Message;
    type StartParams;

    fn start(self, params: Self::StartParams) -> Result<Arc<Sender<Self::Message>>>;

    fn spawn<Fut>(f: Fut) -> Result<()>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send + 'static,
    {
        tokio::spawn(f);
        Ok(())
    }
}
