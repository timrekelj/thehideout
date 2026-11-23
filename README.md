# thehideout
A Discord-style chat app for me and my friends, built to learn handling lots of data

The server also serves a public landing page at `GET /` (downloads, features; embedded from `server/assets/landing/`). To enable the "Open in browser" button, set `WEB_APP_URL` in `server/assets/landing/site.js`.

## Development

Start ScyllaDB (single dev node), a local S3-compatible object store
(SeaweedFS) for avatars and guild icons, and Mailpit (catches all outgoing
email: SMTP on 1025, web inbox at <http://127.0.0.1:8025>), and LiveKit (the
voice channel media server: ws on 7880, TCP 7881, UDP 50000-50100). A one-shot
`seaweedfs-init` container creates the `thehideout-media` bucket; all services
listen on 127.0.0.1 only:

```sh
docker compose up -d
```

Run the server (applies schema migrations on startup, listens on `0.0.0.0:8080`):

```sh
cd server && cargo run
```

The dev Scylla runs with 2 cores and 2 GB (`--smp 2 --memory 2G` in
`docker-compose.yml`); the integration tests are DDL- and write-heavy and slow
down a lot on one core. After changing these flags run `docker compose up -d scylla`
(the data volume is kept). The tests share one migrated keyspace
(`test_shared_<hash>`), so run only one `cargo test` against the same Scylla at a time.

Run the tests (unit tests plus integration tests against the Scylla and S3
containers, so `docker compose up -d` must be running; the media tests fail
with a clear message if the object store is unreachable):

```sh
cd server && cargo test
```

Requires `buf` on `PATH` for protobuf codegen.

## Configuration

Settings are environment variables. Copy `.env.example` to `.env` (gitignored)
and edit it; the server loads the repo-root `.env`, and real environment
variables override it. `.env.example` documents every variable and its default.
Never commit `.env`.

## Running in production

Operational settings (all in `.env.example`):

| Variable | Default | Meaning |
| --- | --- | --- |
| `THEHIDEOUT_TRUSTED_PROXIES` | empty | Comma-separated IPs/CIDRs of reverse proxies whose `X-Forwarded-For` / `X-Real-IP` are believed, e.g. `127.0.0.1/32,172.16.0.0/12`. Empty = the socket peer is the client. |
| `THEHIDEOUT_LOG_FORMAT` | `text` | `text` or `json`. |
| `THEHIDEOUT_MAX_STREAMS_PER_USER` | `8` | Open gateway streams per user; more get `RESOURCE_EXHAUSTED`. |
| `THEHIDEOUT_MAX_STREAMS` | `5000` | Open gateway streams per instance. |
| `THEHIDEOUT_SCYLLA_USERNAME` / `THEHIDEOUT_SCYLLA_PASSWORD` | unset | ScyllaDB login (both or neither). |
| `THEHIDEOUT_SCYLLA_TLS_CA` | unset | PEM CA file; enables TLS to Scylla (node certs must name the host/IP in `THEHIDEOUT_SCYLLA_NODES`). |
| `THEHIDEOUT_SCYLLA_ALLOW_NO_AUTH` | unset | `1` allows starting a non-dev deployment without Scylla credentials (Scylla on a private network). |
| `THEHIDEOUT_SCYLLA_REPLICATION` | `SimpleStrategy:1` | Replication when the keyspace is created: `SimpleStrategy:<n>` or `NetworkTopologyStrategy:dc1=3,dc2=3`. An existing keyspace is never altered; a mismatch logs a warning. |

- **Client IPs.** Rate limits and the access log use the client address. Behind a proxy, set
  `THEHIDEOUT_TRUSTED_PROXIES` to the proxy's address range; from a trusted peer the client is the
  right-most `X-Forwarded-For` entry that is not itself trusted (falling back to `X-Real-IP`).
  Headers from any other peer are ignored. List only proxies you control.
- **Logs.** Each request logs method, path (no query string), status, latency and client IP, never
  headers. `RUST_LOG` filters as usual.
- **Health.** `GET /health` is liveness (always `ok`). `GET /ready` returns 200 when ScyllaDB and
  object storage each answer within 2 s, else 503 with `{"status":"unavailable","failed":["scylla"|"s3"]}`.
  No auth.
- **Shutdown.** SIGTERM and SIGINT stop accepting connections, end every open gateway stream (clients
  see `UNAVAILABLE` and reconnect) and background sweepers, then wait for in-flight requests and queued
  email for at most 10 s before exiting.
