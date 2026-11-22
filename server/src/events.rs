//! In-process event bus used by the gateway to fan events out to open streams.
//!
//! The API (`publish_to_guild`, `subscribe_guild`, `notify_user_joined_guild`,
//! `subscribe_user`) is deliberately small so the bus can later be reimplemented on top of
//! a multi-node transport such as NATS without touching the services.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::broadcast;

use crate::proto::thehideout::v1::{
    Channel, ChannelDeleted, DmChannel, EmojisUpdated, Guild, GuildMemberJoined, GuildMemberRemoved, GuildRemoved,
    LayoutUpdated, MemberUpdated, Message, MessageDeleted, ReactionChange, RolesUpdated, User, VoiceMoved,
    VoiceStateUpdate,
};

/// Messages buffered per guild channel before slow receivers start lagging.
const GUILD_CHANNEL_CAPACITY: usize = 1024;
/// Messages buffered per DM channel before slow receivers start lagging.
const DM_CHANNEL_CAPACITY: usize = 1024;
/// Control notifications buffered per user.
const USER_CHANNEL_CAPACITY: usize = 64;

/// An event payload published to a guild or DM channel; the gateway stamps it with a per-stream sequence.
#[derive(Debug, Clone)]
pub enum EventPayload {
    MessageCreated(Message),
    /// The full, current message after a change (see `publish_message_updated`).
    MessageUpdated(Message),
    ChannelCreated(Channel),
    GuildMemberJoined(GuildMemberJoined),
    GuildUpdated(Guild),
    ChannelUpdated(Channel),
    LayoutUpdated(LayoutUpdated),
    RolesUpdated(RolesUpdated),
    MemberUpdated(MemberUpdated),
    EmojisUpdated(EmojisUpdated),
    ReactionAdded(ReactionChange),
    ReactionRemoved(ReactionChange),
    /// A user's voice state changed (state unset = left voice).
    VoiceStateUpdate(VoiceStateUpdate),
    MessageDeleted(MessageDeleted),
    GuildMemberRemoved(GuildMemberRemoved),
    ChannelDeleted(ChannelDeleted),
}

/// Control notification delivered to a user's open streams.
#[derive(Debug, Clone, PartialEq)]
pub enum UserControl {
    /// The user joined this guild; streams should start listening to it and
    /// emit a `guild_added` event for it.
    JoinedGuild(Guild),
    /// A DM channel was opened with the user; streams should start listening to
    /// it and emit a `dm_opened` event for it.
    OpenedDm(DmChannel),
    /// A user sharing a guild or DM with the recipient (or the recipient)
    /// changed their profile; becomes a `user_updated` event.
    ProfileUpdated(User),
    /// A moderator moved the user to another voice channel; becomes a
    /// `voice_moved` event.
    VoiceMoved(VoiceMoved),
    /// The user is no longer in this guild; streams stop listening to it and
    /// emit `guild_removed`.
    RemovedFromGuild(GuildRemoved),
}

#[derive(Default)]
pub struct EventBus {
    guilds: Mutex<HashMap<u64, broadcast::Sender<Arc<EventPayload>>>>,
    dms: Mutex<HashMap<u64, broadcast::Sender<Arc<EventPayload>>>>,
    users: Mutex<HashMap<u64, broadcast::Sender<UserControl>>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Delivers `payload` to every current subscriber of the guild. Never fails; having no
    /// subscribers is normal.
    pub fn publish_to_guild(&self, guild_id: u64, payload: EventPayload) {
        let mut guilds = lock(&self.guilds);
        if let Some(tx) = guilds.get(&guild_id) {
            if tx.receiver_count() == 0 {
                guilds.remove(&guild_id);
            } else {
                let _ = tx.send(Arc::new(payload));
            }
        }
    }

