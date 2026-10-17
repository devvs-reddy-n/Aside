use serde::Serialize;
use tokio::sync::watch;

/// Capture being open does not prove that the speech service is accepting it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SpeechConnection {
    Connecting,
    Ready,
    Recovering { message: String },
    Failed { message: String },
    Stopped,
}

pub async fn wait_ready(mut status: watch::Receiver<SpeechConnection>) -> Result<(), String> {
    tokio::time::timeout(std::time::Duration::from_secs(35), async {
        loop {
            let current = status.borrow_and_update().clone();
            match current {
                SpeechConnection::Ready => return Ok(()),
                SpeechConnection::Failed { message } => return Err(message),
                SpeechConnection::Stopped => return Err("Speech recognition was stopped.".into()),
                _ => {}
            }
            status.changed().await.map_err(|_| "Speech connection ended before it was ready.".to_owned())?;
        }
    }).await.map_err(|_| "Speechmatics is still unavailable. Check your connection and try starting again.".to_owned())?
}
