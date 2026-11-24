use std::result;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http: {0}")]
    Http(#[from] wreq::Error),
    #[error("websocket: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unauthorized")]
    Unauthorized,
    #[error("api {status}: {body}")]
    Api { status: u16, body: String },
    #[error("gateway closed with code {0}")]
    GatewayClosed(u16),
    #[error("gateway connect timeout")]
    GatewayTimeout,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = result::Result<T, Error>;
