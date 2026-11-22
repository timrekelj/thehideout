//! Link embeds: after a message is created, the URLs in it are unfurled in
//! the background (title, description, site name, a WebP thumbnail), stored in
//! `message_embeds` and announced with `message_updated`.
//!
//! - [`extract`]: which URLs of a message count (only code spans and fences are skipped).
//! - [`ssrf`] + [`fetch`]: the SSRF-safe HTTP client every request goes through.
//! - [`html`]: metadata of the page head.
//! - [`provider`] + [`providers`]: how a URL becomes an [`provider::Unfurled`]
//!   (text + media sources): the generic OpenGraph provider and the X,
//!   TikTok, Instagram and Reddit ones (through mirrors, see `providers`).
//! - [`download`]: downloads the media of provider embeds into S3.
//!
//! Pipeline per URL: choose provider → (cache) → provider describes the link →
//! the media step downloads the sources through the same safe client (generic
//! links: the first image, re-encoded as the thumbnail; provider embeds:
//! up to four images/videos/GIFs, a poster per video and the author avatar) →
//! rows are stored.
//!
//! Failure is silent by design: a URL that cannot be fetched, is blocked,
//! times out or has nothing to show simply gets no embed; if none of the
//! message's URLs yields one, nothing is stored or published.
//!
//! Stored files are never shared between embeds: each embed owns its objects
//! (`embeds/{channel_id}/{id}.webp|mp4|webm`) with their own accounting rows, counted
//! against the guild (DMs: global only, with the author as owner). The per-URL
//! cache (`embed_cache`) therefore holds only metadata and the thumbnail's
//! source URL; a cache hit still fetches and re-encodes the image.
//!
//! Crash window: the objects are uploaded before the embed rows (their
//! accounting rows just before); a crash in between leaves objects no message
//! points at, which the admin file list shows and can delete.

pub mod extract;
pub mod fetch;
pub mod html;
pub mod download;
pub mod provider;
pub mod providers;
pub mod ssrf;

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::future::join_all;
use tokio::sync::Semaphore;
use url::Url;

use crate::AppState;
use crate::auth::internal;
use crate::convert::id_to_db;
use crate::media::{self, EmbedThumbnail, Owner};
use crate::proto::thehideout::v1::{Embed, EmbedKind, EmbedMedia, EmbedMediaKind};
use crate::services::message::publish_message_updated;
use crate::snowflake::bucket_of;
use fetch::Fetcher;
use provider::{AssetKind, Ctx, Provider, Unfurled};
use providers::Mirrors;
use ssrf::Policy;

/// Largest HTML prefix read from a page.
pub const HTML_CAP: usize = 1024 * 1024;
/// Largest image downloaded for a thumbnail or a direct image link.
pub const IMAGE_CAP: usize = 5 * 1024 * 1024;
/// Largest image of a provider embed.
pub const MEDIA_IMAGE_CAP: usize = 10 * 1024 * 1024;
/// Default largest video of a provider embed (`THEHIDEOUT_EMBED_MAX_VIDEO_BYTES`).
pub const MAX_VIDEO_BYTES: usize = 50 * 1024 * 1024;
/// Most media stored per embed.
pub const MAX_MEDIA: usize = 4;
/// Messages unfurled at once; further ones are dropped (and logged).
pub const MAX_CONCURRENT_JOBS: usize = 8;