- **Timeouts.** Requests time out after 30 s (408), except uploads and media reads (10 min) and the
  gateway stream (none). Clients have 10 s to send request headers.
- **Email.** Mail is sent in the background. Transient failures (network errors, 5xx, 429, SMTP 4xx)
  are retried after 2 s, 10 s and 30 s (up to 4 attempts). The final failure is logged with the template
  kind and the recipient's domain only.
- **Migrations.** Instances starting together take turns: migrations run under a lock row in
  `schema_lock` (`INSERT ... IF NOT EXISTS USING TTL 300`, refreshed per migration, released at the
  end; a crashed holder's lock expires after 5 minutes). A version is recorded in `schema_migrations`
  only after all of its statements succeed, and "already exists" errors left by an interrupted run are
  skipped so the version completes on the next start.
- **ScyllaDB hardening.** Enabling authentication also means raising the `system_auth` keyspace
  replication to match your cluster. A non-dev `THEHIDEOUT_PUBLIC_URL` refuses to start without Scylla
  credentials unless `THEHIDEOUT_SCYLLA_ALLOW_NO_AUTH=1`.

## Accounts, invites and email

Accounts are invite-only; there is no open sign-up.

- **First admin.** On an install with no accounts, the server creates a
  site-admin invite for `THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL` at startup, emails
  it and logs its code and link (`created bootstrap admin invite`). Without the
  variable it logs a reminder instead. A still-pending bootstrap invite is not
  re-created on restart.
- **CLI.** `thehideout-server invite <email> [--admin]` (from `server/`:
  `cargo run -- invite alice@example.com --admin`) creates and emails an invite
  at any time and prints its code and link, then exits.
- **Dashboard.** Site admins invite anyone (optionally pre-joining guilds and
  making them admin); guild members with the `INVITE_PEOPLE` role permission
  invite people into that guild. Guild invite codes only let existing accounts
  join more guilds.
- `THEHIDEOUT_ADMINS` still works as a second way to make admins: listed
  usernames are promoted at startup and when the account is created.

Sign-in methods: username or email + password, emailed one-time codes, an
authenticator app (TOTP) with recovery codes as a second factor, and passkeys
(WebAuthn; the relying party is the host of `THEHIDEOUT_PUBLIC_URL`, which must
be a domain name: `localhost` in development). Second-factor challenges and
passkey ceremonies are kept in memory, so with several server instances a
sign-in must finish on the instance that started it.

Email goes through Resend when `THEHIDEOUT_RESEND_API_KEY` is set, otherwise
over plain SMTP to `THEHIDEOUT_SMTP_URL` (Mailpit in development). Links in
emails use `THEHIDEOUT_PUBLIC_URL`.

Two guards keep a development setup from sending real mail:

- While the host of `THEHIDEOUT_PUBLIC_URL` is `localhost`, `*.localhost`,
  `*.test`, `*.example` or an IP address, the Resend key is ignored and mail
  goes over SMTP (Mailpit); the server logs a warning at startup. Set
  `THEHIDEOUT_EMAIL_FORCE_RESEND=true` to use Resend anyway.
- Mail to reserved test domains (`example.com`, `example.net`, `example.org`,
  `*.test`, `*.example`, `*.invalid`, `localhost`) is never sent through
  Resend: it is logged and dropped, and the flow carries on as if it was sent.

## Moderation and guild structure

Message edit/delete, kick, ban, leaving, deleting channels and guilds, invite-code
rotation and ownership transfer (`GuildService`, `MessageService`; permissions
MANAGE_MESSAGES=2048, KICK_MEMBERS=4096, BAN_MEMBERS=8192). The rules, events and
audit-log actions are in `proto/README.md`. Operationally:

- Deletions make the thing unreachable at once (message, `channels` / `guilds` and
  membership rows) and clean up what hangs off it afterwards: reactions, embeds,
  attachment rows and their S3 objects per message; a channel's message partitions in the
  background after `DeleteChannel`; a deleted guild's roles, emojis, bans, sections,
  counters and every file in its size index in the background after `DeleteGuild`.
  Purges are best effort and are not resumed after a crash: leftovers are unreachable
  rows or objects that the admin file list still shows and can delete.
- Messages: `SendMessage` / `EditMessage` share a per-user token bucket (burst 10, then
  1/s; `RESOURCE_EXHAUSTED` with `retry-after`), and a send retried with the same nonce
  within 60 seconds is deduplicated (`message_nonces`, written with a TTL).
- Migration `0013_moderation.cql` adds `guild_bans` and `message_nonces`.

## Account lifecycle, legal pages and client versions

- **Deleting an account** (`AuthService.DeleteAccount`) anonymizes it: messages stay and
  show as "Deleted user"; the username, email, avatar, password, authenticator, passkeys,
  recovery codes, sessions, DM membership, uploads and attachments, reactions and pending
  invites are removed, and the username is free again. The owner must transfer or delete
  their servers first (`FAILED_PRECONDITION` naming them). The account is unusable at once;
  the cleanup is a list of idempotent steps (`lifecycle.rs`) run in the background. A crash
  leaves the account in `deleting` state in `account_deletions` and the sweep at startup
  (and every 10 minutes) finishes it. Messages are found by scanning the channels of the
  user's guilds, of guilds they left since migration 0014, and of their DMs; messages in a
  guild left before that migration are not found.
- **Data export** (`ExportData`): a JSON file generated in the background into object
  storage under `exports/`, emailed as `{PUBLIC_URL}/export/{token}` (unguessable token,
  7 days; the sweeper deletes the file). One per user per 24 hours; needs an email address.
- **Admin**: `SuspendUser` / `UnsuspendUser` (revokes sessions; sign-in fails with
  PERMISSION_DENIED "Your account is suspended" and an `AccountSuspended` detail) and
  `AdminDeleteUser` (same deletion; refused for site admins; `force_transfer` hands each
  owned server to its highest-role member, ties the longest member, and deletes servers
  where the user is alone). All are audit-logged (`user.suspend`, `user.unsuspend`,
  `account.delete`).
- **Sessions** slide: using a session older than a day rewrites its rows with a fresh
  30-day TTL (at most once a day per session), so active users are not signed out.
- **Legal pages**: `GET /privacy` and `GET /terms` (version `policy::LEGAL_VERSION`,
  minimum age 16), linked from the invite page and every email footer. `THEHIDEOUT_CONTACT_EMAIL`
  is required on non-development hosts (the server refuses to start without it);
  `THEHIDEOUT_OPERATOR_NAME` is optional. `AcceptInvite` requires
  `accepted_terms_version` = the current version and `age_confirmed`; `GetMe` reports
  `terms_update_required` for users who have not accepted the current one (`AcceptTerms`).
  The notices state 30-day log and backup retention: keep your operations in line with that.
- **Minimum client version**: `THEHIDEOUT_MIN_CLIENT_VERSION` (and `_DESKTOP`, `_IOS`,
  `_ANDROID`) refuses clients whose `x-thehideout-client: <platform>/<version> ...` header is
  older with `FAILED_PRECONDITION` "client too old". Requests without the header, and
  `/health`, `/ready`, `/privacy`, `/terms`, `/invite` and `/media`, pass.
- Migration `0014_account_lifecycle.cql` adds the deletion, suspension and terms columns,
  `account_deletions`, `user_left_guilds`, `export_requests`, `data_exports` and
  `data_exports_index`, and `sessions.refreshed_at`.

## Voice (LiveKit)

Voice channels (`Channel.kind = CHANNEL_KIND_VOICE`, chosen at creation, never
changed; posting a message in one is `FAILED_PRECONDITION`) carry audio, camera
and screen share through a self-hosted [LiveKit](https://livekit.io) SFU. The
server is the authority on who may join and who is in which channel; LiveKit
only moves media. The `livekit` service in `docker-compose.yml` runs
`livekit/livekit-server` (pinned) with the dev key `devkey` / `secret`, bound to
127.0.0.1, and posts webhooks to `host.docker.internal:8080/livekit/webhook`
(the server on the host; `host-gateway` makes that name work on Linux too).
`curl http://127.0.0.1:7880` answers `OK` when it is up. LiveKit logs "secret is
too short" for the dev secret; it is harmless on loopback.

| Variable | Dev default | Meaning |
| --- | --- | --- |
| `LIVEKIT_URL` | `ws://127.0.0.1:7880` | URL clients connect to |
| `LIVEKIT_API_URL` | `LIVEKIT_URL` with ws -> http | URL the server calls LiveKit's API on |
| `LIVEKIT_API_KEY` | `devkey` | API key (also signs join tokens and verifies webhooks) |
| `LIVEKIT_API_SECRET` | `secret` | API secret |

Unset (all of URL, key and secret empty) = voice off: voice channels still list,
`JoinVoice` answers `UNAVAILABLE` "voice is not configured on this server" and
the webhook endpoint answers 503. Setting only some of them is a startup error, as
is a `LIVEKIT_API_URL` that is not `http(s)://`. The dev key `devkey`, the dev secret
`secret` and any secret shorter than 32 characters are public or weak, so the server
refuses to start with them unless `THEHIDEOUT_PUBLIC_URL` is a development host
(`localhost`, `*.test`, `*.example` or an IP address), the same rule as for the dev
S3 keys; set a real key and a secret of 32+ characters (or unset `LIVEKIT_*`).

- **Names.** Room `voice-<channel_id>`, participant identity = user id (decimal),
  participant name = username.
- **Join.** `VoiceService.JoinVoice(channel_id)` needs guild membership
  (`NOT_FOUND` otherwise) and `CONNECT`, on a voice channel. It returns
  `{ url, token, channel_id }`; the token (about 10 minutes to connect) allows
  joining that room and subscribing, and publishing only if the caller has
  `SPEAK`, limited to `microphone` plus `camera`, `screen_share` and
  `screen_share_audio` with `VIDEO` (no microphone while server-muted). A user is in
  one voice channel at a time: joining another closes their LiveKit connection to
  the old room. Joining the channel you are in reissues a token and keeps the state.
- **Permissions** (`types.proto`): `CONNECT`=64, `SPEAK`=128, `VIDEO`=256,
  `MUTE_MEMBERS`=512, `MOVE_MEMBERS`=1024 (moving includes disconnecting). There
  is no @everyone role, so `CONNECT`, `SPEAK` and `VIDEO` are a baseline every
  member has regardless of roles. `CreateRole` / `UpdateRole` accept those bits but they
  add nothing, and nothing can revoke them: listen-only members need an everyone-level
  role, which is not available yet (the desktop role editor offers no switch for them).
  A change of a member's roles or of a role's permissions during a call is pushed to
  LiveKit at once (`UpdateParticipant`), so it applies live when such a limit exists.
  `MUTE_MEMBERS` / `MOVE_MEMBERS` are
  held by nobody by default: the owner and `ADMINISTRATOR` imply them, and roles
  can grant them under the usual "only grant what you hold" rule.
- **Moderation.** `ServerMuteMember`, `MoveMember` and `DisconnectMember` need the
  permission and a top role strictly above the target's (the owner outranks all;
  nobody but the owner can act on the owner; you may move or disconnect yourself
  but not server-mute yourself). The target must be in a voice channel of that guild.
  - *Mute* is enforced, not advisory: the server updates the participant's LiveKit
    permissions (RoomService `UpdateParticipant`) to drop the microphone source, which
    makes LiveKit unpublish a live microphone track. If the member has not connected yet
    it is applied when LiveKit reports `participant_joined`. The mute is kept per
    (guild, user), apart from the voice state: it survives a move, leaving and rejoining,
    and joining another guild's voice and coming back, and ends only with an unmute (the
    target must be in voice for that) or the member leaving the guild. Like the voice
    state it lives in memory, so a restart clears it.
  - *Disconnect* removes the voice state and the LiveKit participant
    (`RemoveParticipant`).
  - *Move* switches the voice state to the new channel, removes the old participant,
    and sends the target a gateway `voice_moved { guild_id, channel_id }`; their client
    then calls `JoinVoice(channel_id)` for a fresh token. A target that does not
    reconnect within about 30 seconds is dropped.
