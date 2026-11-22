//! `NotificationService`: push device registration, foreground state and
//! notification settings. Dispatch lives in `crate::push`; see
//! `notification_service.proto` for the contract.

use chrono::{DateTime, SubsecRound};
use connectrpc::{ConnectError, RequestContext, Response, ServiceRequest, ServiceResult};

use crate::AppState;
use crate::access::{is_member, require_channel_access};
use crate::auth::{authenticate, bearer_token, hash_token};
use crate::proto::thehideout::v1::*;
use crate::push::settings::{self, Level, Override, Update};
use crate::push::store::{self, Registration};
use crate::push::Platform;

const MAX_TOKEN_LEN: usize = 4096;
const MAX_VERSION_CHARS: usize = 32;
const MAX_LOCALE_CHARS: usize = 35;
/// Overrides accepted in one UpdateNotificationSettings call.
const MAX_OVERRIDES_PER_CALL: usize = 100;
/// RegisterDevice / UnregisterDevice calls per user and minute.
const DEVICE_CALLS_PER_MINUTE: usize = 30;

pub struct NotificationSvc {
    state: AppState,
}

impl NotificationSvc {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    /// Converts and validates a list of overrides; `guild` selects the access check.
    async fn overrides(&self, user_id: u64, list: &[NotificationOverrideView<'_>], guild: bool) -> Result<Vec<(u64, Override)>, ConnectError> {
        let mut out = Vec::with_capacity(list.len());
        for item in list {
            let level = match item.level.as_known() {
                Some(l) => Level::from_proto(l),
                None => return Err(ConnectError::invalid_argument("unknown notification level")),
            };
            let muted_until = match item.muted_until.as_option() {
                // Milliseconds, as stored, so the saved value reads back the same.
                Some(ts) => Some(
                    DateTime::from_timestamp(ts.seconds, ts.nanos.max(0) as u32)
                        .ok_or_else(|| ConnectError::invalid_argument("invalid muted_until"))?
                        .trunc_subsecs(3),
                ),
                None => None,
            };
            let o = Override { level, muted_until };
            // Removing an override needs no access, so settings of guilds the user left can be cleaned up.
            if !o.is_empty() {
                if guild {
                    if !is_member(&self.state, user_id, item.id).await? {
                        return Err(ConnectError::not_found("guild not found"));
                    }
                } else {
                    require_channel_access(&self.state, user_id, item.id).await?;
                }
            }
            out.push((item.id, o));
        }
        Ok(out)
    }
}

fn valid_token(platform: Platform, token: &str) -> bool {
    if token.is_empty() || token.len() > MAX_TOKEN_LEN || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    match platform {
        // APNs tokens are hex (32 bytes today; Apple says they may grow).
        Platform::Ios => token.bytes().all(|b| b.is_ascii_hexdigit()) && token.len() >= 16,
        Platform::Android => token.len() >= 16,
    }
}

fn clean(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(max).collect()
}

impl NotificationService for NotificationSvc {
    async fn register_device(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, RegisterDeviceRequest>,
    ) -> ServiceResult<RegisterDeviceResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let session_hash = hash_token(bearer_token(&ctx).expect("authenticate succeeded"));
        let platform = match req.platform.as_known() {
            Some(PushPlatform::PUSH_PLATFORM_IOS) => Platform::Ios,
            Some(PushPlatform::PUSH_PLATFORM_ANDROID) => Platform::Android,
            _ => return Err(ConnectError::invalid_argument("platform must be ios or android")),
        };
        let token = req.token.trim();
        if !valid_token(platform, token) {
            return Err(ConnectError::invalid_argument("malformed push token"));
        }
        self.state.limiter.check(&format!("push-device:{user_id}"), DEVICE_CALLS_PER_MINUTE, std::time::Duration::from_secs(60))?;
        let app_version = clean(req.app_version, MAX_VERSION_CHARS);
        let device_name = crate::auth::device_label(req.device_name);
        let locale = req.locale.map(|l| clean(l, MAX_LOCALE_CHARS)).filter(|l| !l.is_empty());
        let device_id = store::register(
            &self.state,
            user_id,
            &session_hash,
            &Registration { platform, token, app_version: &app_version, device_name: &device_name, locale: locale.as_deref(), sandbox: req.sandbox },
        )
        .await?;
        Response::ok(RegisterDeviceResponse { device_id, ..Default::default() })
    }

    async fn unregister_device(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UnregisterDeviceRequest>,
    ) -> ServiceResult<UnregisterDeviceResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        self.state.limiter.check(&format!("push-device:{user_id}"), DEVICE_CALLS_PER_MINUTE, std::time::Duration::from_secs(60))?;
        if req.device_id != 0 {
            // Only the caller's own devices: the lookup is keyed by their user id.
            store::delete_device(&self.state, user_id, req.device_id).await?;
        } else if !req.token.trim().is_empty() {
            store::delete_by_token(&self.state, user_id, req.token.trim()).await?;
        }
        Response::ok(UnregisterDeviceResponse::default())
    }

    async fn set_client_state(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, SetClientStateRequest>,
    ) -> ServiceResult<SetClientStateResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let session_hash = hash_token(bearer_token(&ctx).expect("authenticate succeeded"));
        self.state.push.presence.set_foreground(user_id, &session_hash, req.foreground);
        Response::ok(SetClientStateResponse::default())
    }

    async fn get_notification_settings(
        &self,
        ctx: RequestContext,
        _req: ServiceRequest<'_, GetNotificationSettingsRequest>,
    ) -> ServiceResult<GetNotificationSettingsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        let settings = settings::load(&self.state, user_id).await?;
        Response::ok(GetNotificationSettingsResponse { settings: settings.to_proto().into(), ..Default::default() })
    }

    async fn update_notification_settings(
        &self,
        ctx: RequestContext,
        req: ServiceRequest<'_, UpdateNotificationSettingsRequest>,
    ) -> ServiceResult<UpdateNotificationSettingsResponse> {
        let user_id = authenticate(&self.state, &ctx).await?;
        if req.guilds.len() + req.channels.len() > MAX_OVERRIDES_PER_CALL {
            return Err(ConnectError::invalid_argument("at most 100 overrides per call"));
        }
        let guild_default = match req.guild_default {
            None => None,
            Some(level) => match level.as_known().map(Level::from_proto) {
                Some(l @ (Level::All | Level::Mentions | Level::Nothing)) => Some(l),
                _ => return Err(ConnectError::invalid_argument("guild_default must be ALL, MENTIONS or NOTHING")),
            },
        };
        let guilds = self.overrides(user_id, &req.guilds, true).await?;
        let channels = self.overrides(user_id, &req.channels, false).await?;
        let saved = settings::save(
            &self.state,
            user_id,
            &Update { dms_enabled: req.dms_enabled, guild_default, show_previews: req.show_previews, guilds, channels },
        )
        .await?;
        let proto = saved.to_proto();
        self.state.events.notify_user_settings_updated(user_id, proto.clone());
        Response::ok(UpdateNotificationSettingsResponse { settings: proto.into(), ..Default::default() })
    }
}
