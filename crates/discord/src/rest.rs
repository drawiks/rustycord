use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tracing::{debug, warn};
use wreq::{Method, header::HeaderValue};
use wreq_util::Emulation;

use crate::{
    error::{Error, Result},
    fingerprint::Fingerprint,
    models::{Channel, Message, Relationship, User},
};

const BASE: &str = "https://discord.com/api/v9";
const MUTATION_GAP: Duration = Duration::from_millis(350);
const MAX_ATTEMPTS: u32 = 3;

#[derive(Clone)]
pub struct Http {
    inner: Arc<Inner>,
}

struct Inner {
    client: wreq::Client,
    token: Arc<str>,
    fp: Arc<Fingerprint>,
    limits: RateLimiter,
}

impl Http {
    pub fn new(token: String, fp: Arc<Fingerprint>) -> Result<Self> {
        let client = wreq::Client::builder()
            .emulation(Emulation::Chrome149)
            .cookie_store(true)
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                client,
                token: token.into(),
                fp,
                limits: RateLimiter::default(),
            }),
        })
    }

    pub fn token(&self) -> &str {
        &self.inner.token
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        route: &str,
        major: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value> {
        let key = format!("{route}:{major}");
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            self.inner
                .limits
                .acquire(&key, method != Method::GET)
                .await;

            let url = if query.is_empty() {
                format!("{BASE}{path}")
            } else {
                let encoded: Vec<String> = query
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}={}",
                            urlencoding::encode(key),
                            urlencoding::encode(value)
                        )
                    })
                    .collect();
                format!("{BASE}{path}?{}", encoded.join("&"))
            };

            let mut req = self
                .inner
                .client
                .request(method.clone(), url)
                .headers(self.headers());
            if let Some(body) = &body {
                req = req.json(body);
            }

            let resp = req.send().await?;
            let status = resp.status().as_u16();
            self.inner.limits.update(&key, resp.headers(), status == 429);

            if status == 401 {
                return Err(Error::Unauthorized);
            }
            if status == 429 || status >= 500 {
                if attempt >= MAX_ATTEMPTS {
                    return Err(Error::Api {
                        status,
                        body: resp.text().await.unwrap_or_default(),
                    });
                }
                let retry_after = resp
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|v| v["retry_after"].as_f64())
                    .unwrap_or(1.0)
                    .clamp(0.1, 30.0);
                warn!("rate limited on {key} ({status}), retrying in {retry_after}s");
                if status == 429 && attempt == 1 {
                    self.inner.limits.pause_global(retry_after);
                }
                tokio::time::sleep(Duration::from_secs_f64(retry_after)).await;
                continue;
            }

            let text = resp.text().await?;
            if !(200..300).contains(&status) {
                return Err(Error::Api { status, body: text });
            }
            if text.is_empty() {
                return Ok(Value::Null);
            }
            return Ok(serde_json::from_str(&text)?);
        }
    }

    fn headers(&self) -> wreq::header::HeaderMap {
        let mut headers = wreq::header::HeaderMap::new();
        let fp = &self.inner.fp;
        headers.insert(
            wreq::header::AUTHORIZATION,
            HeaderValue::from_str(&self.inner.token).expect("token header"),
        );
        headers.insert(
            "x-super-properties",
            HeaderValue::from_str(&fp.super_properties()).expect("super props"),
        );
        headers.insert(
            "x-discord-locale",
            HeaderValue::from_str(&fp.locale).expect("locale"),
        );
        headers.insert(
            "x-discord-timezone",
            HeaderValue::from_str(&fp.timezone).expect("timezone"),
        );
        headers.insert(
            "x-installation-id",
            HeaderValue::from_str(&fp.installation_id).expect("installation id"),
        );
        headers.insert(
            wreq::header::USER_AGENT,
            HeaderValue::from_static(fp.user_agent()),
        );
        headers.insert(
            wreq::header::ORIGIN,
            HeaderValue::from_static("https://discord.com"),
        );
        headers.insert(
            wreq::header::REFERER,
            HeaderValue::from_static("https://discord.com/channels/@me"),
        );
        headers.insert(wreq::header::ACCEPT, HeaderValue::from_static("*/*"));
        if let Some(fingerprint) = &fp.browser_fingerprint
            && let Ok(value) = HeaderValue::from_str(fingerprint)
        {
            headers.insert("x-fingerprint", value);
        }
        headers
    }

    pub async fn me(&self) -> Result<User> {
        let value = self.send(Method::GET, "/users/@me", "users_me", "", &[], None).await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn private_channels(&self) -> Result<Vec<Channel>> {
        let value = self
            .send(Method::GET, "/users/@me/channels", "users_channels", "", &[], None)
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn relationships(&self) -> Result<Vec<Relationship>> {
        let value = self
            .send(
                Method::GET,
                "/users/@me/relationships",
                "users_relationships",
                "",
                &[],
                None,
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn channel(&self, channel_id: &str) -> Result<Channel> {
        let value = self
            .send(
                Method::GET,
                &format!("/channels/{channel_id}"),
                "channel",
                channel_id,
                &[],
                None,
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn channel_messages(
        &self,
        channel_id: &str,
        before: Option<&str>,
        limit: u8,
    ) -> Result<Vec<Message>> {
        let mut query = vec![("limit", limit.clamp(1, 100).to_string())];
        if let Some(before) = before {
            query.push(("before", before.to_string()));
        }
        let value = self
            .send(
                Method::GET,
                &format!("/channels/{channel_id}/messages"),
                "channel_messages",
                channel_id,
                &query,
                None,
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<Message> {
        let mut body = json!({ "content": content, "tts": false });
        if let Some(message_id) = reply_to {
            body["message_reference"] = json!({
                "message_id": message_id,
                "channel_id": channel_id,
            });
        }
        let value = self
            .send(
                Method::POST,
                &format!("/channels/{channel_id}/messages"),
                "channel_send",
                channel_id,
                &[],
                Some(body),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<Message> {
        let value = self
            .send(
                Method::PATCH,
                &format!("/channels/{channel_id}/messages/{message_id}"),
                "channel_message",
                channel_id,
                &[],
                Some(json!({ "content": content })),
            )
            .await?;
        Ok(serde_json::from_value(value)?)
    }

    pub async fn delete_message(&self, channel_id: &str, message_id: &str) -> Result<()> {
        self.send(
            Method::DELETE,
            &format!("/channels/{channel_id}/messages/{message_id}"),
            "channel_message",
            channel_id,
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn ack(&self, channel_id: &str, message_id: &str, token: Option<&str>) -> Result<Value> {
        let body = match token {
            Some(token) => json!({ "token": token }),
            None => json!({ "manual": true, "mention_count": 0 }),
        };
        self.send(
            Method::POST,
            &format!("/channels/{channel_id}/messages/{message_id}/ack"),
            "channel_ack",
            channel_id,
            &[],
            Some(body),
        )
        .await
    }

    pub async fn typing(&self, channel_id: &str) -> Result<()> {
        self.send(
            Method::POST,
            &format!("/channels/{channel_id}/typing"),
            "channel_typing",
            channel_id,
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn add_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        let emoji = urlencoding::encode(emoji);
        self.send(
            Method::PUT,
            &format!("/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me"),
            "channel_reaction",
            channel_id,
            &[],
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn remove_reaction(&self, channel_id: &str, message_id: &str, emoji: &str) -> Result<()> {
        let emoji = urlencoding::encode(emoji);
        self.send(
            Method::DELETE,
            &format!("/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me"),
            "channel_reaction",
            channel_id,
            &[],
            None,
        )
        .await?;
        Ok(())
    }
}

#[derive(Default)]
struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
    global_until: Mutex<Option<Instant>>,
    last_mutation: Mutex<Option<Instant>>,
}

struct Bucket {
    remaining: u32,
    reset: Instant,
}

impl RateLimiter {
    async fn acquire(&self, key: &str, mutating: bool) {
        loop {
            let global_wait = {
                let guard = self.global_until.lock().unwrap();
                guard.and_then(|until| until.checked_duration_since(Instant::now()))
            };
            if let Some(wait) = global_wait {
                tokio::time::sleep(wait).await;
                continue;
            }

            let bucket_wait = {
                let guard = self.buckets.lock().unwrap();
                guard.get(key).and_then(|bucket| {
                    if bucket.remaining == 0 {
                        bucket.reset.checked_duration_since(Instant::now())
                    } else {
                        None
                    }
                })
            };
            if let Some(wait) = bucket_wait {
                tokio::time::sleep(wait + Duration::from_millis(50)).await;
                continue;
            }

            if mutating {
                loop {
                    let wait = {
                        let mut guard = self.last_mutation.lock().unwrap();
                        match *guard {
                            Some(last) if last.elapsed() < MUTATION_GAP => {
                                Some(MUTATION_GAP - last.elapsed())
                            }
                            _ => {
                                *guard = Some(Instant::now());
                                None
                            }
                        }
                    };
                    match wait {
                        Some(wait) => tokio::time::sleep(wait).await,
                        None => break,
                    }
                }
            }

            if let Some(bucket) = self.buckets.lock().unwrap().get_mut(key) {
                bucket.remaining = bucket.remaining.saturating_sub(1);
            }
            return;
        }
    }

    fn update(&self, key: &str, headers: &wreq::header::HeaderMap, limited: bool) {
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok());
        let reset_after = headers
            .get("x-ratelimit-reset-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok());

        if let (Some(remaining), Some(reset_after)) = (remaining, reset_after) {
            let reset = Instant::now() + Duration::from_secs_f64(reset_after.max(0.0));
            self.buckets
                .lock()
                .unwrap()
                .insert(key.to_string(), Bucket { remaining, reset });
        }

        if limited {
            let global = headers
                .get("x-ratelimit-global")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            debug!("429 on {key}, global={global}");
        }
    }

    fn pause_global(&self, seconds: f64) {
        let until = Instant::now() + Duration::from_secs_f64(seconds.clamp(0.0, 60.0));
        *self.global_until.lock().unwrap() = Some(until);
    }
}