- **Voice state** (`VoiceState`: guild, channel, user, `self_mute`, `self_deaf`,
  `server_mute`, `self_video`, `self_stream`, `joined_at`) lives in server memory only
  (single node, lost on restart; clients and LiveKit then reconcile through the
  webhooks below and by calling `JoinVoice` again). Changes are published as
  `voice_state_update { guild_id, user_id, state? }` (state unset = left) to guild
  members. A new gateway stream sends `voice_snapshot { guild_ids, states }` right after
  its first heartbeat when anyone is in voice in the user's guilds (nothing when nobody
  is, so clients clear voice states when a stream opens and then apply a snapshot if
  one comes); joining a guild sends one for that guild after `guild_added`.
  `UpdateVoiceState` changes the caller's own flags (video / stream need `VIDEO`).
- **Staying consistent.** LiveKit's webhooks (`POST /livekit/webhook`, verified with
  the `Authorization` JWT, which signs the SHA-256 of the body) drive it:
  `participant_joined` confirms a join and records the participant SID (and disconnects
  a participant we have no join for, e.g. a stale token), `participant_left` and
  `room_finished` clear the state. A user who joined but is not confirmed within 30
  seconds is dropped and their LiveKit participant removed; the server then logs a
  warning (at most every 5 minutes) that the webhook may not be reaching it. Joining the
  channel you are in again keeps the confirmed connection and does not restart that
  timer. Deleting a channel (`DeleteChannel`/`DeleteGuild`), or a member leaving
  or being kicked or banned, calls `voice::on_channel_deleted` /
  `voice::on_member_removed`: the user leaves voice and their LiveKit participant is
  removed. A reconnect of the same identity makes LiveKit send a
  late `participant_left` for the old connection: it is ignored when its SID is not the
  one recorded from `participant_joined`, so it does not drop the fresh rejoin.
