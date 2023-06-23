use anyhow::Result;
use tokio::sync::mpsc::{Sender};
use std::{future::Future, sync::Arc};

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
