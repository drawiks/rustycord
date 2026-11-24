use base64::Engine;
use rand::Rng;
use serde_json::{Value, json};
use tracing::{debug, warn};
use wreq_util::Emulation;

const FALLBACK_BUILD: u32 = 580_004;
const CHROME_VERSION: &str = "149.0.0.0";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct Fingerprint {
    pub build_number: u32,
    pub locale: String,
    pub timezone: String,
    pub installation_id: String,
    pub client_launch_id: String,
    pub launch_signature: String,
    pub browser_fingerprint: Option<String>,
}

impl Fingerprint {
    pub async fn fetch() -> Self {
        let client = wreq::Client::builder()
            .emulation(Emulation::Chrome149)
            .timeout(std::time::Duration::from_secs(15))
            .build();
        let client = match client {
            Ok(c) => c,
            Err(e) => {
                warn!("fingerprint client init failed: {e}");
                return Self::fallback();
            }
        };

        let build_number = match fetch_build_number(&client).await {
            Some(b) => b,
            None => {
                warn!("could not scrape client build number, using fallback {FALLBACK_BUILD}");
                FALLBACK_BUILD
            }
        };

        let browser_fingerprint = match client
            .get("https://discord.com/api/v9/experiments?with_guild_experiments=true")
            .send()
            .await
        {
            Ok(resp) => resp
                .headers()
                .get("x-fingerprint")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
            Err(e) => {
                debug!("experiments request failed: {e}");
                None
            }
        };

        let installation_id = match client
            .get("https://discord.com/api/v9/apex/experiments?surface=2")
            .send()
            .await
        {
            Ok(resp) => resp
                .json::<Value>()
                .await
                .ok()
                .and_then(|v| {
                    find_key(&v, "installation_id")
                        .and_then(|id| id.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(random_id),
            Err(_) => random_id(),
        };

        debug!("fingerprint build={build_number} installation={installation_id}");

        Self {
            build_number,
            locale: "en-US".to_string(),
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string()),
            installation_id,
            client_launch_id: random_id(),
            launch_signature: random_id(),
            browser_fingerprint,
        }
    }

    pub fn fallback() -> Self {
        Self {
            build_number: FALLBACK_BUILD,
            locale: "en-US".to_string(),
            timezone: iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string()),
            installation_id: random_id(),
            client_launch_id: random_id(),
            launch_signature: random_id(),
            browser_fingerprint: None,
        }
    }

    pub fn user_agent(&self) -> &'static str {
        UA
    }

    fn properties(&self) -> Value {
        json!({
            "os": "Windows",
            "browser": "Chrome",
            "device": "",
            "system_locale": self.locale,
            "browser_user_agent": UA,
            "browser_version": CHROME_VERSION,
            "os_version": "10",
            "referrer": "",
            "referring_domain": "",
            "referrer_current": "",
            "referring_domain_current": "",
            "release_channel": "stable",
            "client_build_number": self.build_number,
            "client_event_source": null,
            "installation_id": self.installation_id,
        })
    }

    pub fn super_properties(&self) -> String {
        let mut props = self.properties();
        props["client_launch_id"] = json!(self.client_launch_id);
        props["launch_signature"] = json!(self.launch_signature);
        props["client_app_state"] = json!("focused");
        base64::engine::general_purpose::STANDARD.encode(props.to_string())
    }

    pub fn identify_properties(&self) -> Value {
        self.properties()
    }
}

fn random_id() -> String {
    let mut rng = rand::rng();
    (0..32)
        .map(|_| format!("{:x}", rng.random_range(0..16u8)))
        .collect()
}

fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|v| find_key(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

async fn fetch_build_number(client: &wreq::Client) -> Option<u32> {
    let html = fetch_text(client, "https://discord.com/app").await?;
    if let Some(build) = extract_build(&html) {
        return Some(build);
    }
    let asset = regex::Regex::new(r#"/assets/sentry[^"]+\.js"#)
        .ok()?
        .find(&html)?
        .as_str()
        .to_string();
    let js = fetch_text(client, &format!("https://discord.com{asset}")).await?;
    extract_build(&js)
}

fn extract_build(text: &str) -> Option<u32> {
    let re = regex::Regex::new(r#"buildNumber\D{0,6}(\d{5,9})"#).ok()?;
    re.captures(text)?.get(1)?.as_str().parse().ok()
}

async fn fetch_text(client: &wreq::Client, url: &str) -> Option<String> {
    let resp = client
        .get(url)
        .header("accept", "text/html,application/xhtml+xml,application/javascript")
        .send()
        .await
        .ok()?;
    resp.text().await.ok()
}

#[cfg(test)]
mod tests {
    use super::extract_build;
    use base64::Engine;

    #[test]
    fn extracts_build_number() {
        assert_eq!(extract_build(r#"e.buildNumber","581234","#), Some(581234));
        assert_eq!(extract_build(r#"{"buildNumber": 581234}"#), Some(581234));
        assert_eq!(extract_build("nothing here"), None);
    }

    #[test]
    fn super_properties_are_base64_json() {
        let fp = super::Fingerprint::fallback();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(fp.super_properties())
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(value["browser"], "Chrome");
        assert_eq!(value["client_build_number"], fp.build_number);
    }
}