- Production: run LiveKit with its own config (real keys of 32+ characters, TLS in front
  of the signalling port, `rtc.use_external_ip: true` or a `node_ip`, the UDP range open),
  and point its `webhook.urls` at the server's `/livekit/webhook`.

## Push notifications (APNs and FCM)

The mobile apps get notified of DMs, @mentions and (per their settings) other messages
through Apple's push service (iOS) and Firebase Cloud Messaging (Android). Both
providers are optional: with neither configured `RegisterDevice` still stores tokens and
nothing is sent (the dispatcher logs at debug level). A provider takes effect when the
server starts.

- **APIs** (`proto/thehideout/v1/notification_service.proto`): `RegisterDevice`,
  `UnregisterDevice`, `SetClientState(foreground)`, `GetNotificationSettings`,
  `UpdateNotificationSettings`; the gateway event `notification_settings_updated` mirrors a
  settings change to the user's other sessions.
- **Who is notified.** For a new message: the other DM participant, or the guild members
  (every member can see every channel); never the author. A recipient needs a registered
  device whose session still exists. Their effective level decides: the channel override,
  else the guild override, else the global default (`guild_default`, or `dms_enabled` for DMs).
  `all` = every message, `mentions` = the message `@mentions` them (`@username`, parsed like
  the clients do; not in code spans; there is no @everyone or role mention), `nothing` = never.
  `muted_until` in the future means nothing (a muted guild still lets a channel with its own
  level through). Edits, reactions and voice never push; being added to a guild
  (`guild_added`) does, unless the guild default is `nothing`.
