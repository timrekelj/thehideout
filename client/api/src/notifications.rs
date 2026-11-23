//! Push notification calls of [`Client`]: device registration for the mobile apps, the
//! foreground state, and the notification settings (also used by the desktop later).

use std::time::SystemTime;

use buffa_types::google::protobuf::Timestamp;
use crate::transport::Transport;

use crate::client::{Client, required};
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;
use crate::types::time;

type Result<T> = std::result::Result<T, ApiError>;

/// Push service of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushPlatform {
    /// Apple Push Notification service.
    Ios,
    /// Firebase Cloud Messaging.
    Android,
}

impl PushPlatform {
    fn to_proto(self) -> pb::PushPlatform {
        match self {
            Self::Ios => pb::PushPlatform::PUSH_PLATFORM_IOS,
            Self::Android => pb::PushPlatform::PUSH_PLATFORM_ANDROID,
        }
    }
}

/// A device to push to ([`Client::register_device`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRegistration {
    pub platform: PushPlatform,
    /// The APNs device token (hex) or FCM registration token.
    pub token: String,
    pub app_version: String,
    pub device_name: String,
    /// BCP 47 language of the app, e.g. `sl-SI`.
    pub locale: Option<String>,
    /// iOS only: the token is from a development build (APNs sandbox).
    pub sandbox: bool,
}

/// What a guild, channel or the account notifies about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    /// Overrides only: use the next less specific setting.
    Inherit,
    /// Every message.
    All,
    /// Only messages that @mention you.
    Mentions,
    /// Nothing.
    Nothing,
}

impl NotificationLevel {
    fn from_proto(level: &buffa::EnumValue<pb::NotificationLevel>) -> Self {
        match level.as_known() {
            Some(pb::NotificationLevel::NOTIFICATION_LEVEL_ALL) => Self::All,
            Some(pb::NotificationLevel::NOTIFICATION_LEVEL_MENTIONS) => Self::Mentions,
            Some(pb::NotificationLevel::NOTIFICATION_LEVEL_NOTHING) => Self::Nothing,
            _ => Self::Inherit,
        }
    }

    fn to_proto(self) -> pb::NotificationLevel {
        match self {
            Self::Inherit => pb::NotificationLevel::NOTIFICATION_LEVEL_INHERIT,
            Self::All => pb::NotificationLevel::NOTIFICATION_LEVEL_ALL,
            Self::Mentions => pb::NotificationLevel::NOTIFICATION_LEVEL_MENTIONS,
            Self::Nothing => pb::NotificationLevel::NOTIFICATION_LEVEL_NOTHING,
        }
    }
}

/// The setting of one guild or channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationOverride {
    /// The guild or channel id.
    pub id: u64,
    pub level: NotificationLevel,
    /// Muted until then (a far-future time = until turned on again).
    pub muted_until: Option<SystemTime>,
}

impl NotificationOverride {
    /// An override that changes nothing; sending it removes the stored one.
    pub fn reset(id: u64) -> Self {
        Self { id, level: NotificationLevel::Inherit, muted_until: None }
    }

    fn to_proto(self) -> pb::NotificationOverride {
        pb::NotificationOverride {
            id: self.id,
            level: self.level.to_proto().into(),
            muted_until: self.muted_until.map(Timestamp::from).into(),
            ..Default::default()
        }
    }
}

impl From<pb::NotificationOverride> for NotificationOverride {
    fn from(o: pb::NotificationOverride) -> Self {
        Self {
            id: o.id,
            level: NotificationLevel::from_proto(&o.level),
            muted_until: o.muted_until.as_option().map(|t| time(Some(t))),
        }
    }
}

/// The account's notification settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSettings {
    /// Push every DM message.
    pub dms_enabled: bool,
    /// Level of guild channels without an override: `All`, `Mentions` or `Nothing`.
    pub guild_default: NotificationLevel,
    /// Include the message text in pushes.
    pub show_previews: bool,
    pub guilds: Vec<NotificationOverride>,
    pub channels: Vec<NotificationOverride>,
}

impl From<pb::NotificationSettings> for NotificationSettings {
    fn from(s: pb::NotificationSettings) -> Self {
        Self {
            dms_enabled: s.dms_enabled,
            guild_default: NotificationLevel::from_proto(&s.guild_default),
            show_previews: s.show_previews,
            guilds: s.guilds.into_iter().map(Into::into).collect(),
            channels: s.channels.into_iter().map(Into::into).collect(),
        }
    }
}

/// A change to the settings; `None` / empty leaves things as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotificationSettingsUpdate {
    pub dms_enabled: Option<bool>,
    /// `All`, `Mentions` or `Nothing`.
    pub guild_default: Option<NotificationLevel>,
    pub show_previews: Option<bool>,
    /// Replaces the override of each guild (at most 100 per call).
    pub guilds: Vec<NotificationOverride>,
    /// Replaces the override of each channel (guild or DM).
    pub channels: Vec<NotificationOverride>,
}

