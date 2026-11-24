use std::{collections::HashMap, time::Instant};

use crate::{
    events::Event,
    models::{Channel, Guild, Id, Message, ReadState, Relationship, User, snowflake},
};

const MAX_MESSAGES_PER_CHANNEL: usize = 200;

#[derive(Default)]
pub struct State {
    pub me: Option<User>,
    pub guilds: Vec<Guild>,
    pub channels: Vec<Channel>,
    pub relationships: Vec<Relationship>,
    pub read_states: HashMap<Id, ReadState>,
    pub messages: HashMap<Id, Vec<Message>>,
    pub typing: HashMap<Id, (Id, Instant)>,
}

impl State {
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::Ready(ready) => {
                self.me = Some(ready.user.clone());
                self.guilds = ready.guilds.clone();
                self.channels = ready.private_channels.clone();
                self.messages.clear();
                self.read_states.clear();
                if let Some(read_state) = &ready.read_state {
                    for entry in &read_state.entries {
                        self.read_states.insert(entry.id.clone(), entry.clone());
                    }
                }
            }
            Event::GuildCreate(guild) | Event::GuildUpdate(guild) => {
                self.upsert_guild((**guild).clone());
            }
            Event::GuildRemove { id, unavailable } => {
                if !unavailable {
                    self.guilds.retain(|g| &g.id != id);
                }
            }
            Event::ChannelCreate(channel) | Event::ChannelUpdate(channel) => {
                self.upsert_channel((**channel).clone());
            }
            Event::ChannelDelete(channel) => {
                self.remove_channel(channel);
            }
            Event::MessageCreate(message) => {
                self.insert_message((**message).clone());
            }
            Event::MessageUpdate(update) => {
                self.merge_message(update);
            }
            Event::MessageDelete { id, channel_id } => {
                if let Some(list) = self.messages.get_mut(channel_id) {
                    list.retain(|m| &m.id != id);
                }
            }
            Event::TypingStart { channel_id, user_id } => {
                self.typing
                    .insert(channel_id.clone(), (user_id.clone(), Instant::now()));
            }
            Event::RelationshipAdd(relationship) => {
                if let Some(existing) = self
                    .relationships
                    .iter_mut()
                    .find(|r| r.id == relationship.id)
                {
                    *existing = (**relationship).clone();
                } else {
                    self.relationships.push((**relationship).clone());
                }
            }
            Event::RelationshipRemove { id } => {
                self.relationships.retain(|r| &r.id != id);
            }
            Event::UserUpdate(user) => {
                if self.me.as_ref().is_some_and(|me| me.id == user.id) {
                    self.me = Some((**user).clone());
                }
            }
            Event::Connection(_) => {}
        }
    }

    pub fn guild(&self, id: &str) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == id)
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels
            .iter()
            .find(|c| c.id == id)
            .or_else(|| self.guilds.iter().flat_map(|g| &g.channels).find(|c| c.id == id))
    }

    pub fn text_channels(&self, guild_id: &str) -> Vec<&Channel> {
        let Some(guild) = self.guild(guild_id) else {
            return Vec::new();
        };
        let mut channels: Vec<&Channel> = guild.channels.iter().filter(|c| c.is_text()).collect();
        channels.sort_by_key(|c| c.position);
        channels
    }

    pub fn messages(&self, channel_id: &str) -> &[Message] {
        self.messages
            .get(channel_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn insert_history(&mut self, channel_id: &str, mut messages: Vec<Message>) {
        let list = self.messages.entry(channel_id.to_string()).or_default();
        messages.retain(|m| !list.iter().any(|existing| existing.id == m.id));
        messages.extend(std::mem::take(list));
        messages.sort_by_key(|m| snowflake(&m.id));
        *list = messages;
    }

    fn upsert_guild(&mut self, guild: Guild) {
        if let Some(existing) = self.guilds.iter_mut().find(|g| g.id == guild.id) {
            if !guild.channels.is_empty() {
                existing.channels = guild.channels;
            }
            existing.name = guild.name;
            existing.icon = guild.icon;
            existing.roles = guild.roles;
            existing.member_count = guild.member_count;
            existing.unavailable = guild.unavailable;
        } else {
            self.guilds.push(guild);
        }
    }

    fn upsert_channel(&mut self, channel: Channel) {
        if let Some(guild_id) = &channel.guild_id
            && let Some(guild) = self.guilds.iter_mut().find(|g| &g.id == guild_id)
        {
            if let Some(existing) = guild.channels.iter_mut().find(|c| c.id == channel.id) {
                *existing = channel;
            } else {
                guild.channels.push(channel);
            }
            return;
        }
        if let Some(existing) = self.channels.iter_mut().find(|c| c.id == channel.id) {
            *existing = channel;
        } else {
            self.channels.push(channel);
        }
    }

    fn remove_channel(&mut self, channel: &Channel) {
        for guild in &mut self.guilds {
            guild.channels.retain(|c| c.id != channel.id);
        }
        self.channels.retain(|c| c.id != channel.id);
    }

    fn insert_message(&mut self, message: Message) {
        let list = self.messages.entry(message.channel_id.clone()).or_default();
        if list.iter().any(|m| m.id == message.id) {
            return;
        }
        let position = list.partition_point(|m| snowflake(&m.id) <= snowflake(&message.id));
        list.insert(position, message);
        if list.len() > MAX_MESSAGES_PER_CHANNEL {
            list.drain(..list.len() - MAX_MESSAGES_PER_CHANNEL);
        }
    }

    fn merge_message(&mut self, update: &Message) {
        let Some(list) = self.messages.get_mut(&update.channel_id) else {
            return;
        };
        let Some(message) = list.iter_mut().find(|m| m.id == update.id) else {
            return;
        };
        if !update.content.is_empty() {
            message.content = update.content.clone();
        }
        if update.edited_timestamp.is_some() {
            message.edited_timestamp = update.edited_timestamp.clone();
        }
        if !update.embeds.is_empty() {
            message.embeds = update.embeds.clone();
        }
        if !update.attachments.is_empty() {
            message.attachments = update.attachments.clone();
        }
        if !update.reactions.is_empty() {
            message.reactions = update.reactions.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Message;

    fn message(id: &str, channel: &str, content: &str) -> Message {
        Message {
            id: id.to_string(),
            channel_id: channel.to_string(),
            content: content.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn messages_stay_sorted_and_trimmed() {
        let mut state = State::default();
        state.apply(&Event::MessageCreate(Box::new(message("30", "1", "c"))));
        state.apply(&Event::MessageCreate(Box::new(message("10", "1", "a"))));
        state.apply(&Event::MessageCreate(Box::new(message("20", "1", "b"))));
        state.apply(&Event::MessageCreate(Box::new(message("20", "1", "dup"))));

        let ids: Vec<&str> = state.messages("1").iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["10", "20", "30"]);
    }

    #[test]
    fn message_update_merges_content() {
        let mut state = State::default();
        state.apply(&Event::MessageCreate(Box::new(message("10", "1", "old"))));
        state.apply(&Event::MessageUpdate(Box::new(message("10", "1", "new"))));
        assert_eq!(state.messages("1")[0].content, "new");
    }

    #[test]
    fn history_is_merged_without_duplicates() {
        let mut state = State::default();
        state.apply(&Event::MessageCreate(Box::new(message("30", "1", "c"))));
        state.insert_history("1", vec![message("10", "1", "a"), message("30", "1", "c")]);
        let ids: Vec<&str> = state.messages("1").iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["10", "30"]);
    }
}