- **Foreground rule.** A user with a live gateway stream that is not marked backgrounded
  is not pushed (they see the message live). A stream counts as foreground until the app
  calls `SetClientState(foreground = false)`; mobile apps call it when they go to the
  background and with `true` when they return.
- **Coalescing.** At most one push per channel and recipient per 5 seconds. The first goes
  out at once; messages arriving in the window become one trailing push (the latest) with the
  same collapse id (the channel id; `thread-id` on iOS, collapse key and tag on Android).
  Sends run concurrently (32 at a time), with retries (0.5 s, 1 s, 2 s) on 429 and 5xx and
  nothing on the message path.
- **Payload.** Title `Ana` (DM) or `Ana in #general · Hideout`; body = the message as plain
  text (markdown removed, 180 characters) or `New message` when the user turned previews off
  (`show_previews`); `Sent an attachment` for text-less messages. Data: `kind`
  (`message`, `dm`, `mention`, `guild_added`), `guild_id`, `channel_id`, `message_id`. Android
  channels: `mentions` (DMs and mentions) and `messages`. iOS sets `mutable-content: 1`
  so a notification service extension can be added later. **No badge count**: the server
  keeps no read state, so there is nothing to count.
- **Devices.** A device belongs to the session that registered it (one per platform per
  session, at most 10 per user); sign-out, session revocation, suspension and account
  deletion remove it, expiry is noticed on the next push, and rows expire after 30 days
  unless the app registers again (call `RegisterDevice` at every launch). A token that
  APNs or FCM reports as dead removes its device.

### APNs (iOS)

1. In the Apple Developer account: Certificates, Identifiers & Profiles, Keys, create a key
   with *Apple Push Notifications service (APNs)*; download the `.p8` file once and note the
   **Key ID**; your **Team ID** is in the membership page.
