//! The fake world's moderation and structure rules. A child module of `fake` (see the `#[path]`
//! there), so it reaches the world's private fields. The rules are the server's
//! (`proto/README.md`, "Moderation and guild structure"):
//!
//! - editing is for the author only (same length rules as sending); deleting is for the author,
//!   or MANAGE_MESSAGES in a guild channel (never someone else's DM);
//! - kicking needs KICK_MEMBERS, banning BAN_MEMBERS, and both need a top role above the target's
//!   (the owner outranks everyone, nobody acts on the owner or on themself); a ban also works on
//!   non-members and can delete the target's recent messages; a banned user cannot join by code;
//! - the owner cannot leave; deleting a channel needs MANAGE_CHANNELS; deleting a guild and
//!   transferring it are the owner's, with the guild's name as confirmation; rotating the invite
//!   code needs MANAGE_SERVER.
//!
//! Every success also queues the gateway events the server publishes, and (for us being removed)
//! `guild_removed`.

use super::*;
use thehideout_api::{Ban, MemberRemovalReason};

/// The longest span a ban may delete messages for.
const MAX_PURGE_SECONDS: u32 = 7 * 24 * 60 * 60;
const MAX_REASON: usize = 512;

#[derive(Default)]
pub(super) struct Moderation {
    /// Bans per guild, newest first.
    bans: HashMap<u64, Vec<Ban>>,
    rotations: u64,
}

impl Moderation {
    pub(super) fn bans_of(&self, guild: u64) -> Vec<Ban> {
        self.bans.get(&guild).cloned().unwrap_or_default()
    }
}

fn event(e: Event) -> Reply {
    Reply::Gateway(GatewayUpdate::Event(e))
}

impl World {
    pub(super) fn is_banned(&self, guild: u64, user: u64) -> bool {
        self.moderation.bans.get(&guild).is_some_and(|b| b.iter().any(|b| b.user.id == user))
    }

    fn in_guild(&self, guild: u64, user: u64) -> bool {
        self.members.get(&guild).is_some_and(|m| m.contains(&user))
    }

    /// The caller's check: the guild exists, they are in it and hold `bit` (`NotFound` for
    /// non-members, `PermissionDenied` without the bit).
    fn require(&self, guild: u64, me: u64, bit: Permissions) -> Result<(), ApiError> {
        if !self.guilds.iter().any(|g| g.id == guild) || !self.in_guild(guild, me) {
            return Err(ApiError::NotFound);
        }
        if !self.has_perm(guild, me, bit) {
            return Err(ApiError::PermissionDenied);
        }
        Ok(())
    }

    /// The server's `outranks_member`.
    fn outranks_target(&self, guild: u64, me: u64, target: u64) -> bool {
        let (owner, _, top) = self.standing(guild, me);
        let (target_owner, _, target_top) = self.standing(guild, target);
        if owner {
            return true;
        }
        if target_owner {
            return false;
        }
        top.is_some_and(|t| target_top.is_none_or(|p| p < t))
    }

    /// Which guild a channel is in, and whether `user` may read it: a member of its guild, or a
    /// participant of the DM. `None` for anything else (the server says `NotFound`).
    fn reach(&self, channel: u64, user: u64) -> Option<Option<u64>> {
        if let Some(c) = self.channels.iter().find(|c| c.id == channel) {
            return self.in_guild(c.guild_id, user).then_some(Some(c.guild_id));
        }
        self.dms
            .iter()
            .find(|d| d.id == channel)
            .filter(|d| d.participants.iter().any(|p| p.id == user))
            .map(|_| None)
    }

    fn edit_message(&mut self, me: u64, channel: u64, message: u64, content: &str) -> Result<Message, ApiError> {
        self.reach(channel, me).ok_or(ApiError::NotFound)?;
        if self.channels.iter().any(|c| c.id == channel && c.kind == ChannelKind::Voice) {
            return Err(ApiError::Precondition("voice channels have no messages".into()));
        }
        let at = self.messages.get(&channel).and_then(|v| v.iter().position(|m| m.id == message)).ok_or(ApiError::NotFound)?;
        let current = self.messages[&channel][at].clone();
        if current.author_id != me {
            return Err(ApiError::PermissionDenied);
        }
        if (content.trim().is_empty() && current.attachments.is_empty()) || content.chars().count() > MAX_CHARS {
            return Err(ApiError::InvalidArgument("message must be 1 to 4000 characters".into()));
        }
        if current.content == content {
            return Ok(current);
        }
        self.sent_at += 15;
        let edited_at = time(self.sent_at);
        let m = &mut self.messages.get_mut(&channel).unwrap()[at];
        m.content = content.to_owned();
        m.edited_at = Some(edited_at);
        // The old link previews belonged to the old text.
        m.embeds.clear();
        Ok(m.clone())
    }

