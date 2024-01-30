use crate::traits::Actor;
use anyhow::Result;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::mpsc::{self, Sender};

static SHOULD_DIE: AtomicBool = AtomicBool::new(false);

pub struct Kill;

impl Kill {
    pub fn should_die() -> bool {
        self::SHOULD_DIE.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Actor for Kill {
    type Message = ();
    type StartParams = ();

    fn start(self, _params: Self::StartParams) -> Result<Arc<Sender<()>>> {
        let (sender, mut receiver) = mpsc::channel::<()>(1);
        Self::spawn(async move {
            if receiver.recv().await.is_some() {
                log::info!("Kill received");
                SHOULD_DIE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        })?;
        Ok(Arc::new(sender))
    }
}
