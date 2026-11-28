use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("websocket error: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid voice protocol message: {0}")]
    Protocol(String),
    #[error("transport crypto error: {0}")]
    Crypto(String),
    #[error("dave error: {0}")]
    Dave(String),
    #[error("voice connection closed: {0}")]
    Closed(String),
    #[error("voice connection timed out")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, Error>;