    fn delete_message(&mut self, me: u64, channel: u64, message: u64) -> Result<(), ApiError> {
        let guild = self.reach(channel, me).ok_or(ApiError::NotFound)?;
        let at = self.messages.get(&channel).and_then(|v| v.iter().position(|m| m.id == message)).ok_or(ApiError::NotFound)?;
        if self.messages[&channel][at].author_id != me {
            match guild {
                Some(g) if self.has_perm(g, me, Permissions::MANAGE_MESSAGES) => {}
                _ => return Err(ApiError::PermissionDenied),
            }
        }
        self.messages.get_mut(&channel).unwrap().remove(at);
        Ok(())
    }

    /// Takes `user` out of `guild`: membership, roles and voice. The events the server publishes:
    /// `guild_member_removed` to the others and, when it is us, `guild_removed`.
    fn remove_member(&mut self, guild: u64, user: u64, reason: MemberRemovalReason) -> Vec<Reply> {
        if let Some(m) = self.members.get_mut(&guild) {
            m.retain(|u| *u != user);
        }
        self.member_roles.remove(&(guild, user));
        let mut out: Vec<Reply> = self.voice_clear_member(guild, user).into_iter().collect();
        out.push(event(Event::GuildMemberRemoved { guild_id: guild, user_id: user, reason }));
        if user == self.me.id {
            out.push(event(Event::GuildRemoved { guild_id: guild }));
        }
        out
    }

    fn delete_channel(&mut self, guild: u64, channel: u64) -> (GuildLayout, Vec<Reply>) {
        self.channels.retain(|c| c.id != channel);
        self.messages.remove(&channel);
        if let Some(l) = self.layouts.get_mut(&guild) {
            for e in &mut l.entries {
                e.channel_ids.retain(|c| *c != channel);
            }
        }
        let layout = self.publish_layout(guild);
        let mut out = self.voice_clear_channel(channel);
        out.push(event(Event::ChannelDeleted { guild_id: guild, channel_id: channel }));
        out.push(self.layout_event(guild));
        (layout, out)
    }

    fn delete_guild(&mut self, guild: u64) -> Vec<Reply> {
        let channels: Vec<u64> = self.channels.iter().filter(|c| c.guild_id == guild).map(|c| c.id).collect();
        let mut out = Vec::new();
        for c in &channels {
            out.extend(self.voice_clear_channel(*c));
            self.messages.remove(c);
        }
        self.channels.retain(|c| c.guild_id != guild);
        self.guilds.retain(|g| g.id != guild);
        self.layouts.remove(&guild);
        self.roles.remove(&guild);
        self.members.remove(&guild);
        self.emojis.remove(&guild);
        self.moderation.bans.remove(&guild);
        self.member_roles.retain(|(g, _), _| *g != guild);
        out.push(event(Event::GuildRemoved { guild_id: guild }));
        out
    }

