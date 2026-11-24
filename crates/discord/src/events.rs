use crate::models::{Channel, Guild, Id, Message, Ready, Relationship, User};

#[derive(Debug, Clone)]
pub enum Event {
    Ready(Box<Ready>),
    GuildCreate(Box<Guild>),
    GuildUpdate(Box<Guild>),
    GuildRemove { id: Id, unavailable: bool },
    ChannelCreate(Box<Channel>),
    ChannelUpdate(Box<Channel>),
    ChannelDelete(Box<Channel>),
    MessageCreate(Box<Message>),
    MessageUpdate(Box<Message>),
    MessageDelete { id: Id, channel_id: Id },
    TypingStart { channel_id: Id, user_id: Id },
    RelationshipAdd(Box<Relationship>),
    RelationshipRemove { id: Id },
    Relationships(Vec<Relationship>),
    PrivateChannels(Vec<Channel>),
    Users(Vec<User>),
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