/// Tunables of the embed pipeline. `Default` is production.
#[derive(Clone)]
pub struct EmbedSettings {
    /// `THEHIDEOUT_EMBEDS=on|off`.
    pub enabled: bool,
    /// Lets the fetcher reach IPv4 loopback on any port. For tests only: it is
    /// deliberately not configurable from the environment.
    pub allow_loopback_for_tests: bool,
    /// Prefix of the keys in the per-URL cache (`embed_cache`). Empty in
    /// production; tests sharing one keyspace give each app its own so their
    /// cached results never mix.
    pub cache_namespace: String,
    pub connect_timeout: Duration,
    /// Deadline of one fetch (all redirects and the body).
    pub total_timeout: Duration,
    pub max_redirects: usize,
    pub html_cap: usize,
    pub image_cap: usize,
    /// How long an unfurl result is reused.
    pub cache_ttl: Duration,
    /// How long "nothing useful found" is remembered.
    pub negative_cache_ttl: Duration,
    /// Mirror and API base URLs of the provider embeds.
    pub mirrors: Mirrors,
    /// Largest image of a provider embed.
    pub media_image_cap: usize,
    /// Largest video of a provider embed; a bigger one is not stored.
    pub max_video_bytes: usize,
    pub max_media: usize,
    /// Deadline of one provider run (mirror failover included).
    pub provider_timeout: Duration,
    /// Deadline of one request to one mirror.
    pub attempt_timeout: Duration,
    /// Deadline of one media download (all hops and the body).
    pub media_timeout: Duration,
    /// Contact shown in the user agent (`THEHIDEOUT_EMBEDS_CONTACT`), e.g. a URL or `mailto:`.
    pub contact: Option<String>,
    slots: Arc<Semaphore>,
    max_jobs: usize,
}

impl Default for EmbedSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_loopback_for_tests: false,
            cache_namespace: String::new(),
            connect_timeout: Duration::from_secs(2),
            total_timeout: Duration::from_secs(5),
            max_redirects: 3,
            html_cap: HTML_CAP,
            image_cap: IMAGE_CAP,
            cache_ttl: Duration::from_secs(24 * 3600),
            negative_cache_ttl: Duration::from_secs(3600),
            mirrors: Mirrors::default(),
            media_image_cap: MEDIA_IMAGE_CAP,
            max_video_bytes: MAX_VIDEO_BYTES,
            max_media: MAX_MEDIA,
            provider_timeout: Duration::from_secs(15),
            attempt_timeout: Duration::from_secs(6),
            media_timeout: Duration::from_secs(30),
            contact: None,
            slots: Arc::new(Semaphore::new(MAX_CONCURRENT_JOBS)),
            max_jobs: MAX_CONCURRENT_JOBS,
        }
    }
}

impl EmbedSettings {
    pub fn disabled() -> Self {
        Self { enabled: false, ..Self::default() }
    }

    /// Allows only this many jobs at once.
    pub fn with_max_concurrent(mut self, jobs: usize) -> Self {
        self.slots = Arc::new(Semaphore::new(jobs));
        self.max_jobs = jobs;
        self
    }

    /// Jobs running right now (tests wait for zero to know a negative outcome is final).
    pub fn in_flight(&self) -> usize {
        self.max_jobs - self.slots.available_permits()
    }

    pub fn fetcher(&self) -> Fetcher {
        let user_agent = match &self.contact {
            Some(c) => format!("thehideout-embed/0.1 (link preview bot; +{c})"),
            None => "thehideout-embed/0.1 (link preview bot)".to_owned(),
        };
        Fetcher {
            policy: Policy { allow_loopback_for_tests: self.allow_loopback_for_tests },
            connect_timeout: self.connect_timeout,
            total_timeout: self.total_timeout,
            max_redirects: self.max_redirects,
            user_agent,
        }
    }

    /// The fetcher for mirrors, APIs and their CDNs: the same bot, but with the
    /// `Mozilla/5.0 (compatible; …)` shape some mirrors insist on.
    pub fn mirror_fetcher(&self) -> Fetcher {
        let user_agent = match &self.contact {
            Some(c) => format!("Mozilla/5.0 (compatible; thehideout-embed/0.1; link preview bot; +{c})"),
            None => "Mozilla/5.0 (compatible; thehideout-embed/0.1; link preview bot)".to_owned(),
        };
        Fetcher { user_agent, ..self.fetcher() }
    }
}

/// A freshly created message to unfurl.
pub struct Job {
    pub channel_id: u64,
    /// 0 for DM channels.
    pub guild_id: u64,
    pub message_id: u64,
    pub author_id: u64,
    pub content: String,
}

