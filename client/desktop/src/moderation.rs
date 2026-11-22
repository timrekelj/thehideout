//! Moderation and guild structure, as state: editing and deleting messages, kicking, banning,
//! leaving, deleting channels and guilds, rotating the invite code and transferring ownership.
//! No view here (the controls come later): `AppState` actions push the commands, `*_reply` methods
//! take the answers, and `message_removed` / `member_removed` / `guild_dropped` /
//! `channel_removed` take the gateway events. Every transition is idempotent, because a
//! reply and its echo on the gateway both arrive.
//!
//! The selectors (`can_delete_message`, `can_kick`, `can_ban`, ...) mirror the server's rules
//! (`perms.rs`) to decide which controls to offer; the server still checks every call.

#![cfg_attr(not(test), allow(dead_code))]

use thehideout_api::{ApiError, Ban, Guild, GuildLayout, MAX_MESSAGE_CHARS, MemberRemovalReason, Message, Permissions};
use thehideout_design::components::ToastKind;

use crate::backend::Command;
use crate::server_settings::error_text;
use crate::state::{AppState, Place, sentence};

/// What a refused moderation call says to the user.
pub fn moderation_error_text(e: &ApiError) -> String {
    match e {
        ApiError::RateLimited => "You're doing that too fast. Wait a moment and try again.".to_owned(),
        ApiError::NotFound => "That no longer exists.".to_owned(),
        ApiError::Precondition(m) if m.contains("owner cannot leave") => {
            "The owner can't leave. Transfer ownership or delete the server first.".to_owned()
        }
        other => error_text(other),
    }
}

impl AppState {
    // ---- who may do what ---------------------------------------------------------------------

    /// Whether `m` is the signed-in user's own message (the only one they may edit).
    pub fn can_edit_message(&self, m: &Message) -> bool {
        m.author_id == self.my_id()
    }

    /// Whether the delete control is offered for `m`: your own, or anyone's in a guild channel
    /// where you hold MANAGE_MESSAGES (never someone else's DM).
    pub fn can_delete_message(&self, m: &Message) -> bool {
        m.author_id == self.my_id() || m.guild_id.is_some_and(|g| self.perms(g).contains(Permissions::MANAGE_MESSAGES))
    }

    /// Whether `user` may be removed by us: KICK_MEMBERS and a top role above theirs (the owner
    /// above everyone); never yourself, never the owner unless you are.
    pub fn can_kick(&self, guild: u64, user: u64) -> bool {
        user != self.my_id() && self.can_moderate(guild, user, Permissions::KICK_MEMBERS)
    }

    /// Whether `user` may be banned by us: BAN_MEMBERS and the same hierarchy rule as `can_kick`.
    pub fn can_ban(&self, guild: u64, user: u64) -> bool {
        user != self.my_id() && self.can_moderate(guild, user, Permissions::BAN_MEMBERS)
    }

    /// Whether the ban list is available to us.
    pub fn can_view_bans(&self, guild: u64) -> bool {
        self.perms(guild).contains(Permissions::BAN_MEMBERS)
    }

    /// Whether leaving is offered: members may, the owner may not.
    pub fn can_leave_guild(&self, guild: u64) -> bool {
        self.guild(guild).is_some_and(|g| g.owner_id != self.my_id())
    }

    /// Whether deleting the guild or handing it over is offered: only the owner.
    pub fn can_delete_guild(&self, guild: u64) -> bool {
        self.is_owner(guild)
    }

    /// The bans of `guild`, newest first, once loaded.
    pub fn bans_of(&self, guild: u64) -> &[Ban] {
        self.data.bans.get(&guild).map_or(&[], Vec::as_slice)
    }

    // ---- actions -----------------------------------------------------------------------------

