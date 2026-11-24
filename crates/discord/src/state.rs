use std::{collections::HashMap, time::Instant};

use crate::{
    events::Event,
    models::{
        Channel, Guild, Id, Member, Message, Reaction, ReadState, Relationship, Role, User,
        snowflake,
    },
};

const MAX_MESSAGES_PER_CHANNEL: usize = 200;

#[derive(Default)]
pub struct State {
    pub me: Option<User>,
    pub users: HashMap<Id, User>,
    pub guilds: Vec<Guild>,
    pub channels: Vec<Channel>,
    pub relationships: Vec<Relationship>,
    pub read_states: HashMap<Id, ReadState>,
    pub messages: HashMap<Id, Vec<Message>>,
    pub typing: HashMap<Id, (Id, Instant)>,
    pub presences: HashMap<Id, String>,
}

impl State {
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::Ready(ready) => {
                self.me = Some(ready.user.clone());
                for user in &ready.users {
                    self.remember_user(user);
                }
                self.remember_user(&ready.user);
                for presence in &ready.presences {
                    self.apply_presence(presence);
                }
                if let Some(merged) = &ready.merged_presences {
                    for presence in &merged.friends {
                        self.apply_presence(presence);
                    }
                    for guild in &merged.guilds {
                        for presence in guild {
                            self.apply_presence(presence);
                        }
                    }
                }
                if let Some(me) = &self.me
                    && !self.presences.contains_key(&me.id)
                {
                    self.presences.insert(me.id.clone(), "online".to_string());
                }
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
            Event::GuildCreate(guild) => {
                let mut guild = (**guild).clone();
                let resolved: Vec<Member> = guild
                    .members
                    .iter()
                    .map(|member| self.resolve_member(member.clone()))
                    .collect();
                guild.members = resolved;
                for member in &guild.members {
                    self.remember_user(&member.user);
                }
                for presence in &guild.presences {
                    self.apply_presence(presence);
                }
                self.upsert_guild(guild);
            }
            Event::GuildUpdate(guild) => {
                self.upsert_guild((**guild).clone());
            }
            Event::MemberUpdate { guild_id, member } => {
                self.upsert_member(guild_id, member.clone());
            }
            Event::MemberChunk { guild_id, members } => {
                for member in members {
                    self.upsert_member(guild_id, member.clone());
                }
            }
            Event::MemberRemove { guild_id, user_id } => {
                if let Some(guild) = self.guilds.iter_mut().find(|guild| &guild.id == guild_id) {
                    guild.members.retain(|member| member.id() != user_id);
                }
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
                self.remember_user(&message.author);
                if let Some(channel) = self.channel_mut(&message.channel_id) {
                    channel.last_message_id = Some(message.id.clone());
                }
                self.insert_message((**message).clone());
            }
            Event::MessageUpdate(update) => {
                self.remember_user(&update.author);
                self.merge_message(update);
            }
            Event::MessageDelete { id, channel_id } => {
                if let Some(list) = self.messages.get_mut(channel_id) {
                    list.retain(|m| &m.id != id);
                }
            }
            Event::ReactionAdd {
                message_id,
                channel_id,
                user_id,
                emoji,
            } => {
                let mine = self.me.as_ref().is_some_and(|me| &me.id == user_id);
                let key = emoji.text();
                if let Some(message) = self.message_mut(channel_id, message_id) {
                    if let Some(reaction) = message
                        .reactions
                        .iter_mut()
                        .find(|reaction| reaction.emoji.text() == key)
                    {
                        reaction.count += 1;
                        if mine {
                            reaction.me = true;
                        }
                    } else {
                        message.reactions.push(Reaction {
                            count: 1,
                            me: mine,
                            emoji: emoji.clone(),
                        });
                    }
                }
            }
            Event::ReactionRemove {
                message_id,
                channel_id,
                user_id,
                emoji,
            } => {
                let mine = self.me.as_ref().is_some_and(|me| &me.id == user_id);
                let key = emoji.text();
                if let Some(message) = self.message_mut(channel_id, message_id)
                    && let Some(index) = message
                        .reactions
                        .iter()
                        .position(|reaction| reaction.emoji.text() == key)
                {
                    let reaction = &mut message.reactions[index];
                    reaction.count = reaction.count.saturating_sub(1);
                    if mine {
                        reaction.me = false;
                    }
                    if reaction.count == 0 {
                        message.reactions.remove(index);
                    }
                }
            }
            Event::ReactionRemoveAll {
                message_id,
                channel_id,
            } => {
                if let Some(message) = self.message_mut(channel_id, message_id) {
                    message.reactions.clear();
                }
            }
            Event::ReactionRemoveEmoji {
                message_id,
                channel_id,
                emoji,
            } => {
                let key = emoji.text();
                if let Some(message) = self.message_mut(channel_id, message_id) {
                    message
                        .reactions
                        .retain(|reaction| reaction.emoji.text() != key);
                }
            }
            Event::TypingStart {
                channel_id,
                user_id,
            } => {
                self.typing
                    .insert(channel_id.clone(), (user_id.clone(), Instant::now()));
            }
            Event::RelationshipAdd(relationship) => {
                self.remember_user(&relationship.user);
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
            Event::Relationships(relationships) => {
                for relationship in relationships {
                    self.remember_user(&relationship.user);
                }
                self.relationships = relationships.clone();
            }
            Event::PrivateChannels(channels) => {
                for channel in channels {
                    for user in &channel.recipients {
                        self.remember_user(user);
                    }
                    self.upsert_channel(channel.clone());
                }
            }
            Event::Users(users) => {
                for user in users {
                    self.remember_user(user);
                }
            }
            Event::PresenceUpdate(presence) => {
                self.apply_presence(presence);
            }
            Event::UserUpdate(user) => {
                self.remember_user(user);
                if self.me.as_ref().is_some_and(|me| me.id == user.id) {
                    self.me = Some((**user).clone());
                }
            }
            Event::Connection(_) => {}
        }
    }