/// Starts unfurling the URLs of a message in the background, if there are
/// any and a job slot is free. Never blocks the caller.
pub fn spawn(state: &AppState, job: Job) {
    if !state.embeds.enabled {
        return;
    }
    let urls = extract::extract_urls(&job.content);
    if urls.is_empty() {
        return;
    }
    let Ok(permit) = state.embeds.slots.clone().try_acquire_owned() else {
        tracing::warn!(message_id = job.message_id, "embed queue saturated; not unfurling this message");
        return;
    };
    let state = state.clone();
    tokio::spawn(async move {
        run(&state, &job, urls).await;
        drop(permit);
    });
}

/// A described link with its media downloaded and ready to store.
struct Ready {
    url: String,
    provider: Provider,
    unfurled: Unfurled,
    /// The 400 px legacy thumbnail, uploaded by `store`.
    thumbnail: Option<EmbedThumbnail>,
    /// Stored media of a provider embed.
    items: Vec<download::StoredMedia>,
    author_icon: Option<String>,
    /// Objects already uploaded for this embed (key, size): accounted when it
    /// is stored, deleted if it is not.
    objects: Vec<(String, u64)>,
}

impl Ready {
    /// Whether there is anything worth an embed (given whether the thumbnail was stored).
    fn worth_storing(&self, thumbnail_stored: bool) -> bool {
        self.unfurled.has_text() || thumbnail_stored || !self.items.is_empty()
    }

    /// `message_embeds.kind`.
    fn kind(&self, thumbnail_stored: bool) -> &'static str {
        if self.provider == Provider::Generic {
            return if !self.unfurled.has_text() && thumbnail_stored { "image" } else { "link" };
        }
        if self.items.iter().any(|m| matches!(m.kind, AssetKind::Video | AssetKind::Gifv)) { "video" } else { "rich" }
    }
}

/// Unfurls every URL, stores the embeds that have something to show, and
/// publishes `message_updated`. Awaitable so tests can run it directly.
pub async fn run(state: &AppState, job: &Job, urls: Vec<String>) {
    let unfurled: Vec<Ready> = join_all(urls.into_iter().map(|u| unfurl(state, job, u))).await.into_iter().flatten().collect();
    if unfurled.is_empty() {
        return;
    }
    // The message may have been deleted (or edited again) while we fetched.
    let still_there = state
        .session
        .execute_unpaged(
            "SELECT content FROM messages WHERE channel_id = ? AND bucket = ? AND id = ?",
            (id_to_db(job.channel_id), bucket_of(job.message_id), id_to_db(job.message_id)),
        )
        .await
        .ok()
        .and_then(|r| r.into_rows_result().ok())
        .and_then(|r| r.maybe_first_row::<(Option<String>,)>().ok().flatten())
        .is_some_and(|(content,)| content.as_deref() == Some(job.content.as_str()));
    if !still_there {
        discard_ready(state, unfurled).await;
        return;
    }
    if let Err(e) = store(state, job, unfurled).await {
        tracing::warn!(message_id = job.message_id, error = %e.message.as_deref().unwrap_or("?"), "storing embeds failed");
        return;
    }
    if let Err(e) = publish_message_updated(state, job.channel_id, job.message_id).await {
        tracing::warn!(message_id = job.message_id, error = %e.message.as_deref().unwrap_or("?"), "publishing message_updated failed");
    }
}

/// Deletes the objects downloaded for embeds that will not be stored.
async fn discard_ready(state: &AppState, unfurled: Vec<Ready>) {
    for item in unfurled {
        discard(state, keys_of(&item.objects)).await;
    }
}

fn keys_of(objects: &[(String, u64)]) -> Vec<String> {
    objects.iter().map(|(k, _)| k.clone()).collect()
}

/// Deletes uploaded objects that no row will point at.
async fn discard(state: &AppState, keys: Vec<String>) {
    if let Some(storage) = &state.storage {
        for key in keys {
            let _ = storage.delete(&key).await;
        }
    }
}

/// Keys stored for rows that were never written (or were all dropped) are
/// orphans: forget their accounting and delete them.
async fn undo(state: &AppState, keys: Vec<String>) {
    for key in &keys {
        media::delete_record(state, key).await;
    }
    discard(state, keys).await;
}