    /// Receives events published to the guild from now on.
    pub fn subscribe_guild(&self, guild_id: u64) -> broadcast::Receiver<Arc<EventPayload>> {
        lock(&self.guilds)
            .entry(guild_id)
            .or_insert_with(|| broadcast::channel(GUILD_CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// Delivers `payload` to every current subscriber of the DM channel. Never fails; having no
    /// subscribers is normal.
    pub fn publish_to_dm(&self, dm_id: u64, payload: EventPayload) {
        let mut dms = lock(&self.dms);
        if let Some(tx) = dms.get(&dm_id) {
            if tx.receiver_count() == 0 {
                dms.remove(&dm_id);
            } else {
                let _ = tx.send(Arc::new(payload));
            }
        }
    }

    /// Receives events published to the DM channel from now on.
    pub fn subscribe_dm(&self, dm_id: u64) -> broadcast::Receiver<Arc<EventPayload>> {
        lock(&self.dms)
            .entry(dm_id)
            .or_insert_with(|| broadcast::channel(DM_CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// Tells the user's open streams they joined `guild`.
    pub fn notify_user_joined_guild(&self, user_id: u64, guild: Guild) {
        self.notify_user(user_id, UserControl::JoinedGuild(guild));
    }

    /// Tells the user's open streams a DM channel was opened with them.
    pub fn notify_user_opened_dm(&self, user_id: u64, channel: DmChannel) {
        self.notify_user(user_id, UserControl::OpenedDm(channel));
    }

    /// Tells the user's open streams that `updated` changed their profile.
    pub fn notify_user_profile_updated(&self, user_id: u64, updated: User) {
        self.notify_user(user_id, UserControl::ProfileUpdated(updated));
    }

    /// Tells the user's open streams a moderator moved them to another voice channel.
    pub fn notify_user_voice_moved(&self, user_id: u64, moved: VoiceMoved) {
        self.notify_user(user_id, UserControl::VoiceMoved(moved));
    }

    /// Tells the user's open streams they are no longer a member of the guild.
    pub fn notify_user_removed_from_guild(&self, user_id: u64, guild_id: u64) {
        self.notify_user(user_id, UserControl::RemovedFromGuild(GuildRemoved { guild_id, ..Default::default() }));
    }

    fn notify_user(&self, user_id: u64, control: UserControl) {
        let mut users = lock(&self.users);
        if let Some(tx) = users.get(&user_id) {
            if tx.receiver_count() == 0 {
                users.remove(&user_id);
            } else {
                let _ = tx.send(control);
            }
        }
    }

    /// Receives control notifications for the user from now on.
    pub fn subscribe_user(&self, user_id: u64) -> broadcast::Receiver<UserControl> {
        lock(&self.users)
            .entry(user_id)
            .or_insert_with(|| broadcast::channel(USER_CHANNEL_CAPACITY).0)
            .subscribe()
    }

    /// Removes guild/DM/user channels nobody listens to anymore; called when a stream ends.
    pub fn prune(&self) {
        lock(&self.guilds).retain(|_, tx| tx.receiver_count() > 0);
        lock(&self.dms).retain(|_, tx| tx.receiver_count() > 0);
        lock(&self.users).retain(|_, tx| tx.receiver_count() > 0);
    }

    /// Number of guild channels currently held (for leak checks in tests).
    pub fn guild_channel_count(&self) -> usize {
        lock(&self.guilds).len()
    }

    /// Number of DM channels currently held (for leak checks in tests).
    pub fn dm_channel_count(&self) -> usize {
        lock(&self.dms).len()
    }

    /// Number of user control channels currently held (for leak checks in tests).
    pub fn user_channel_count(&self) -> usize {
        lock(&self.users).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel_event() -> EventPayload {
        EventPayload::ChannelCreated(Channel::default())
    }

    #[tokio::test]
    async fn publish_reaches_subscribers_and_prunes_dead_channels() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_guild(1);
        bus.publish_to_guild(1, channel_event());
        assert!(matches!(*rx.recv().await.unwrap(), EventPayload::ChannelCreated(_)));
        assert_eq!(bus.guild_channel_count(), 1);

        drop(rx);
        bus.publish_to_guild(1, channel_event());
        assert_eq!(bus.guild_channel_count(), 0);
    }

    #[tokio::test]
    async fn dm_publish_reaches_subscribers_and_prunes_dead_channels() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe_dm(9);
        bus.publish_to_dm(9, channel_event());
        assert!(matches!(*rx.recv().await.unwrap(), EventPayload::ChannelCreated(_)));
        assert_eq!(bus.dm_channel_count(), 1);

        drop(rx);
        bus.publish_to_dm(9, channel_event());
        assert_eq!(bus.dm_channel_count(), 0);
    }

    #[tokio::test]
    async fn user_notifications_are_delivered_and_pruned() {
        let bus = EventBus::new();
        let guild = |id| Guild { id, ..Default::default() };
        bus.notify_user_joined_guild(7, guild(1)); // no subscribers: no-op
        let mut rx = bus.subscribe_user(7);
        bus.notify_user_joined_guild(7, guild(2));
        assert_eq!(rx.recv().await.unwrap(), UserControl::JoinedGuild(guild(2)));
        bus.notify_user_opened_dm(7, DmChannel {
            id: 42,
            ..Default::default()
        });
        assert_eq!(
            rx.recv().await.unwrap(),
            UserControl::OpenedDm(DmChannel {
                id: 42,
                ..Default::default()
            })
        );
        drop(rx);
        bus.prune();
        assert_eq!(bus.user_channel_count(), 0);
    }
}
