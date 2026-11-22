//! The background dispatcher: from bus events to pushes.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use connectrpc::ConnectError;
use futures::StreamExt;
use tokio::sync::broadcast::error::RecvError;

use super::payload::{self, Kind, Notification};
use super::settings::{self, Level};
use super::{Admit, Device, Outcome, Pending, store};
use crate::AppState;
use crate::auth::{internal, session_exists};
use crate::convert::{id_from_db, id_to_db};
use crate::events::Tap;
use crate::proto::thehideout::v1::{Guild, Message};

/// Events handled at the same time.
const MAX_CONCURRENT_EVENTS: usize = 64;
/// Recipients examined at the same time for one message.
const RECIPIENT_CONCURRENCY: usize = 16;
/// Distinct @names looked up per message.
const MAX_MENTION_LOOKUPS: usize = 20;
/// Attempts per device (the first one plus retries).
const MAX_ATTEMPTS: u32 = 4;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(30);

/// Starts the dispatcher task (ends with the server).
pub fn spawn(state: &AppState) {
    let state = state.clone();
    let mut tap = state.events.subscribe_tap();
    tokio::spawn(async move {
        let gate = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_EVENTS));
        loop {
            let event = tokio::select! {
                _ = state.shutdown.triggered() => break,
                event = tap.recv() => event,
            };
            let event = match event {
                Ok(event) => event,
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!(missed, "push dispatcher fell behind; some notifications were skipped");
                    continue;
                }
                Err(RecvError::Closed) => break,
            };
            if state.push.transport.is_none() {
                tracing::debug!("push is not configured; not notifying");
                continue;
            }
            let Ok(permit) = gate.clone().acquire_owned().await else { break };
            let state = state.clone();
            tokio::spawn(async move {
                let result = match &*event {
                    Tap::MessageCreated(message) => handle_message(&state, message).await,
                    Tap::GuildAdded { user_id, guild } => handle_guild_added(&state, *user_id, guild).await,
                };
                if let Err(e) = result {
                    tracing::warn!(error = e.message.as_deref().unwrap_or("?"), "push dispatch failed");
                }
                drop(permit);
            });
        }
    });
}

/// Who should be pushed about `message`, and how.
struct Recipient {
    devices: Vec<Device>,
    show_previews: bool,
    mentioned: bool,
}

async fn handle_message(state: &AppState, message: &Message) -> Result<(), ConnectError> {
    let (guild_id, channel_id, author) = (message.guild_id, message.channel_id, message.author_id);
    let candidates = if guild_id == 0 { dm_partners(state, channel_id, author).await? } else { guild_members(state, guild_id).await? };
    let mentioned = mentioned_ids(state, &message.content).await?;
    let in_guild = (guild_id != 0).then_some(guild_id);

    let recipients: Vec<(u64, Recipient)> = futures::stream::iter(candidates.into_iter().filter(|id| *id != author))
        .map(|user_id| {
            let mentioned = mentioned.contains(&user_id);
            async move { recipient(state, user_id, in_guild, channel_id, mentioned).await.map(|r| r.map(|r| (user_id, r))) }
        })
        .buffer_unordered(RECIPIENT_CONCURRENCY)
        .filter_map(|r| async move {
            match r {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = e.message.as_deref().unwrap_or("?"), "could not evaluate a push recipient");
                    None
                }
            }
        })
        .collect()
        .await;
    if recipients.is_empty() {
        return Ok(());
    }

    let sender = crate::access::find_user(state, id_to_db(author)).await?.map_or_else(|| "Someone".to_owned(), |u| u.username);
    let place = if guild_id == 0 { None } else { Some(names(state, guild_id, channel_id).await?) };
    let title = payload::title(&sender, place.as_ref().map(|(c, g)| (c.as_str(), g.as_str())));
    let has_attachments = !message.attachments.is_empty();
    for (user_id, r) in recipients {
        let kind = if guild_id == 0 {
            Kind::Dm
        } else if r.mentioned {
            Kind::Mention
        } else {
            Kind::Message
        };
        let notification = Notification {
            kind,
            title: title.clone(),
            body: payload::body(&message.content, has_attachments, r.show_previews),
            guild_id,
            channel_id,
            message_id: message.id,
        };
        deliver(state, user_id, notification, r.devices);
    }
    Ok(())
}

async fn handle_guild_added(state: &AppState, user_id: u64, guild: &Guild) -> Result<(), ConnectError> {
    let devices = store::list(state, user_id).await?;
    if devices.is_empty() || state.push.presence.is_active(user_id) {
        return Ok(());
    }
    if settings::load(state, user_id).await?.guild_default == Level::Nothing {
        return Ok(());
    }
    let devices = live_devices(state, devices).await?;
    if devices.is_empty() {
        return Ok(());
    }
    let notification = Notification {
        kind: Kind::GuildAdded,
        title: "Added to a server".to_owned(),
        body: payload::truncate(&format!("You are now a member of {}", guild.name), payload::MAX_BODY_CHARS),
        guild_id: guild.id,
        channel_id: 0,
        message_id: 0,
    };
    deliver(state, user_id, notification, devices);
    Ok(())
}

/// Decides whether `user_id` gets a push for a message: they must have a
/// usable device, not be looking at the app, and have a setting that asks for it.
async fn recipient(state: &AppState, user_id: u64, guild_id: Option<u64>, channel_id: u64, mentioned: bool) -> Result<Option<Recipient>, ConnectError> {
    let devices = store::list(state, user_id).await?;
    if devices.is_empty() || state.push.presence.is_active(user_id) {
        return Ok(None);
    }
    let settings = settings::load(state, user_id).await?;
    let notify = match settings.effective(guild_id, channel_id, Utc::now()) {
        Level::All => true,
        Level::Mentions => mentioned,
        Level::Nothing | Level::Inherit => false,
    };
    if !notify {
        return Ok(None);
    }
    let devices = live_devices(state, devices).await?;
    if devices.is_empty() {
        return Ok(None);
    }
    Ok(Some(Recipient { devices, show_previews: settings.show_previews, mentioned }))
}