/// Records the accounting rows of `objects`; on failure undoes them (and the
/// objects) and returns `false`.
async fn account(state: &AppState, owner: Owner<'_>, meta: media::Meta<'_>, objects: &[(String, u64)]) -> bool {
    let mut recorded: Vec<&str> = Vec::new();
    for (key, size) in objects {
        if media::record_with(state, key, owner, *size, meta).await.is_err() {
            for key in &recorded {
                media::delete_record(state, key).await;
            }
            return false;
        }
        recorded.push(key);
    }
    true
}

/// Stores thumbnails and writes the embed rows (batches in the message's
/// partition: the media rows first, then the embeds that show them). A
/// failure undoes what was stored.
async fn store(state: &AppState, job: &Job, unfurled: Vec<Ready>) -> Result<(), connectrpc::ConnectError> {
    let owner = if job.guild_id != 0 {
        Owner { kind: "guild", id: job.guild_id, guild_id: Some(job.guild_id), label: "embed" }
    } else {
        Owner { kind: "user", id: job.author_id, guild_id: None, label: "embed" }
    };
    let meta = media::Meta {
        channel_id: Some(job.channel_id),
        message_id: Some(job.message_id),
        user_id: Some(job.author_id),
        ..Default::default()
    };
    // Every key stored and accounted so far (undone on failure).
    let mut stored_keys: Vec<String> = Vec::new();
    // (embed, thumbnail key)
    let mut rows: Vec<(Ready, Option<String>)> = Vec::new();
    for mut item in unfurled {
        let mut key = None;
        if let (Some(thumb), Some(storage)) = (&item.thumbnail, &state.storage) {
            let candidate = media::embed_thumbnail_key(job.channel_id);
            match storage.put(&candidate, thumb.bytes.clone(), "image/webp").await {
                Ok(()) => {
                    item.objects.push((candidate.clone(), thumb.bytes.len() as u64));
                    key = Some(candidate);
                }
                Err(e) => tracing::warn!(error = %format!("{e:#}"), "storing embed thumbnail failed"),
            }
        }
        // An embed whose only content was media that could not be stored has nothing left.
        if !item.worth_storing(key.is_some()) || !account(state, owner, meta, &item.objects).await {
            discard(state, keys_of(&item.objects)).await;
            continue;
        }
        stored_keys.extend(item.objects.iter().map(|(k, _)| k.clone()));
        rows.push((item, key));
    }
    if rows.is_empty() {
        return Ok(());
    }
    let mut media_batch = scylla::statement::batch::Batch::new(scylla::statement::batch::BatchType::Logged);
    let mut media_values = Vec::new();
    for (position, (item, _)) in rows.iter().enumerate() {
        for (idx, m) in item.items.iter().enumerate() {
            media_batch.append_statement(
                "INSERT INTO message_embed_media (channel_id, bucket, message_id, position, idx, kind, file, poster, \
                 width, height, duration_ms, size) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            );
            media_values.push((
                id_to_db(job.channel_id),
                bucket_of(job.message_id),
                id_to_db(job.message_id),
                position as i32,
                idx as i32,
                media_kind_name(m.kind),
                m.file.as_str(),
                m.poster.as_deref(),
                m.width as i32,
                m.height as i32,
                m.duration_ms.map(|d| d as i32),
                m.size as i64,
            ));
        }
    }
    if !media_values.is_empty()
        && let Err(e) = state.session.batch(&media_batch, media_values).await
    {
        undo(state, stored_keys).await;
        return Err(internal(e));
    }

    let mut batch = scylla::statement::batch::Batch::new(scylla::statement::batch::BatchType::Logged);
    let mut values = Vec::with_capacity(rows.len());
    for (position, (item, key)) in rows.iter().enumerate() {
        batch.append_statement(
            "INSERT INTO message_embeds (channel_id, bucket, message_id, position, url, title, description, \
             site_name, thumbnail, thumbnail_width, thumbnail_height, kind, provider, author_name, author_url, \
             author_icon, published_at) VALUES (:channel_id, :bucket, :message_id, :position, :url, :title, \
             :description, :site_name, :thumbnail, :thumbnail_width, :thumbnail_height, :kind, :provider, \
             :author_name, :author_url, :author_icon, :published_at)",
        );
        let thumb = key.as_ref().and(item.thumbnail.as_ref());
        let unfurled = &item.unfurled;
        values.push(EmbedInsert {
            channel_id: id_to_db(job.channel_id),
            bucket: bucket_of(job.message_id),
            message_id: id_to_db(job.message_id),
            position: position as i32,
            url: item.url.as_str(),
            title: unfurled.title.as_str(),
            description: unfurled.description.as_str(),
            site_name: unfurled.site_name.as_str(),
            thumbnail: key.as_deref(),
            thumbnail_width: thumb.map(|t| t.width as i32),
            thumbnail_height: thumb.map(|t| t.height as i32),
            kind: item.kind(key.is_some()),
            provider: Some(item.provider.name()).filter(|p| !p.is_empty()),
            author_name: Some(unfurled.author.as_str()).filter(|a| !a.is_empty()),
            author_url: unfurled.author_url.as_deref(),
            author_icon: item.author_icon.as_deref(),
            published_at: unfurled.published_at.and_then(|s| DateTime::<Utc>::from_timestamp(s, 0)),
        });
    }
    if let Err(e) = state.session.batch(&batch, values).await {
        undo(state, stored_keys).await;
        for (position, (item, _)) in rows.iter().enumerate() {
            for idx in 0..item.items.len() {
                let _ = state
                    .session
                    .execute_unpaged(
                        "DELETE FROM message_embed_media WHERE channel_id = ? AND bucket = ? AND message_id = ? AND position = ? AND idx = ?",
                        (id_to_db(job.channel_id), bucket_of(job.message_id), id_to_db(job.message_id), position as i32, idx as i32),
                    )
                    .await;
            }
        }
        return Err(internal(e));
    }
    Ok(())
}

/// The values of one `message_embeds` insert (more columns than a tuple can bind).
#[derive(scylla::SerializeRow)]
struct EmbedInsert<'a> {
    channel_id: i64,
    bucket: i32,
    message_id: i64,
    position: i32,
    url: &'a str,
    title: &'a str,
    description: &'a str,
    site_name: &'a str,
    thumbnail: Option<&'a str>,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
    kind: &'static str,
    provider: Option<&'static str>,
    author_name: Option<&'a str>,
    author_url: Option<&'a str>,
    author_icon: Option<&'a str>,
    published_at: Option<DateTime<Utc>>,
}