    /// Answers the moderation and structure commands; any other command comes back for `handle`.
    pub(super) fn moderation_command(&mut self, cmd: Command) -> Result<Vec<Reply>, Command> {
        let me = self.me.id;
        Ok(match cmd {
            Command::EditMessage { channel, message, content } => {
                let result = self.edit_message(me, channel, message, &content);
                let echo = result.clone().ok().map(|m| event(Event::MessageUpdated(m)));
                std::iter::once(Reply::MessageEdited { channel, message, result }).chain(echo).collect()
            }
            Command::DeleteMessage { channel, message } => {
                let result = self.delete_message(me, channel, message);
                let echo = result.is_ok().then(|| event(Event::MessageDeleted { channel_id: channel, message_id: message }));
                std::iter::once(Reply::MessageDeleted { channel, message, result }).chain(echo).collect()
            }
            Command::KickMember { guild, user } => {
                let check = self.require(guild, me, Permissions::KICK_MEMBERS).and_then(|()| {
                    if user == me {
                        Err(ApiError::InvalidArgument("you cannot do this to yourself".into()))
                    } else if !self.in_guild(guild, user) {
                        Err(ApiError::NotFound)
                    } else if !self.outranks_target(guild, me, user) {
                        Err(ApiError::PermissionDenied)
                    } else {
                        Ok(())
                    }
                });
                match check {
                    Err(e) => vec![Reply::Kicked { guild, user, result: Err(e) }],
                    Ok(()) => {
                        let mut out = vec![Reply::Kicked { guild, user, result: Ok(()) }];
                        out.extend(self.remove_member(guild, user, MemberRemovalReason::Kicked));
                        out
                    }
                }
            }
            Command::BanMember { guild, user, reason, delete_seconds } => {
                let check = self.require(guild, me, Permissions::BAN_MEMBERS).and_then(|()| {
                    if user == me {
                        Err(ApiError::InvalidArgument("you cannot do this to yourself".into()))
                    } else if reason.trim().chars().count() > MAX_REASON {
                        Err(ApiError::InvalidArgument("reason must be at most 512 characters".into()))
                    } else if delete_seconds.unwrap_or(0) > MAX_PURGE_SECONDS {
                        Err(ApiError::InvalidArgument("delete_message_seconds must be at most 604800 (7 days)".into()))
                    } else if self.in_guild(guild, user) && !self.outranks_target(guild, me, user) {
                        Err(ApiError::PermissionDenied)
                    } else if !self.users.iter().any(|u| u.id == user) {
                        Err(ApiError::NotFound)
                    } else {
                        Ok(())
                    }
                });
                match check {
                    Err(e) => vec![Reply::Banned { guild, user, result: Err(e) }],
                    Ok(()) => {
                        let target = self.users.iter().find(|u| u.id == user).cloned().unwrap();
                        let list = self.moderation.bans.entry(guild).or_default();
                        list.retain(|b| b.user.id != user);
                        list.insert(0, Ban { user: target, banned_by: me, reason: reason.trim().to_owned(), created_at: time(self.sent_at) });
                        let mut out = vec![Reply::Banned { guild, user, result: Ok(()) }];
                        if self.in_guild(guild, user) {
                            out.extend(self.remove_member(guild, user, MemberRemovalReason::Banned));
                        }
                        if let Some(seconds) = delete_seconds.filter(|s| *s > 0) {
                            let since = now_unix() - i64::from(seconds);
                            let channels: Vec<u64> = self.channels.iter().filter(|c| c.guild_id == guild).map(|c| c.id).collect();
                            for channel in channels {
                                let Some(list) = self.messages.get_mut(&channel) else { continue };
                                let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(list).into_iter().partition(|m| {
                                    m.author_id == user
                                        && m.created_at.duration_since(UNIX_EPOCH).is_ok_and(|d| d.as_secs() as i64 >= since)
                                });
                                *list = kept;
                                out.extend(gone.into_iter().map(|m| event(Event::MessageDeleted { channel_id: channel, message_id: m.id })));
                            }
                        }
                        out
                    }
                }
            }
            Command::UnbanMember { guild, user } => {
                let result = self.require(guild, me, Permissions::BAN_MEMBERS).and_then(|()| {
                    let list = self.moderation.bans.entry(guild).or_default();
                    match list.iter().position(|b| b.user.id == user) {
                        Some(at) => {
                            list.remove(at);
                            Ok(())
                        }
                        None => Err(ApiError::NotFound),
                    }
                });
                vec![Reply::Unbanned { guild, user, result }]
            }
            Command::LoadBans { guild } => {
                let result = self.require(guild, me, Permissions::BAN_MEMBERS).map(|()| self.moderation.bans_of(guild));
                vec![Reply::Bans { guild, result }]
            }
            Command::LeaveGuild { guild } => {
                let owner = self.guilds.iter().find(|g| g.id == guild).map(|g| g.owner_id);
                if owner.is_none() || !self.in_guild(guild, me) {
                    vec![Reply::GuildLeft { guild, result: Err(ApiError::NotFound) }]
                } else if owner == Some(me) {
                    vec![Reply::GuildLeft {
                        guild,
                        result: Err(ApiError::Precondition("the owner cannot leave: transfer ownership or delete the guild first".into())),
                    }]
                } else {
                    let mut out = vec![Reply::GuildLeft { guild, result: Ok(()) }];
                    out.extend(self.remove_member(guild, me, MemberRemovalReason::Left));
                    out
                }
            }
            Command::DeleteChannel { guild, channel } => {
                let check = self.require(guild, me, Permissions::MANAGE_CHANNELS).and_then(|()| {
                    if self.channels.iter().any(|c| c.id == channel && c.guild_id == guild) {
                        Ok(())
                    } else {
                        Err(ApiError::NotFound)
                    }
                });
                match check {
                    Err(e) => vec![Reply::ChannelDeleted { guild, channel, result: Err(e) }],
                    Ok(()) => {
                        let (layout, events) = self.delete_channel(guild, channel);
                        let mut out = vec![Reply::ChannelDeleted { guild, channel, result: Ok(layout) }];
                        out.extend(events);
                        out
                    }
                }
            }
            Command::DeleteGuild { guild, confirm_name } => {
                let found = self.guilds.iter().find(|g| g.id == guild).cloned();
                let result = match found {
                    Some(g) if self.in_guild(guild, me) => {
                        if g.owner_id != me {
                            Err(ApiError::PermissionDenied)
                        } else if confirm_name.trim() != g.name {
                            Err(ApiError::InvalidArgument("confirm_name does not match the guild's name".into()))
                        } else {
                            Ok(())
                        }
                    }
                    _ => Err(ApiError::NotFound),
                };
                match result {
                    Err(e) => vec![Reply::GuildDeleted { guild, result: Err(e) }],
                    Ok(()) => {
                        let mut out = vec![Reply::GuildDeleted { guild, result: Ok(()) }];
                        out.extend(self.delete_guild(guild));
                        out
                    }
                }
            }
            Command::RotateInviteCode { guild } => match self.require(guild, me, Permissions::MANAGE_SERVER) {
                Err(e) => vec![Reply::InviteRotated { guild, result: Err(e) }],
                Ok(()) => {
                    self.moderation.rotations += 1;
                    let code = format!("ROTATED{guild}X{}", self.moderation.rotations);
                    let g = self.guilds.iter_mut().find(|g| g.id == guild).unwrap();
                    g.invite_code = code.clone();
                    let g = g.clone();
                    vec![Reply::InviteRotated { guild, result: Ok(code) }, event(Event::GuildUpdated(g))]
                }
            },
            Command::TransferOwnership { guild, user, confirm_name } => {
                let found = self.guilds.iter().find(|g| g.id == guild).cloned();
                let result = match found {
                    Some(g) if self.in_guild(guild, me) => {
                        if g.owner_id != me {
                            Err(ApiError::PermissionDenied)
                        } else if confirm_name.trim() != g.name || user == me {
                            Err(ApiError::InvalidArgument("confirm_name does not match the guild's name".into()))
                        } else if !self.in_guild(guild, user) {
                            Err(ApiError::NotFound)
                        } else {
                            Ok(())
                        }
                    }
                    _ => Err(ApiError::NotFound),
                };
                match result {
                    Err(e) => vec![Reply::OwnershipTransferred { guild, user, result: Err(e) }],
                    Ok(()) => {
                        let g = self.guilds.iter_mut().find(|g| g.id == guild).unwrap();
                        g.owner_id = user;
                        let g = g.clone();
                        vec![Reply::OwnershipTransferred { guild, user, result: Ok(g.clone()) }, event(Event::GuildUpdated(g))]
                    }
                }
            }
            other => return Err(other),
        })
    }

    // ---- tests and scenes drive the world directly -------------------------------------------

    pub(super) fn remove_member_as_moderator(&mut self, guild: u64, user: u64, reason: MemberRemovalReason) {
        for r in self.remove_member(guild, user, reason) {
            self.enqueue(r);
        }
    }

    pub(super) fn delete_message_as_moderator(&mut self, channel: u64, message: u64) {
        if let Some(list) = self.messages.get_mut(&channel) {
            list.retain(|m| m.id != message);
        }
        self.enqueue(event(Event::MessageDeleted { channel_id: channel, message_id: message }));
    }

    pub(super) fn delete_channel_as_moderator(&mut self, guild: u64, channel: u64) {
        for r in self.delete_channel(guild, channel).1 {
            self.enqueue(r);
        }
    }

    pub(super) fn delete_guild_as_owner(&mut self, guild: u64) {
        for r in self.delete_guild(guild) {
            self.enqueue(r);
        }
    }
}