/// The devices the transport can serve whose session still exists; devices
/// of expired or revoked sessions are deleted on the spot.
async fn live_devices(state: &AppState, devices: Vec<Device>) -> Result<Vec<Device>, ConnectError> {
    let Some(transport) = &state.push.transport else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for device in devices {
        if !transport.supports(device.platform) {
            continue;
        }
        if !device.session_hash.is_empty() && !session_exists(state, &device.session_hash).await? {
            store::delete_device(state, device.user_id, device.id).await?;
            continue;
        }
        out.push(device);
    }
    Ok(out)
}

/// Sends now, or holds the push for the end of the channel's coalescing window.
fn deliver(state: &AppState, user_id: u64, notification: Notification, devices: Vec<Device>) {
    let key = (user_id, if notification.channel_id != 0 { notification.channel_id } else { notification.guild_id });
    match state.push.coalescer.admit(key, Pending { notification, devices }) {
        Admit::Now(pending) => send_all(state, pending),
        Admit::Merged => {}
        Admit::Later(delay) => {
            let state = state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                if let Some(pending) = state.push.coalescer.take(key) {
                    send_all(&state, pending);
                }
            });
        }
    }
}

fn send_all(state: &AppState, pending: Pending) {
    let notification = Arc::new(pending.notification);
    for device in pending.devices {
        let (state, notification) = (state.clone(), notification.clone());
        tokio::spawn(async move { send_one(&state, &device, &notification).await });
    }
}

/// One device, with backoff on 429 / 5xx and removal of dead tokens.
async fn send_one(state: &AppState, device: &Device, notification: &Notification) {
    let Some(transport) = &state.push.transport else { return };
    for attempt in 0..MAX_ATTEMPTS {
        let outcome = {
            let Ok(_permit) = state.push.sends.acquire().await else { return };
            transport.send(device, notification).await
        };
        match outcome {
            Outcome::Delivered => return,
            Outcome::InvalidToken => {
                tracing::info!(device = device.id, user = device.user_id, platform = device.platform.as_str(), "push token is no longer valid; removing the device");
                if let Err(e) = store::delete_device(state, device.user_id, device.id).await {
                    tracing::warn!(error = e.message.as_deref().unwrap_or("?"), "removing a dead device failed");
                }
                return;
            }
            Outcome::Retry(after, why) => {
                if attempt + 1 == MAX_ATTEMPTS {
                    tracing::warn!(device = device.id, why, "push gave up after retries");
                    return;
                }
                let backoff = state.push.tuning.retry_backoff * 2u32.pow(attempt);
                tokio::time::sleep(after.unwrap_or(backoff).min(MAX_RETRY_DELAY)).await;
            }
            Outcome::Failed(why) => {
                tracing::warn!(device = device.id, platform = device.platform.as_str(), why, "push failed");
                return;
            }
        }
    }
}

/// The other participant of a DM channel.
async fn dm_partners(state: &AppState, dm_id: u64, author: u64) -> Result<Vec<u64>, ConnectError> {
    let row = state
        .session
        .execute_unpaged("SELECT user_a, user_b FROM dm_channels WHERE id = ?", (id_to_db(dm_id),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .maybe_first_row::<(i64, i64)>()
        .map_err(internal)?;
    Ok(row.into_iter().flat_map(|(a, b)| [id_from_db(a), id_from_db(b)]).filter(|id| *id != author).collect())
}

async fn guild_members(state: &AppState, guild_id: u64) -> Result<Vec<u64>, ConnectError> {
    Ok(state
        .session
        .execute_unpaged("SELECT user_id FROM guild_members WHERE guild_id = ?", (id_to_db(guild_id),))
        .await
        .map_err(internal)?
        .into_rows_result()
        .map_err(internal)?
        .rows::<(i64,)>()
        .map_err(internal)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal)?
        .into_iter()
        .map(|(id,)| id_from_db(id))
        .collect())
}

/// Ids of the users `content` @mentions (unknown names are ignored).
async fn mentioned_ids(state: &AppState, content: &str) -> Result<BTreeSet<u64>, ConnectError> {
    let mut ids = BTreeSet::new();
    for name in payload::mentioned_usernames(content).into_iter().take(MAX_MENTION_LOOKUPS) {
        if let Some(id) = crate::accounts::id_by_username(state, &name).await? {
            ids.insert(id);
        }
    }
    Ok(ids)
}

/// Channel and guild names for the title.
async fn names(state: &AppState, guild_id: u64, channel_id: u64) -> Result<(String, String), ConnectError> {
    let first = |rows: scylla::response::query_result::QueryRowsResult| -> Result<Option<String>, ConnectError> {
        Ok(rows.maybe_first_row::<(Option<String>,)>().map_err(internal)?.and_then(|(n,)| n))
    };
    let channel = first(
        state
            .session
            .execute_unpaged("SELECT name FROM channels WHERE guild_id = ? AND id = ?", (id_to_db(guild_id), id_to_db(channel_id)))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?,
    )?;
    let guild = first(
        state
            .session
            .execute_unpaged("SELECT name FROM guilds WHERE id = ?", (id_to_db(guild_id),))
            .await
            .map_err(internal)?
            .into_rows_result()
            .map_err(internal)?,
    )?;
    Ok((channel.unwrap_or_else(|| "channel".to_owned()), guild.unwrap_or_else(|| "server".to_owned())))
}