    /// Edits our message `message` of `channel`. Refused here (nothing is sent) when it is not
    /// ours or the text is empty (unless it has attachments) or too long.
    pub fn edit_message(&mut self, channel: u64, message: u64, content: &str) -> bool {
        let content = content.trim();
        let Some(m) = self.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message)) else {
            return false;
        };
        if !self.can_edit_message(m)
            || (content.is_empty() && m.attachments.is_empty())
            || content.chars().count() > MAX_MESSAGE_CHARS
            || m.content == content
        {
            return false;
        }
        self.outbox.push(Command::EditMessage { channel, message, content: content.to_owned() });
        true
    }

    /// Deletes `message` of `channel` (see `can_delete_message`).
    pub fn delete_message(&mut self, channel: u64, message: u64) -> bool {
        let Some(m) = self.log(channel).and_then(|l| l.messages.iter().find(|m| m.id == message)) else {
            return false;
        };
        if !self.can_delete_message(m) {
            return false;
        }
        self.outbox.push(Command::DeleteMessage { channel, message });
        true
    }

    pub fn kick(&mut self, guild: u64, user: u64) -> bool {
        if !self.can_kick(guild, user) {
            return false;
        }
        self.outbox.push(Command::KickMember { guild, user });
        true
    }

    /// Bans `user`; `delete_seconds` also deletes their recent messages (at most 7 days).
    pub fn ban(&mut self, guild: u64, user: u64, reason: &str, delete_seconds: Option<u32>) -> bool {
        if !self.can_ban(guild, user) {
            return false;
        }
        self.outbox.push(Command::BanMember {
            guild,
            user,
            reason: reason.trim().to_owned(),
            delete_seconds,
        });
        true
    }

    pub fn unban(&mut self, guild: u64, user: u64) -> bool {
        if !self.can_view_bans(guild) {
            return false;
        }
        self.outbox.push(Command::UnbanMember { guild, user });
        true
    }

    /// Asks for the ban list.
    pub fn load_bans(&mut self, guild: u64) -> bool {
        if !self.can_view_bans(guild) {
            return false;
        }
        self.outbox.push(Command::LoadBans { guild });
        true
    }

    pub fn leave_guild(&mut self, guild: u64) -> bool {
        if !self.can_leave_guild(guild) {
            return false;
        }
        self.data.exiting.insert(guild);
        self.outbox.push(Command::LeaveGuild { guild });
        true
    }

    pub fn delete_channel(&mut self, guild: u64, channel: u64) -> bool {
        if !self.can_manage_channels(guild) || self.channel(guild, channel).is_none() {
            return false;
        }
        self.outbox.push(Command::DeleteChannel { guild, channel });
        true
    }

    /// Deletes `guild` for good; `confirm_name` must be its name.
    pub fn delete_guild(&mut self, guild: u64, confirm_name: &str) -> bool {
        let Some(g) = self.guild(guild) else { return false };
        if !self.can_delete_guild(guild) || confirm_name.trim() != g.name {
            return false;
        }
        self.data.exiting.insert(guild);
        self.outbox.push(Command::DeleteGuild { guild, confirm_name: confirm_name.trim().to_owned() });
        true
    }

    pub fn rotate_invite(&mut self, guild: u64) -> bool {
        if !self.can_manage_server(guild) {
            return false;
        }
        self.outbox.push(Command::RotateInviteCode { guild });
        true
    }

    /// Hands `guild` to `user` (a member other than us); `confirm_name` must be its name.
    pub fn transfer_ownership(&mut self, guild: u64, user: u64, confirm_name: &str) -> bool {
        let Some(g) = self.guild(guild) else { return false };
        let member = self.data.members.get(&guild).is_none_or(|m| m.contains(&user));
        if !self.can_delete_guild(guild) || user == self.my_id() || !member || confirm_name.trim() != g.name {
            return false;
        }
        self.outbox.push(Command::TransferOwnership { guild, user, confirm_name: confirm_name.trim().to_owned() });
        true
    }

    // ---- replies -----------------------------------------------------------------------------

    pub fn message_edited(&mut self, result: Result<Message, ApiError>) {
        match result {
            Ok(m) => {
                crate::apply::message_updated(self, m);
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't edit the message. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn message_deleted_reply(&mut self, channel: u64, message: u64, result: Result<(), ApiError>) {
        match result {
            Ok(()) => self.message_removed(channel, message),
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't delete the message. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn kicked(&mut self, guild: u64, user: u64, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                self.member_gone(guild, user);
                let name = self.user_name(user);
                self.toast(ToastKind::Success, format!("Removed {name}"));
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't remove them. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn banned(&mut self, guild: u64, user: u64, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                self.member_gone(guild, user);
                let name = self.user_name(user);
                // A ban list that is on screen is asked for again (the server has the reason and time).
                if self.data.bans.contains_key(&guild) {
                    self.outbox.push(Command::LoadBans { guild });
                }
                self.toast(ToastKind::Success, format!("Banned {name}"));
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't ban them. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn unbanned(&mut self, guild: u64, user: u64, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                if let Some(list) = self.data.bans.get_mut(&guild) {
                    list.retain(|b| b.user.id != user);
                }
                let name = self.user_name(user);
                self.toast(ToastKind::Success, format!("Unbanned {name}"));
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't unban them. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn bans_loaded(&mut self, guild: u64, result: Result<Vec<Ban>, ApiError>) {
        match result {
            Ok(list) => {
                for b in &list {
                    self.data.users.entry(b.user.id).or_insert_with(|| b.user.clone());
                }
                self.data.bans.insert(guild, list);
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't load the bans. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn guild_left(&mut self, guild: u64, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                let name = self.guild(guild).map(|g| g.name.clone());
                self.guild_dropped(guild);
                if let Some(name) = name {
                    self.toast(ToastKind::Success, format!("Left {name}"));
                }
            }
            Err(e) => {
                self.data.exiting.remove(&guild);
                self.toast(ToastKind::Danger, format!("Couldn't leave. {}", sentence(&moderation_error_text(&e))));
            }
        }
    }

    pub fn channel_deleted_reply(&mut self, guild: u64, channel: u64, result: Result<GuildLayout, ApiError>) {
        match result {
            Ok(layout) => {
                let name = self.channel(guild, channel).map(|c| c.name.clone());
                self.channel_removed(guild, channel);
                self.set_layout(guild, layout);
                self.toast(ToastKind::Success, format!("Deleted #{}", name.unwrap_or_default()));
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't delete the channel. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn guild_deleted_reply(&mut self, guild: u64, result: Result<(), ApiError>) {
        match result {
            Ok(()) => {
                let name = self.guild(guild).map(|g| g.name.clone());
                self.guild_dropped(guild);
                if let Some(name) = name {
                    self.toast(ToastKind::Success, format!("Deleted {name}"));
                }
            }
            Err(e) => {
                self.data.exiting.remove(&guild);
                self.toast(ToastKind::Danger, format!("Couldn't delete the server. {}", sentence(&moderation_error_text(&e))));
            }
        }
    }

    pub fn invite_rotated(&mut self, guild: u64, result: Result<String, ApiError>) {
        match result {
            Ok(code) => {
                if let Some(g) = self.data.guilds.iter_mut().find(|g| g.id == guild) {
                    g.invite_code = code;
                }
                self.toast(ToastKind::Success, "New invite code. The old one no longer works.");
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't change the invite code. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    pub fn ownership_transferred(&mut self, user: u64, result: Result<Guild, ApiError>) {
        match result {
            Ok(g) => {
                let (name, who) = (g.name.clone(), self.user_name(user));
                self.set_guild(g);
                self.toast(ToastKind::Success, format!("{who} now owns {name}"));
            }
            Err(e) => self.toast(ToastKind::Danger, format!("Couldn't transfer the server. {}", sentence(&moderation_error_text(&e)))),
        }
    }

    // ---- events and their echoes ---------------------------------------------------------------

    /// `message_deleted`: the message leaves its transcript (and a send of it still pending).
    pub fn message_removed(&mut self, channel: u64, message: u64) {
        if let Some(log) = self.data.logs.get_mut(&channel) {
            log.messages.retain(|m| m.id != message);
            log.breaks.retain(|b| *b != message);
            log.oldest_id = log.messages.first().map(|m| m.id);
        }
    }

    /// `guild_member_removed`: the member leaves the lists. When it is us, the guild goes too
    /// (`guild_removed` follows on the gateway; this just doesn't wait for it).
    pub fn member_removed(&mut self, guild: u64, user: u64, reason: MemberRemovalReason) {
        if user == self.my_id() {
            self.guild_removed_event(guild, Some(reason));
            return;
        }
        self.member_gone(guild, user);
    }

    fn member_gone(&mut self, guild: u64, user: u64) {
        if let Some(list) = self.data.members.get_mut(&guild) {
            list.retain(|u| *u != user);
        }
        if let Some(m) = self.data.member_roles.get_mut(&guild) {
            m.remove(&user);
        }
        if let Some(users) = self.data.voice.get_mut(&guild) {
            users.remove(&user);
        }
    }

    /// `guild_removed`: we are no longer in the guild. Said to the user unless they did it
    /// themselves (leaving or deleting; their reply says so).
    pub fn guild_removed_event(&mut self, guild: u64, reason: Option<MemberRemovalReason>) {
        let Some(name) = self.guild(guild).map(|g| g.name.clone()) else { return };
        let own = self.data.exiting.contains(&guild);
        self.guild_dropped(guild);
        if own {
            return;
        }
        let text = match reason {
            Some(MemberRemovalReason::Kicked) => format!("You were removed from {name}"),
            Some(MemberRemovalReason::Banned) => format!("You were banned from {name}"),
            Some(MemberRemovalReason::Left) => format!("You left {name}"),
            None => format!("You're no longer in {name}"),
        };
        self.toast(ToastKind::Info, text);
    }

    /// Forgets everything about `guild`; if it was open the app goes home.
    pub fn guild_dropped(&mut self, guild: u64) {
        self.data.exiting.remove(&guild);
        let channels: Vec<u64> = self.channels_of(guild).iter().map(|c| c.id).collect();
        let was_open = self.ui.place == Place::Guild(guild);
        self.drop_guild_voice(guild);
        for c in &channels {
            self.data.logs.remove(c);
            self.ui.unread.remove(c);
            self.ui.composers.remove(c);
            self.data.outgoing.retain(|o| o.channel != *c);
        }
        self.data.guilds.retain(|g| g.id != guild);
        self.data.channels.remove(&guild);
        self.data.layouts.remove(&guild);
        self.data.layout_undo.remove(&guild);
        self.data.roles.remove(&guild);
        self.data.role_undo.remove(&guild);
        self.data.role_versions.remove(&guild);
        self.data.member_roles.remove(&guild);
        self.data.access_asked.remove(&guild);
        self.data.members.remove(&guild);
        self.data.members_asked.remove(&guild);
        self.data.emojis.remove(&guild);
        self.data.emojis_asked.remove(&guild);
        self.data.bans.remove(&guild);
        self.ui.guild_channel.remove(&guild);
        self.ui.collapsed.retain(|(g, _)| *g != guild);
        if self.ui.server_settings.as_ref().is_some_and(|s| s.guild == guild) {
            self.ui.server_settings = None;
        }
        if was_open {
            self.go_home();
        }
    }

    /// `channel_deleted` (or our own delete's reply): the channel leaves the lists and, if it was
    /// open, the guild opens on its first text channel.
    pub fn channel_removed(&mut self, guild: u64, channel: u64) {
        if let Some(list) = self.data.channels.get_mut(&guild) {
            list.retain(|c| c.id != channel);
        }
        if let Some(l) = self.data.layouts.get_mut(&guild) {
            for e in &mut l.entries {
                e.channel_ids.retain(|c| *c != channel);
            }
        }
        self.data.logs.remove(&channel);
        self.ui.unread.remove(&channel);
        self.ui.composers.remove(&channel);
        self.data.outgoing.retain(|o| o.channel != channel);
        if let Some(users) = self.data.voice.get_mut(&guild) {
            users.retain(|_, st| st.channel_id != channel);
        }
        if self.ui.guild_channel.get(&guild) == Some(&channel) {
            self.ui.guild_channel.remove(&guild);
            if self.ui.place == Place::Guild(guild)
                && let Some(first) = self.first_text_channel(guild)
            {
                self.select_channel(first);
            }
        }
    }
}
