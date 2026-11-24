mod error;
mod events;
mod fingerprint;
mod gateway;
mod models;
mod rest;
mod state;

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

pub use error::{Error, Result};
pub use events::{ConnectionState, Event};
pub use gateway::GatewayHandle;
pub use models::*;
pub use state::State;

fn install_crypto_provider() {
    use std::sync::Once;

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[derive(Clone)]
pub struct Discord {
    http: rest::Http,
    fingerprint: Arc<fingerprint::Fingerprint>,
}

impl Discord {
    pub async fn login(token: impl Into<String>) -> Result<(Self, User)> {
        install_crypto_provider();
        let fingerprint = Arc::new(fingerprint::Fingerprint::fetch().await);
        let http = rest::Http::new(token.into(), fingerprint.clone())?;
        let me = http.me().await?;
        Ok((Self { http, fingerprint }, me))
    }

    pub fn spawn_gateway(&self, events: mpsc::Sender<Event>) -> GatewayHandle {
        gateway::spawn(
            self.http.token().to_string(),
            self.fingerprint.clone(),
            events,
        )
    }

    pub async fn history(
        &self,
        channel_id: &str,
        before: Option<&str>,
        limit: u8,
    ) -> Result<Vec<Message>> {
        self.http.channel_messages(channel_id, before, limit).await
    }

    pub async fn send(
        &self,
        channel_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<Message> {
        self.http.send_message(channel_id, content, reply_to).await
    }

    pub async fn edit(&self, channel_id: &str, message_id: &str, content: &str) -> Result<Message> {
        self.http
            .edit_message(channel_id, message_id, content)
            .await
    }

    pub async fn delete(&self, channel_id: &str, message_id: &str) -> Result<()> {
        self.http.delete_message(channel_id, message_id).await
    }

    pub async fn ack(
        &self,
        channel_id: &str,
        message_id: &str,
        token: Option<&str>,
    ) -> Result<Value> {
        self.http.ack(channel_id, message_id, token).await
    }

    pub async fn send_file(
        &self,
        channel_id: &str,
        content: &str,
        path: &std::path::Path,
    ) -> Result<Message> {
        self.http.send_file(channel_id, content, path).await
    }

    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        self.http.download(url).await
    }

    pub async fn typing(&self, channel_id: &str) -> Result<()> {
        self.http.typing(channel_id).await
    }

    pub async fn react(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        self.http.add_reaction(channel_id, message_id, emoji).await
    }

    pub async fn unreact(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        self.http
            .remove_reaction(channel_id, message_id, emoji)
            .await
    }

    pub async fn channel(&self, channel_id: &str) -> Result<Channel> {
        self.http.channel(channel_id).await
    }

    pub async fn relationships(&self) -> Result<Vec<Relationship>> {
        self.http.relationships().await
    }

    pub async fn private_channels(&self) -> Result<Vec<Channel>> {
        self.http.private_channels().await
    }
}