fn media_kind_name(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Image => "image",
        AssetKind::Video => "video",
        AssetKind::Gifv => "gifv",
    }
}

/// Unfurls one URL: provider (or cache) for the description, then the media
/// step. `None` if there is nothing useful to show.
async fn unfurl(state: &AppState, job: &Job, url: String) -> Option<Ready> {
    let parsed = Url::parse(&url).ok()?;
    let provider = Provider::for_url(&parsed);
    let fetcher = state.embeds.fetcher();
    let mirror_fetcher = state.embeds.mirror_fetcher();
    let mut unfurled = match cache_get(state, &url).await {
        Some(Some(unfurled)) => unfurled,
        Some(None) => return None,
        None => {
            let ctx = Ctx {
                fetcher: &fetcher,
                mirror_fetcher: &mirror_fetcher,
                mirrors: &state.embeds.mirrors,
                html_cap: state.embeds.html_cap,
                image_cap: state.embeds.image_cap,
                provider_timeout: state.embeds.provider_timeout,
                attempt_timeout: state.embeds.attempt_timeout,
                max_video_bytes: state.embeds.max_video_bytes,
                max_media: state.embeds.max_media,
            };
            match provider.unfurl(&ctx, &parsed).await {
                Ok(found) => {
                    cache_put(state, &url, found.as_ref()).await;
                    found?
                }
                Err(e) => {
                    tracing::debug!(%url, error = %e, "unfurl failed");
                    return None;
                }
            }
        }
    };
    let mut ready = Ready { url, provider, thumbnail: None, items: Vec::new(), author_icon: None, objects: Vec::new(), unfurled: Unfurled::default() };
    if let Some(storage) = &state.storage {
        if provider == Provider::Generic {
            ready.thumbnail = thumbnail_of(&fetcher, &mut unfurled, state.embeds.image_cap).await;
        } else {
            let limits = download::Limits {
                image_cap: state.embeds.media_image_cap,
                max_video_bytes: state.embeds.max_video_bytes,
                timeout: state.embeds.media_timeout,
                max_media: state.embeds.max_media,
            };
            let fetched = download::fetch_media(&mirror_fetcher, storage, job.channel_id, &unfurled, &limits).await;
            ready.thumbnail = fetched.thumbnail;
            ready.items = fetched.items;
            ready.author_icon = fetched.author_icon;
            ready.objects = fetched.objects;
        }
    }
    ready.unfurled = unfurled;
    if ready.worth_storing(ready.thumbnail.is_some()) {
        Some(ready)
    } else {
        discard(state, keys_of(&ready.objects)).await;
        None
    }
}

