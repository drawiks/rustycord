use crate::models::{
    Channel, Emoji, Guild, Id, Member, Message, Presence, Ready, Relationship, User, VoiceState,
};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Ready(Box<Ready>),
    GuildCreate(Box<Guild>),
    GuildUpdate(Box<Guild>),
    GuildRemove {
        id: Id,
        unavailable: bool,
    },
    ChannelCreate(Box<Channel>),
    ChannelUpdate(Box<Channel>),
    ChannelDelete(Box<Channel>),
    MessageCreate(Box<Message>),
    MessageUpdate(Box<Message>),
    MessageDelete {
        id: Id,
        channel_id: Id,
    },
    ReactionAdd {
        message_id: Id,
        channel_id: Id,
        user_id: Id,
        emoji: Emoji,
    },
    ReactionRemove {
        message_id: Id,
        channel_id: Id,
        user_id: Id,
        emoji: Emoji,
    },
    ReactionRemoveAll {
        message_id: Id,
        channel_id: Id,
    },
    ReactionRemoveEmoji {
        message_id: Id,
        channel_id: Id,
        emoji: Emoji,
    },
    TypingStart {
        channel_id: Id,
        user_id: Id,
    },
    RelationshipAdd(Box<Relationship>),
    RelationshipRemove {
        id: Id,
    },
    Relationships(Vec<Relationship>),
    PrivateChannels(Vec<Channel>),
    Users(Vec<User>),
    PresenceUpdate(Box<Presence>),
    MemberUpdate {
        guild_id: Id,
        member: Member,
    },
    MemberRemove {
        guild_id: Id,
        user_id: Id,
    },
    MemberChunk {
        guild_id: Id,
        members: Vec<Member>,
    },
    VoiceStateUpdate {
        guild_id: Id,
        state: Box<VoiceState>,
    },
    UserUpdate(Box<User>),
    Connection(ConnectionState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Resumed,
    Reconnecting { attempt: u32 },
    Disconnected { reason: String },
}
