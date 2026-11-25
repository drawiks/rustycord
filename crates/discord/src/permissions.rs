use std::collections::HashSet;

use crate::models::{Channel, Guild, Member, PermissionOverwrite};

pub const ADMINISTRATOR: u64 = 1 << 3;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;

pub fn bits(value: &str) -> u64 {
    value.parse().unwrap_or(0)
}

pub fn base_permissions(guild: &Guild, member: &Member) -> u64 {
    if guild.owner_id.as_deref() == Some(member.id()) {
        return u64::MAX;
    }

    let mut permissions = 0;
    if let Some(everyone) = guild.roles.iter().find(|role| role.id == guild.id) {
        permissions |= bits(&everyone.permissions);
    }
    for role_id in &member.roles {
        if let Some(role) = guild.roles.iter().find(|role| &role.id == role_id) {
            permissions |= bits(&role.permissions);
        }
    }

    if permissions & ADMINISTRATOR != 0 {
        u64::MAX
    } else {
        permissions
    }
}

pub fn channel_permissions(guild: &Guild, member: &Member, channel: &Channel) -> u64 {
    let base = base_permissions(guild, member);
    if base == u64::MAX {
        return u64::MAX;
    }

    let mut permissions = base;
    let role_ids: HashSet<&str> = member.roles.iter().map(String::as_str).collect();
    let mut layers: Vec<&[PermissionOverwrite]> = Vec::new();

    if let Some(parent_id) = &channel.parent_id
        && let Some(parent) = guild
            .channels
            .iter()
            .find(|parent| &parent.id == parent_id && parent.is_category())
    {
        layers.push(&parent.permission_overwrites);
    }
    layers.push(&channel.permission_overwrites);

    for overwrites in layers {
        if let Some(overwrite) = overwrites
            .iter()
            .find(|overwrite| overwrite.kind == 0 && overwrite.id == guild.id)
        {
            apply(&mut permissions, overwrite);
        }

        let mut allow = 0;
        let mut deny = 0;
        for overwrite in overwrites
            .iter()
            .filter(|overwrite| overwrite.kind == 0 && role_ids.contains(overwrite.id.as_str()))
        {
            allow |= bits(&overwrite.allow);
            deny |= bits(&overwrite.deny);
        }
        permissions = (permissions & !deny) | allow;

        if let Some(overwrite) = overwrites
            .iter()
            .find(|overwrite| overwrite.kind == 1 && overwrite.id == member.id())
        {
            apply(&mut permissions, overwrite);
        }
    }

    permissions
}

pub fn can(permissions: u64, permission: u64) -> bool {
    permissions & permission == permission
}

fn apply(permissions: &mut u64, overwrite: &PermissionOverwrite) {
    *permissions &= !bits(&overwrite.deny);
    *permissions |= bits(&overwrite.allow);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Channel, Guild, Member, PermissionOverwrite, Role};

    fn guild(overwrites: Vec<PermissionOverwrite>) -> Guild {
        Guild {
            id: "1".to_string(),
            name: "g".to_string(),
            roles: vec![
                Role {
                    id: "1".to_string(),
                    name: "@everyone".to_string(),
                    permissions: (VIEW_CHANNEL | READ_MESSAGE_HISTORY).to_string(),
                    ..Default::default()
                },
                Role {
                    id: "2".to_string(),
                    name: "member".to_string(),
                    ..Default::default()
                },
            ],
            channels: vec![Channel {
                id: "10".to_string(),
                kind: Channel::TEXT,
                name: Some("general".to_string()),
                permission_overwrites: overwrites,
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn member(roles: &[&str]) -> Member {
        Member {
            user: crate::models::User {
                id: "7".to_string(),
                username: "user".to_string(),
                ..Default::default()
            },
            roles: roles.iter().map(|role| role.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn everyone_overwrite_denies_view() {
        let guild = guild(vec![PermissionOverwrite {
            id: "1".to_string(),
            kind: 0,
            allow: "0".to_string(),
            deny: VIEW_CHANNEL.to_string(),
        }]);
        let permissions = channel_permissions(&guild, &member(&[]), &guild.channels[0]);
        assert!(!can(permissions, VIEW_CHANNEL));
    }

    #[test]
    fn role_allow_beats_everyone_deny() {
        let guild = guild(vec![
            PermissionOverwrite {
                id: "1".to_string(),
                kind: 0,
                allow: "0".to_string(),
                deny: VIEW_CHANNEL.to_string(),
            },
            PermissionOverwrite {
                id: "2".to_string(),
                kind: 0,
                allow: VIEW_CHANNEL.to_string(),
                deny: "0".to_string(),
            },
        ]);
        let permissions = channel_permissions(&guild, &member(&["2"]), &guild.channels[0]);
        assert!(can(permissions, VIEW_CHANNEL));
    }

    #[test]
    fn member_overwrite_wins() {
        let guild = guild(vec![
            PermissionOverwrite {
                id: "1".to_string(),
                kind: 0,
                allow: "0".to_string(),
                deny: VIEW_CHANNEL.to_string(),
            },
            PermissionOverwrite {
                id: "7".to_string(),
                kind: 1,
                allow: VIEW_CHANNEL.to_string(),
                deny: "0".to_string(),
            },
        ]);
        let permissions = channel_permissions(&guild, &member(&[]), &guild.channels[0]);
        assert!(can(permissions, VIEW_CHANNEL));
    }

    #[test]
    fn category_denies_view() {
        let mut guild = guild(vec![]);
        guild.channels.push(Channel {
            id: "20".to_string(),
            kind: Channel::CATEGORY,
            permission_overwrites: vec![PermissionOverwrite {
                id: "1".to_string(),
                kind: 0,
                allow: "0".to_string(),
                deny: VIEW_CHANNEL.to_string(),
            }],
            ..Default::default()
        });
        guild.channels[0].parent_id = Some("20".to_string());
        let permissions = channel_permissions(&guild, &member(&[]), &guild.channels[0]);
        assert!(!can(permissions, VIEW_CHANNEL));
    }

    #[test]
    fn owner_sees_everything() {
        let mut guild = guild(vec![PermissionOverwrite {
            id: "1".to_string(),
            kind: 0,
            allow: "0".to_string(),
            deny: VIEW_CHANNEL.to_string(),
        }]);
        guild.owner_id = Some("7".to_string());
        let permissions = channel_permissions(&guild, &member(&[]), &guild.channels[0]);
        assert!(can(permissions, VIEW_CHANNEL));
    }
}