impl Client {
    fn notifications(&self) -> Result<pb::NotificationServiceClient<Transport>> {
        Ok(pb::NotificationServiceClient::new(self.http.clone(), self.config()?))
    }

    /// Registers the push token of this session and returns the device id. Call it again
    /// whenever the token changes and at launch. [`ApiError::Other`] for a malformed token.
    pub async fn register_device(&self, device: &DeviceRegistration) -> Result<u64> {
        let res = self
            .notifications()?
            .register_device(pb::RegisterDeviceRequest {
                platform: device.platform.to_proto().into(),
                token: device.token.clone(),
                app_version: device.app_version.clone(),
                device_name: device.device_name.clone(),
                locale: device.locale.clone(),
                sandbox: device.sandbox,
                ..Default::default()
            })
            .await?
            .into_owned();
        Ok(res.device_id)
    }

    /// Stops pushes to a device by id. Unknown devices are a no-op.
    pub async fn unregister_device(&self, device_id: u64) -> Result<()> {
        self.notifications()?.unregister_device(pb::UnregisterDeviceRequest { device_id, ..Default::default() }).await?;
        Ok(())
    }

    /// Stops pushes to a device by token.
    pub async fn unregister_device_token(&self, token: &str) -> Result<()> {
        self.notifications()?.unregister_device(pb::UnregisterDeviceRequest { token: token.into(), ..Default::default() }).await?;
        Ok(())
    }

    /// Tells the server whether the app is in the foreground; while it is, the account is not
    /// pushed. Mobile apps call it with `false` when they go to the background.
    pub async fn set_client_state(&self, foreground: bool) -> Result<()> {
        self.notifications()?.set_client_state(pb::SetClientStateRequest { foreground, ..Default::default() }).await?;
        Ok(())
    }

    /// The notification settings (defaults when never changed).
    pub async fn notification_settings(&self) -> Result<NotificationSettings> {
        let res = self.notifications()?.get_notification_settings(pb::GetNotificationSettingsRequest::default()).await?.into_owned();
        required(res.settings.into_option())
    }

    /// Changes settings and returns the result. Other sessions get
    /// [`Event::NotificationSettingsUpdated`](crate::Event::NotificationSettingsUpdated).
    pub async fn update_notification_settings(&self, update: &NotificationSettingsUpdate) -> Result<NotificationSettings> {
        let res = self
            .notifications()?
            .update_notification_settings(pb::UpdateNotificationSettingsRequest {
                dms_enabled: update.dms_enabled,
                guild_default: update.guild_default.map(|l| l.to_proto().into()),
                show_previews: update.show_previews,
                guilds: update.guilds.iter().map(|o| o.to_proto()).collect(),
                channels: update.channels.iter().map(|o| o.to_proto()).collect(),
                ..Default::default()
            })
            .await?
            .into_owned();
        required(res.settings.into_option())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn settings_convert_from_the_wire() {
        let until = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let wire = pb::NotificationSettings {
            dms_enabled: true,
            guild_default: pb::NotificationLevel::NOTIFICATION_LEVEL_MENTIONS.into(),
            show_previews: false,
            guilds: vec![pb::NotificationOverride {
                id: 4,
                level: pb::NotificationLevel::NOTIFICATION_LEVEL_NOTHING.into(),
                muted_until: Timestamp::from(until).into(),
                ..Default::default()
            }],
            channels: vec![pb::NotificationOverride { id: 5, level: pb::NotificationLevel::NOTIFICATION_LEVEL_ALL.into(), ..Default::default() }],
            ..Default::default()
        };
        let s = NotificationSettings::from(wire);
        assert!(s.dms_enabled && !s.show_previews);
        assert_eq!(s.guild_default, NotificationLevel::Mentions);
        assert_eq!(s.guilds, [NotificationOverride { id: 4, level: NotificationLevel::Nothing, muted_until: Some(until) }]);
        assert_eq!(s.channels, [NotificationOverride { id: 5, level: NotificationLevel::All, muted_until: None }]);
    }

    #[test]
    fn overrides_round_trip_and_reset_is_empty() {
        let until = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let o = NotificationOverride { id: 9, level: NotificationLevel::All, muted_until: Some(until) };
        assert_eq!(NotificationOverride::from(o.to_proto()), o);
        let reset = NotificationOverride::reset(3).to_proto();
        assert_eq!(NotificationLevel::from_proto(&reset.level), NotificationLevel::Inherit);
        assert!(!reset.muted_until.is_set());
    }
}