    pub fn user(&self, id: &str) -> Option<&User> {
        self.users.get(id)
    }

    pub fn presence(&self, id: &str) -> &str {
        self.presences
            .get(id)
            .map(String::as_str)
            .unwrap_or("offline")
    }

    fn apply_presence(&mut self, presence: &crate::models::Presence) {
        let id = presence
            .user_id
            .clone()
            .or_else(|| (!presence.user.id.is_empty()).then(|| presence.user.id.clone()));
        let Some(id) = id else {
            return;
        };
        if !presence.user.id.is_empty() {
            self.remember_user(&presence.user);
        }
        if !presence.status.is_empty() {
            self.presences.insert(id, presence.status.clone());
        }
    }

    fn resolve_member(&self, mut member: Member) -> Member {
        if member.user.id.is_empty()
            && let Some(id) = member.user_id.clone()
        {
            if let Some(user) = self.users.get(&id) {
                member.user = user.clone();
            } else {
                member.user.id = id;
            }
        }
        member
    }

    fn upsert_member(&mut self, guild_id: &str, member: Member) {
        let member = self.resolve_member(member);
        self.remember_user(&member.user);
        if let Some(guild) = self.guilds.iter_mut().find(|guild| guild.id == guild_id) {
            if let Some(existing) = guild
                .members
                .iter_mut()
                .find(|existing| existing.id() == member.id())
            {
                *existing = member;
            } else {
                guild.members.push(member);
            }
        }
    }

    fn remember_user(&mut self, user: &User) {
        if !user.id.is_empty() {
            self.users.insert(user.id.clone(), user.clone());
        }
    }

    pub fn guild(&self, id: &str) -> Option<&Guild> {
        self.guilds.iter().find(|g| g.id == id)
    }

    pub fn role(&self, id: &str) -> Option<&crate::models::Role> {
        self.guilds
            .iter()
            .flat_map(|guild| &guild.roles)
            .find(|role| role.id == id)
    }

    pub fn member(&self, guild_id: &str, user_id: &str) -> Option<&Member> {
        self.guild(guild_id)?
            .members
            .iter()
            .find(|member| member.id() == user_id)
    }

    pub fn members(&self, guild_id: &str) -> &[Member] {
        self.guild(guild_id)
            .map(|guild| guild.members.as_slice())
            .unwrap_or_default()
    }

    pub fn member_display_name(&self, guild_id: &str, user: &User) -> String {
        self.member(guild_id, &user.id)
            .map(|member| member.display_name().to_string())
            .unwrap_or_else(|| user.display_name().to_string())
    }