2. Make sure the App ID `si.zabit.thehideout` (the bundle id) has Push Notifications enabled.

| Variable | Meaning |
| --- | --- |
| `THEHIDEOUT_APNS_KEY_PATH` | path of the `.p8` file (or use `THEHIDEOUT_APNS_KEY`) |
| `THEHIDEOUT_APNS_KEY` | the PEM text of the key; newlines may be written as `\n` |
| `THEHIDEOUT_APNS_KEY_ID` | the Key ID |
| `THEHIDEOUT_APNS_TEAM_ID` | the Team ID |
| `THEHIDEOUT_APNS_TOPIC` | the bundle id, default `si.zabit.thehideout` |
| `THEHIDEOUT_APNS_SANDBOX` | `true` sends everything through `api.sandbox.push.apple.com` (default `false`) |

Production (`api.push.apple.com`) serves App Store and TestFlight builds; builds from Xcode
use the sandbox. An app can say so per device with `RegisterDevice.sandbox = true`, so
one server can serve both without the global switch. The provider token (ES256 JWT) is
signed locally and renewed every 50 minutes.

### FCM (Android)

1. In the Firebase console create a project, add the Android app, then Project settings,
   Service accounts, *Generate new private key* (a JSON file; the account needs the
   *Firebase Cloud Messaging API Admin* role, which the default Firebase Admin SDK account has).
2. `THEHIDEOUT_FCM_SERVICE_ACCOUNT` = the path of that file, or the JSON itself (on one line).

The server signs an OAuth2 assertion with the account's key, caches the access token until
shortly before it expires, and sends through the HTTP v1 API.

Treat both secrets like the other credentials (see the production env file); the server never
logs them or the device tokens. Production compose passes these variables through when set
(`deploy/.env.production`).

### Privacy

Push tokens are stored and message previews pass through Apple's and Google's push
services when previews are on; the privacy notice says so (version `2026-11-23`: it names the
two services as processors, so everyone is asked to accept the notice again).

## Media storage (S3)

Avatars and guild icons are processed (centre-cropped, 256×256, lossless WebP) and
stored in an S3-compatible bucket; clients load them from `GET /media/{key}` on the
server (public, immutable cache headers). If the object store is unreachable at
startup the server logs one warning and keeps running: the media RPCs answer
`UNAVAILABLE` and `/media` answers 503 until it is back.

| Variable | Default (local compose) | Meaning |
| --- | --- | --- |
| `THEHIDEOUT_S3_ENDPOINT` | `http://127.0.0.1:8333` | S3 API endpoint |
| `THEHIDEOUT_S3_REGION` | `us-east-1` | signing region |
| `THEHIDEOUT_S3_BUCKET` | `thehideout-media` | bucket (must exist) |
| `THEHIDEOUT_S3_ACCESS_KEY` | `thehideout-dev` | access key id |
| `THEHIDEOUT_S3_SECRET_KEY` | `thehideout-dev-secret` | secret key |
| `THEHIDEOUT_S3_PATH_STYLE` | `true` | `endpoint/bucket/key` addressing; set `false` for virtual-hosted style |
| `THEHIDEOUT_S3_PUBLIC_BASE` | unset | reserved for a CDN in front of `/media`; currently only logged |

The compose credentials are well-known development values, valid only for the
127.0.0.1-bound local service. Never reuse them anywhere reachable.

### Hosting on a real S3 provider

Any S3-compatible provider works (AWS S3, Cloudflare R2, Backblaze B2, Wasabi, a
self-hosted SeaweedFS/Garage, ...): create a private bucket, create an access key
limited to it, and set `THEHIDEOUT_S3_ENDPOINT`, `_REGION`, `_BUCKET`, `_ACCESS_KEY`
and `_SECRET_KEY`. For AWS S3 set `THEHIDEOUT_S3_PATH_STYLE=false` (and the bucket's
real region); R2 and most others accept either style with `us-east-1`/`auto`
regions as documented by the provider. The bucket stays private: the server
streams objects through `/media`, so no public bucket policy is needed.

## Production deployment

`deploy/` has a Docker Compose stack (server, ScyllaDB, LiveKit with TURN, Caddy with automatic
TLS), backup and restore scripts and a step-by-step guide: see [deploy/README.md](deploy/README.md).
The server image is built from the root `Dockerfile`; CI is in `.github/workflows/ci.yml`.
