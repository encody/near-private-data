use std::sync::Arc;
use std::io::Write;
use anyhow::Result;
use tokio::sync::mpsc::{self, Sender};
use crate::traits::Actor;

pub struct Draw;
pub type Message = String;

impl Actor for Draw {
    type Message = Message;

    type StartParams = ();

    fn start(
        self,
        _params: Self::StartParams,
    ) -> Result<std::sync::Arc<Sender<Self::Message>>> {
        let (sender, mut receiver) = mpsc::channel::<Self::Message>(4);

        Self::spawn(async move {
            let stdout = console::Term::stdout();

            loop {
                if let Some(msg) = receiver.recv().await {
                    writeln!(&stdout, "{}", msg).unwrap();
                }
            }
        })?;
        Ok(Arc::new(sender))
    }
}