/// The media step of generic links: the first image becomes the embed's
/// thumbnail (downloaded, decoded, fit within 400x400, re-encoded as WebP).
async fn thumbnail_of(fetcher: &Fetcher, unfurled: &mut Unfurled, cap: usize) -> Option<EmbedThumbnail> {
    let first = unfurled.media.iter().position(|m| m.kind == AssetKind::Image)?;
    // Only `media[0]` can have been prefetched.
    if first == 0
        && let Some(bytes) = unfurled.prefetched.take()
    {
        return media::embed_thumbnail(bytes).await;
    }
    let url = Url::parse(&unfurled.media[first].url).ok()?;
    let result = fetcher
        .within(async {
            let reply = fetcher.get(&url).await?;
            if !reply.content_type.starts_with("image/") {
                return Err(fetch::FetchError::NotAllowedType(reply.content_type));
            }
            reply.read_capped(cap).await
        })
        .await;
    match result {
        Ok(bytes) => media::embed_thumbnail(bytes).await,
        Err(e) => {
            tracing::debug!(image_url = %url, error = %e, "embed media download failed");
            None
        }
    }
}

type CacheRow = (Option<bool>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>);

/// `None`: not cached. `Some(None)`: cached as "nothing useful".
async fn cache_get(state: &AppState, url: &str) -> Option<Option<Unfurled>> {
    let row = state
        .session
        .execute_unpaged(
            "SELECT found, title, description, site_name, author, media, author_url, author_icon, published_at \
             FROM embed_cache WHERE url = ?",
            (format!("{}{url}", state.embeds.cache_namespace),),
        )
        .await
        .ok()?
        .into_rows_result()
        .ok()?
        .maybe_first_row::<CacheRow>()
        .ok()??;
    let (found, title, description, site_name, author, media, author_url, author_icon, published_at) = row;
    if found != Some(true) {
        return Some(None);
    }
    Some(Some(Unfurled {
        title: title.unwrap_or_default(),
        description: description.unwrap_or_default(),
        site_name: site_name.unwrap_or_default(),
        author: author.unwrap_or_default(),
        author_url,
        author_icon,
        published_at,
        media: media.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or_default(),
        prefetched: None,
    }))
}

/// Best effort: a failed cache write only costs a refetch later.
async fn cache_put(state: &AppState, url: &str, found: Option<&Unfurled>) {
    let ttl = if found.is_some() { state.embeds.cache_ttl } else { state.embeds.negative_cache_ttl };
    let ttl = i32::try_from(ttl.as_secs()).unwrap_or(i32::MAX).max(1);
    let empty = Unfurled::default();
    let f = found.unwrap_or(&empty);
    let media = serde_json::to_string(&f.media).unwrap_or_else(|_| "[]".to_owned());
    let result = state
        .session
        .execute_unpaged(
            "INSERT INTO embed_cache (url, found, title, description, site_name, author, media, author_url, author_icon, \
             published_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) USING TTL ?",
            (format!("{}{url}", state.embeds.cache_namespace), found.is_some(), &f.title, &f.description, &f.site_name, &f.author, media, &f.author_url, &f.author_icon, f.published_at, ttl),
        )
        .await;
    if let Err(e) = result {
        tracing::warn!(error = %e, "writing embed cache failed");
    }
}