    pub fn member_name<'a>(&'a self, guild_id: &str, user_id: &str) -> Option<&'a str> {
        self.member(guild_id, user_id)
            .map(|member| member.display_name())
    }

    pub fn member_roles(&self, guild_id: &str, user_id: &str) -> Vec<&Role> {
        let Some(guild) = self.guild(guild_id) else {
            return Vec::new();
        };
        let Some(member) = guild.members.iter().find(|member| member.id() == user_id) else {
            return Vec::new();
        };
        let mut roles: Vec<&Role> = member
            .roles
            .iter()
            .filter_map(|id| guild.roles.iter().find(|role| &role.id == id))
            .collect();
        roles.sort_by_key(|role| std::cmp::Reverse(role.position));
        roles
    }

    pub fn member_role_color(&self, guild_id: &str, user_id: &str) -> u32 {
        self.member_roles(guild_id, user_id)
            .first()
            .map(|role| role.color)
            .unwrap_or(0)
    }

    pub fn is_guild_owner(&self, guild_id: &str, user_id: &str) -> bool {
        self.guild(guild_id)
            .and_then(|guild| guild.owner_id.as_deref())
            .is_some_and(|owner| owner == user_id)
    }

    pub fn channel_display_name(&self, channel: &Channel) -> String {
        if let Some(name) = &channel.name {
            return name.clone();
        }
        let mut names: Vec<String> = channel
            .recipient_ids
            .iter()
            .map(|id| {
                self.users
                    .get(id)
                    .map(|user| user.display_name().to_string())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        names.extend(
            channel
                .recipients
                .iter()
                .map(|user| user.display_name().to_string()),
        );
        if names.is_empty() {
            channel.id.clone()
        } else {
            names.join(", ")
        }
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id).or_else(|| {
            self.guilds
                .iter()
                .flat_map(|g| &g.channels)
                .find(|c| c.id == id)
        })
    }

    pub fn text_channels(&self, guild_id: &str) -> Vec<&Channel> {
        let Some(guild) = self.guild(guild_id) else {
            return Vec::new();
        };
        let mut channels: Vec<&Channel> = guild.channels.iter().filter(|c| c.is_text()).collect();
        channels.sort_by_key(|c| c.position);
        channels
    }

    pub fn message(&self, channel_id: &str, message_id: &str) -> Option<&Message> {
        self.messages
            .get(channel_id)?
            .iter()
            .find(|message| message.id == message_id)
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
            if !guild.members.is_empty() {
                existing.members = guild.members;
            }
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

    fn channel_mut(&mut self, id: &str) -> Option<&mut Channel> {
        if let Some(channel) = self.channels.iter_mut().find(|channel| channel.id == id) {
            return Some(channel);
        }
        self.guilds
            .iter_mut()
            .flat_map(|guild| &mut guild.channels)
            .find(|channel| channel.id == id)
    }

    fn message_mut(&mut self, channel_id: &str, message_id: &str) -> Option<&mut Message> {
        self.messages
            .get_mut(channel_id)?
            .iter_mut()
            .find(|message| message.id == message_id)
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
    use crate::models::{Emoji, Guild, Member, Message, Role, User};

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
    fn members_and_role_colors() {
        let mut state = State::default();
        state.apply(&Event::GuildCreate(Box::new(Guild {
            id: "1".to_string(),
            name: "server".to_string(),
            roles: vec![Role {
                id: "role".to_string(),
                name: "Red".to_string(),
                color: 0xff0000,
                position: 2,
            }],
            members: vec![Member {
                user: User {
                    id: "7".to_string(),
                    username: "alice".to_string(),
                    ..Default::default()
                },
                nick: Some("Ally".to_string()),
                roles: vec!["role".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        })));
        let user = User {
            id: "7".to_string(),
            username: "alice".to_string(),
            ..Default::default()
        };
        assert_eq!(state.member_display_name("1", &user), "Ally");
        assert_eq!(state.member_role_color("1", "7"), 0xff0000);
        assert_eq!(state.members("1").len(), 1);
    }

    #[test]
    fn presence_updates_status() {
        let mut state = State::default();
        state.apply(&Event::PresenceUpdate(Box::new(crate::models::Presence {
            user: User {
                id: "7".to_string(),
                username: "a".to_string(),
                ..Default::default()
            },
            user_id: None,
            status: "idle".to_string(),
        })));
        assert_eq!(state.presence("7"), "idle");
        assert_eq!(state.presence("unknown"), "offline");
    }

    #[test]
    fn reactions_add_and_remove() {
        let mut state = State::default();
        state.apply(&Event::MessageCreate(Box::new(message("10", "1", "hi"))));
        let emoji = Emoji {
            id: None,
            name: Some("👍".to_string()),
        };
        state.apply(&Event::ReactionAdd {
            message_id: "10".to_string(),
            channel_id: "1".to_string(),
            user_id: "7".to_string(),
            emoji: emoji.clone(),
        });
        state.apply(&Event::ReactionAdd {
            message_id: "10".to_string(),
            channel_id: "1".to_string(),
            user_id: "8".to_string(),
            emoji: emoji.clone(),
        });
        assert_eq!(state.messages("1")[0].reactions[0].count, 2);

        state.apply(&Event::ReactionRemove {
            message_id: "10".to_string(),
            channel_id: "1".to_string(),
            user_id: "7".to_string(),
            emoji: emoji.clone(),
        });
        assert_eq!(state.messages("1")[0].reactions[0].count, 1);

        state.apply(&Event::ReactionRemove {
            message_id: "10".to_string(),
            channel_id: "1".to_string(),
            user_id: "8".to_string(),
            emoji,
        });
        assert!(state.messages("1")[0].reactions.is_empty());
    }

    #[test]
    fn reaction_remove_all_and_emoji() {
        let mut state = State::default();
        state.apply(&Event::MessageCreate(Box::new(message("10", "1", "hi"))));
        for name in ["a", "b"] {
            state.apply(&Event::ReactionAdd {
                message_id: "10".to_string(),
                channel_id: "1".to_string(),
                user_id: "7".to_string(),
                emoji: Emoji {
                    id: None,
                    name: Some(name.to_string()),
                },
            });
        }
        state.apply(&Event::ReactionRemoveEmoji {
            message_id: "10".to_string(),
            channel_id: "1".to_string(),
            emoji: Emoji {
                id: None,
                name: Some("a".to_string()),
            },
        });
        assert_eq!(state.messages("1")[0].reactions.len(), 1);

        state.apply(&Event::ReactionRemoveAll {
            message_id: "10".to_string(),
            channel_id: "1".to_string(),
        });
        assert!(state.messages("1")[0].reactions.is_empty());
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
