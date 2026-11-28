use serde::{Deserialize, Deserializer, de::Error as _};

pub type Id = String;

pub fn snowflake(id: &str) -> u64 {
    id.parse().unwrap_or(0)
}

fn string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) => text,
        serde_json::Value::Number(number) => number.to_string(),
        _ => String::new(),
    })
}

fn opt_string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(text)) => Some(text),
            Some(serde_json::Value::Number(number)) => Some(number.to_string()),
            _ => None,
        },
    )
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)
        .map_err(D::Error::custom)?
        .unwrap_or_default())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct User {
    pub id: Id,
    pub username: String,
    #[serde(default)]
    pub global_name: Option<String>,
    #[serde(default, deserialize_with = "string_or_number")]
    pub discriminator: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub bot: bool,
    #[serde(default)]
    pub clan: Option<Clan>,
    #[serde(default)]
    pub primary_guild: Option<Clan>,
    #[serde(default)]
    pub avatar_decoration_data: Option<AvatarDecoration>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Clan {
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub identity_guild_id: Option<Id>,
    #[serde(default)]
    pub identity_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AvatarDecoration {
    #[serde(default)]
    pub asset: String,
    #[serde(default)]
    pub sku_id: String,
}

impl User {
    pub fn display_name(&self) -> &str {
        self.global_name.as_deref().unwrap_or(&self.username)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Role {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub color: u32,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub permissions: String,
    #[serde(default)]
    pub hoist: bool,
    #[serde(default)]
    pub managed: bool,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub unicode_emoji: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionOverwrite {
    pub id: Id,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub allow: String,
    #[serde(default)]
    pub deny: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Call {
    #[serde(default)]
    pub channel_id: Id,
    #[serde(default)]
    pub message_id: Option<Id>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub ringing: Vec<Id>,
    #[serde(default)]
    pub ended_timestamp: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VoiceState {
    #[serde(default)]
    pub user_id: Id,
    #[serde(default)]
    pub channel_id: Option<Id>,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub deaf: bool,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub self_deaf: bool,
    #[serde(default)]
    pub self_mute: bool,
    #[serde(default)]
    pub self_stream: bool,
    #[serde(default)]
    pub self_video: bool,
    #[serde(default)]
    pub suppress: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Member {
    #[serde(default)]
    pub user: User,
    #[serde(default)]
    pub user_id: Option<Id>,
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default)]
    pub roles: Vec<Id>,
    #[serde(default)]
    pub joined_at: Option<String>,
}

impl Member {
    pub fn id(&self) -> &str {
        if !self.user.id.is_empty() {
            &self.user.id
        } else {
            self.user_id.as_deref().unwrap_or_default()
        }
    }

    pub fn display_name(&self) -> &str {
        self.nick
            .as_deref()
            .unwrap_or_else(|| self.user.display_name())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Guild {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub owner_id: Option<Id>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub member_count: u32,
    #[serde(default, deserialize_with = "null_default")]
    pub members: Vec<Member>,
    #[serde(default, deserialize_with = "null_default")]
    pub presences: Vec<Presence>,
    #[serde(default, deserialize_with = "null_default")]
    pub voice_states: Vec<VoiceState>,
    #[serde(default)]
    pub unavailable: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Channel {
    pub id: Id,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub guild_id: Option<Id>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub parent_id: Option<Id>,
    #[serde(default, deserialize_with = "opt_string_or_number")]
    pub last_message_id: Option<Id>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub recipients: Vec<User>,
    #[serde(default)]
    pub recipient_ids: Vec<Id>,
    #[serde(default, deserialize_with = "null_default")]
    pub permission_overwrites: Vec<PermissionOverwrite>,
    #[serde(default)]
    pub user_limit: u32,
}

impl Channel {
    pub const TEXT: u8 = 0;
    pub const DM: u8 = 1;
    pub const VOICE: u8 = 2;
    pub const GROUP_DM: u8 = 3;
    pub const CATEGORY: u8 = 4;
    pub const ANNOUNCEMENT: u8 = 5;

    pub fn is_text(&self) -> bool {
        matches!(self.kind, 0 | 1 | 3 | 5 | 10 | 11 | 12 | 13 | 14 | 15 | 16)
            && self.kind != Self::CATEGORY
    }

    pub fn is_voice(&self) -> bool {
        self.kind == Self::VOICE
    }

    pub fn is_category(&self) -> bool {
        self.kind == Self::CATEGORY
    }

    pub fn display_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        let names: Vec<&str> = self.recipients.iter().map(|u| u.display_name()).collect();
        if names.is_empty() {
            self.id.clone()
        } else {
            names.join(", ")
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Attachment {
    pub id: Id,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbedImage {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbedAuthor {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbedField {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub inline: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbedFooter {
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Embed {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub color: Option<u32>,
    #[serde(default)]
    pub author: Option<EmbedAuthor>,
    #[serde(default)]
    pub image: Option<EmbedImage>,
    #[serde(default)]
    pub thumbnail: Option<EmbedImage>,
    #[serde(default)]
    pub fields: Vec<EmbedField>,
    #[serde(default)]
    pub footer: Option<EmbedFooter>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageReference {
    #[serde(default)]
    pub message_id: Option<Id>,
    #[serde(default)]
    pub channel_id: Option<Id>,
    #[serde(default)]
    pub guild_id: Option<Id>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Reaction {
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub me: bool,
    #[serde(default)]
    pub emoji: Emoji,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Emoji {
    #[serde(default)]
    pub id: Option<Id>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Emoji {
    pub fn text(&self) -> String {
        match (&self.id, &self.name) {
            (Some(id), Some(name)) => format!("{name}:{id}"),
            (None, Some(name)) => name.clone(),
            _ => "?".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    #[serde(default)]
    pub guild_id: Option<Id>,
    #[serde(default)]
    pub author: User,
    #[serde(default, deserialize_with = "null_default")]
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub edited_timestamp: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub attachments: Vec<Attachment>,
    #[serde(default, deserialize_with = "null_default")]
    pub embeds: Vec<Embed>,
    #[serde(default, deserialize_with = "null_default")]
    pub mentions: Vec<User>,
    #[serde(default)]
    pub message_reference: Option<MessageReference>,
    #[serde(default)]
    pub referenced_message: Option<Box<Message>>,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default, deserialize_with = "null_default")]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Relationship {
    pub id: Id,
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub user: User,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReadState {
    pub id: Id,
    #[serde(default, deserialize_with = "opt_string_or_number")]
    pub last_message_id: Option<Id>,
    #[serde(default)]
    pub mention_count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReadStateContainer {
    #[serde(default)]
    pub entries: Vec<ReadState>,
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Ready {
    pub session_id: String,
    #[serde(default)]
    pub resume_gateway_url: String,
    #[serde(default)]
    pub user: User,
    #[serde(default)]
    pub guilds: Vec<Guild>,
    #[serde(default)]
    pub private_channels: Vec<Channel>,
    #[serde(default, deserialize_with = "null_default")]
    pub users: Vec<User>,
    #[serde(default, deserialize_with = "null_default")]
    pub presences: Vec<Presence>,
    #[serde(default, deserialize_with = "null_default")]
    pub merged_members: Vec<Vec<Member>>,
    #[serde(default)]
    pub merged_presences: Option<MergedPresences>,
    #[serde(default)]
    pub read_state: Option<ReadStateContainer>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SupplementalGuild {
    #[serde(default)]
    pub id: Id,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReadySupplemental {
    #[serde(default, deserialize_with = "null_default")]
    pub guilds: Vec<SupplementalGuild>,
    #[serde(default, deserialize_with = "null_default")]
    pub lazy_private_channels: Vec<Channel>,
    #[serde(default, deserialize_with = "null_default")]
    pub users: Vec<User>,
    #[serde(default, deserialize_with = "null_default")]
    pub merged_members: Vec<Vec<Member>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActivityAssets {
    #[serde(default)]
    pub large_image: Option<String>,
    #[serde(default)]
    pub small_image: Option<String>,
    #[serde(default)]
    pub large_text: Option<String>,
    #[serde(default)]
    pub small_text: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Activity {
    #[serde(rename = "type", default)]
    pub kind: u8,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub emoji: Option<Emoji>,
    #[serde(default)]
    pub assets: Option<ActivityAssets>,
    #[serde(default)]
    pub application_id: Option<Id>,
}

impl Activity {
    pub const CUSTOM: u8 = 4;

    pub fn custom_status(&self) -> Option<&str> {
        if self.kind == Self::CUSTOM {
            self.state.as_deref()
        } else {
            None
        }
    }

    pub fn label(&self) -> String {
        match self.custom_status() {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Presence {
    #[serde(default)]
    pub user: User,
    #[serde(default)]
    pub user_id: Option<Id>,
    #[serde(default)]
    pub status: String,
    #[serde(default, deserialize_with = "null_default")]
    pub activities: Vec<Activity>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MergedPresences {
    #[serde(default, deserialize_with = "null_default")]
    pub friends: Vec<Presence>,
    #[serde(default, deserialize_with = "null_default")]
    pub guilds: Vec<Vec<Presence>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TypingStart {
    pub channel_id: Id,
    #[serde(default)]
    pub user_id: Option<Id>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_user_with_numeric_and_string_discriminator() {
        let numeric: User =
            serde_json::from_str(r#"{"id":"1","username":"a","discriminator":0}"#).unwrap();
        assert_eq!(numeric.discriminator, "0");

        let text: User =
            serde_json::from_str(r#"{"id":"1","username":"a","discriminator":"1234"}"#).unwrap();
        assert_eq!(text.discriminator, "1234");

        let missing: User = serde_json::from_str(r#"{"id":"1","username":"a"}"#).unwrap();
        assert_eq!(missing.discriminator, "");
    }
}