/// Embed rows as read back by `load_embeds`.
pub type EmbedRow = (
    i64,
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
);

/// The columns `EmbedRow` is read from.
pub const EMBED_COLUMNS: &str = "message_id, position, url, title, description, site_name, thumbnail, thumbnail_width, \
     thumbnail_height, kind, provider, author_name, author_url, author_icon, published_at";

/// Media rows as read back by `load_embeds`: `(message_id, position, idx, kind, file, poster, width, height, duration_ms, size)`.
pub type MediaRow = (i64, i32, i32, Option<String>, Option<String>, Option<String>, Option<i32>, Option<i32>, Option<i32>, Option<i64>);

/// The columns `MediaRow` is read from.
pub const MEDIA_COLUMNS: &str = "message_id, position, idx, kind, file, poster, width, height, duration_ms, size";

/// `(message_id, position, embed)`; the media are attached by [`attach_media`].
pub fn row_to_embed(row: EmbedRow) -> (u64, i32, Embed) {
    let (message_id, position, url, title, description, site_name, thumbnail, width, height, kind, provider, author_name, author_url, author_icon, published_at) = row;
    let thumbnail = thumbnail.filter(|t| !t.is_empty());
    let kind = match kind.as_deref() {
        Some("image") => EmbedKind::EMBED_KIND_IMAGE,
        Some("video") => EmbedKind::EMBED_KIND_VIDEO,
        Some("rich") => EmbedKind::EMBED_KIND_RICH,
        _ => EmbedKind::EMBED_KIND_LINK,
    };
    (
        crate::convert::id_from_db(message_id),
        position,
        Embed {
            url: url.unwrap_or_default(),
            title: title.unwrap_or_default(),
            description: description.unwrap_or_default(),
            site_name: site_name.unwrap_or_default(),
            thumbnail_width: thumbnail.as_ref().and(width).map(|v| v as u32),
            thumbnail_height: thumbnail.as_ref().and(height).map(|v| v as u32),
            thumbnail,
            kind: kind.into(),
            provider: provider.unwrap_or_default(),
            author_name: author_name.unwrap_or_default(),
            author_url: author_url.filter(|u| !u.is_empty()),
            author_icon: author_icon.filter(|u| !u.is_empty()),
            published_at: published_at.map(|t| crate::convert::datetime_to_proto(t).into()).unwrap_or_default(),
            ..Default::default()
        },
    )
}

/// One stored media row as the API's `EmbedMedia` (`None` for an unknown kind or a removed file).
pub fn row_to_media(row: MediaRow) -> Option<(u64, i32, i32, EmbedMedia)> {
    let (message_id, position, idx, kind, file, poster, width, height, duration_ms, size) = row;
    let kind = match kind.as_deref()? {
        "image" => EmbedMediaKind::EMBED_MEDIA_KIND_IMAGE,
        "video" => EmbedMediaKind::EMBED_MEDIA_KIND_VIDEO,
        "gifv" => EmbedMediaKind::EMBED_MEDIA_KIND_GIFV,
        _ => return None,
    };
    let file = file.filter(|f| !f.is_empty())?;
    Some((
        crate::convert::id_from_db(message_id),
        position,
        idx,
        EmbedMedia {
            kind: kind.into(),
            file,
            poster: poster.filter(|p| !p.is_empty()),
            width: width.unwrap_or(0).max(0) as u32,
            height: height.unwrap_or(0).max(0) as u32,
            duration_ms: duration_ms.map(|d| d.max(0) as u32),
            size: size.unwrap_or(0).max(0) as u64,
            ..Default::default()
        },
    ))
}
