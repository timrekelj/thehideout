//! Moderation and guild-structure calls of [`Client`]: editing and deleting messages, kick, ban,
//! leaving, deleting channels and guilds, rotating the invite code and transferring ownership.

use crate::client::{Client, required};
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;
use crate::structure::{Ban, GuildLayout};
use crate::types::{Guild, Message};

type Result<T> = std::result::Result<T, ApiError>;

impl Client {
    /// Replaces the content of your own message (same limits as [`send_message`](Self::send_message);
    /// it may be empty only when the message has attachments). Others' messages are
    /// [`ApiError::PermissionDenied`]. Counts against the message rate limit
    /// ([`ApiError::RateLimited`]).
    pub async fn edit_message(&self, channel_id: u64, message_id: u64, content: &str) -> Result<Message> {
        let res = self
            .messages()?
            .edit_message(pb::EditMessageRequest { channel_id, message_id, content: content.into(), ..Default::default() })
            .await?
            .into_owned();
        required(res.message.into_option())
    }

    /// Deletes a message: your own, or anyone's in a guild channel with `MANAGE_MESSAGES`.
    pub async fn delete_message(&self, channel_id: u64, message_id: u64) -> Result<()> {
        self.messages()?.delete_message(pb::DeleteMessageRequest { channel_id, message_id, ..Default::default() }).await?;
        Ok(())
    }

    /// Removes a member (needs `KICK_MEMBERS` and a top role above theirs). They may rejoin.
    pub async fn kick_member(&self, guild_id: u64, user_id: u64) -> Result<()> {
        self.guilds()?.kick_member(pb::KickMemberRequest { guild_id, user_id, ..Default::default() }).await?;
        Ok(())
    }

    /// Bans a user (needs `BAN_MEMBERS` and a top role above theirs if they are a member): removes
    /// them and blocks rejoining. `delete_message_seconds` (at most 7 days) also deletes their
    /// recent messages in the background; `reason` is at most 512 characters.
    pub async fn ban_member(&self, guild_id: u64, user_id: u64, reason: &str, delete_message_seconds: Option<u32>) -> Result<()> {
        self.guilds()?
            .ban_member(pb::BanMemberRequest {
                guild_id,
                user_id,
                reason: reason.into(),
                delete_message_seconds,
                ..Default::default()
            })
            .await?;
        Ok(())
    }

    /// Lifts a ban (needs `BAN_MEMBERS`); [`ApiError::NotFound`] if the user is not banned.
    pub async fn unban_member(&self, guild_id: u64, user_id: u64) -> Result<()> {
        self.guilds()?.unban_member(pb::UnbanMemberRequest { guild_id, user_id, ..Default::default() }).await?;
        Ok(())
    }

    /// The guild's bans, newest first (needs `BAN_MEMBERS`).
    pub async fn list_bans(&self, guild_id: u64) -> Result<Vec<Ban>> {
        let res = self.guilds()?.list_bans(pb::ListBansRequest { guild_id, ..Default::default() }).await?.into_owned();
        Ok(res.bans.into_iter().map(Ban::from).collect())
    }

    /// Leaves a guild. The owner cannot ([`ApiError::Precondition`]): transfer ownership or
    /// delete the guild first.
    pub async fn leave_guild(&self, guild_id: u64) -> Result<()> {
        self.guilds()?.leave_guild(pb::LeaveGuildRequest { guild_id, ..Default::default() }).await?;
        Ok(())
    }

    /// Deletes a channel and its messages (needs `MANAGE_CHANNELS`); returns the new layout.
    pub async fn delete_channel(&self, guild_id: u64, channel_id: u64) -> Result<GuildLayout> {
        let res = self
            .guilds()?
            .delete_channel(pb::DeleteChannelRequest { guild_id, channel_id, ..Default::default() })
            .await?
            .into_owned();
        required(res.layout.into_option())
    }

    /// Deletes a guild with everything in it (owner only). `confirm_name` must equal the guild's
    /// name ([`ApiError::InvalidArgument`] otherwise).
    pub async fn delete_guild(&self, guild_id: u64, confirm_name: &str) -> Result<()> {
        self.guilds()?
            .delete_guild(pb::DeleteGuildRequest { guild_id, confirm_name: confirm_name.into(), ..Default::default() })
            .await?;
        Ok(())
    }

    /// Replaces the invite code (needs `MANAGE_SERVER`); the old one stops working. Returns the new one.
    pub async fn rotate_invite_code(&self, guild_id: u64) -> Result<String> {
        let res = self.guilds()?.rotate_invite_code(pb::RotateInviteCodeRequest { guild_id, ..Default::default() }).await?.into_owned();
        Ok(res.invite_code)
    }

    /// Makes another member the owner (owner only); `confirm_name` must equal the guild's name.
    pub async fn transfer_ownership(&self, guild_id: u64, user_id: u64, confirm_name: &str) -> Result<Guild> {
        let res = self
            .guilds()?
            .transfer_ownership(pb::TransferOwnershipRequest { guild_id, user_id, confirm_name: confirm_name.into(), ..Default::default() })
            .await?
            .into_owned();
        required(res.guild.into_option())
    }
}
