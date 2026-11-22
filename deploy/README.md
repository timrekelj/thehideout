# Deploying thehideout

A single-host production stack with Docker Compose:

| Service | Role | Reachable from |
| --- | --- | --- |
| `caddy` | reverse proxy, automatic HTTPS (Let's Encrypt) | internet: 80, 443 (tcp+udp) |
| `server` | the thehideout server (built from `../Dockerfile`) | Caddy only |
| `scylla` | ScyllaDB, single node, password auth, internal network, data volume | `server` only |
| `scylla-init` | one-shot: creates the app role, replaces the default superuser password | - |
| `livekit` (+ `livekit-config`) | voice/video SFU with built-in TURN | signalling via Caddy; media on 7881/tcp, the UDP range, 3478/udp |
| `seaweedfs` (+ `-init`), profile `seaweedfs` | optional bundled S3 | internal only |

Files: `compose.yml`, `Caddyfile`, `livekit.yaml` (template), `scylla-init.sh`,
`.env.production.example`, `backup/{backup,restore}.sh`, `backup/systemd/`.

## Requirements

- A Linux host (Debian/Ubuntu tested in mind) with Docker Engine 24+ and the Compose plugin.
  **Not** Docker Desktop: production-mode Scylla needs the host's I/O tuning (`iotune`)
  and fails in Docker Desktop's VM.
- 2 vCPU and 4 GB RAM minimum (Scylla defaults: `SCYLLA_SMP=2`, `SCYLLA_MEMORY=2G`), SSD
  storage, and a public IPv4 address that is not behind NAT (LiveKit advertises it).
- Two DNS names pointing at the host (A and, if you have it, AAAA):
  - `hideout.example.com` -> the app (`THEHIDEOUT_DOMAIN`)
  - `voice.example.com` -> LiveKit signalling and TURN (`LIVEKIT_DOMAIN`)
- An S3 bucket (see below) and a [Resend](https://resend.com) account with a verified domain.

### Firewall

| Port | Protocol | Purpose |
| --- | --- | --- |
| 80 | TCP | ACME HTTP challenge and redirect to HTTPS |
| 443 | TCP, UDP | HTTPS / HTTP3 (app and LiveKit signalling) |
| 7881 | TCP | LiveKit ICE/TCP fallback |
| 50000-50100 | UDP | LiveKit media (`LIVEKIT_UDP_START`..`END`) |
| 3478 | UDP | TURN |
| (5349 | TCP | optional TURN over TLS, see `livekit.yaml`) |

Everything else (9042 Scylla, 8080 server, 7880 LiveKit API, S3) is never published. Note
that Docker's published ports bypass `ufw`; the table is what the host should expose, and
compose publishes only those.

## First deploy

```sh
git clone <repo> && cd thehideout/deploy
cp .env.production.example .env.production && chmod 600 .env.production
$EDITOR .env.production     # fill in every [required] value; generate secrets with: openssl rand -hex 32
docker compose --env-file .env.production config -q      # validates the file
docker compose --env-file .env.production up -d --build  # add --profile seaweedfs for bundled S3
docker compose --env-file .env.production logs -f server
```

Caddy obtains the certificates on the first start (DNS must already resolve and port 80/443
must be reachable, or issuance fails and retries with back-off). The first start builds the
Rust image (several minutes) and Scylla takes about a minute to become healthy; with
production mode it runs I/O benchmarking on every container (re)creation.

Check it works:

```sh
curl -fsS https://hideout.example.com/health
docker compose --env-file .env.production exec server curl -fsS http://127.0.0.1:8080/ready
```

(`/ready`, which checks Scylla and S3, is answered 404 on the public address by Caddy on
purpose; use it from inside, from monitoring on the host, or from the docker healthcheck.)

### The first admin

Accounts are invite-only. On an empty install the server creates a site-admin invite for
`THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL` at startup, emails it through Resend and logs the code and
link (`created bootstrap admin invite`):

```sh
docker compose --env-file .env.production logs server | grep -i "bootstrap admin invite"
```

At any time you can invite someone (add `--admin` for a site admin); it prints the code and link:

```sh
docker compose --env-file .env.production run --rm --no-deps server invite alice@example.com --admin
```

### Object storage

**External provider (recommended).** Create a *private* bucket and an access key limited to
it (AWS S3, Cloudflare R2, Backblaze B2, Wasabi, ...), then set `THEHIDEOUT_S3_ENDPOINT`,
`_REGION`, `_BUCKET`, `_ACCESS_KEY`, `_SECRET_KEY`, `_PATH_STYLE` (`false` for AWS). Nothing else
to run; the server streams objects through `/media`, so the bucket stays private. Examples are
in `.env.production.example`.

**Bundled SeaweedFS.** Start with `--profile seaweedfs`, set
`THEHIDEOUT_S3_ENDPOINT=http://seaweedfs:8333`, and generate a *new* key pair
(`openssl rand -hex 12` / `openssl rand -hex 32`); the server refuses the dev keys. The bucket
is created by `seaweedfs-init`. Data lives in the `seaweedfs-data` volume on this host, so it
is part of what you must back up (the backup script does).

### Email DNS (Resend)

Add the domain in Resend (Domains -> Add) and publish the records it shows at your DNS
provider, then wait for "Verified". They are, for a sending domain `hideout.example.com`:

| Type | Name | Value |
| --- | --- | --- |
| MX | `send.hideout.example.com` | `feedback-smtp.<region>.amazonses.com` (priority 10) |
| TXT (SPF) | `send.hideout.example.com` | `v=spf1 include:amazonses.com ~all` |
| TXT (DKIM) | `resend._domainkey.hideout.example.com` | the `p=...` public key Resend gives you |
| TXT (DMARC) | `_dmarc.hideout.example.com` | `v=DMARC1; p=none; rua=mailto:dmarc@hideout.example.com` |

Use exactly the values Resend displays (they differ per account and region); DMARC is yours:
start with `p=none`, read the reports, then move to `quarantine`/`reject`. Set
`THEHIDEOUT_EMAIL_FROM` to an address on that domain.

## Updating

```sh
cd thehideout && git pull
cd deploy
docker compose --env-file .env.production build server
docker compose --env-file .env.production up -d
```

Schema migrations run automatically at server start. The server gets SIGTERM and has
`stop_grace_period: 30s` to drain; clients reconnect their gateway stream. Take a backup before
upgrades that add migrations. Old images pile up: `docker image prune -f`. Pin a version with
`THEHIDEOUT_VERSION` if you build elsewhere and push to a registry (change `image:` accordingly).

LiveKit and Scylla versions are pinned in `compose.yml`; upgrade them deliberately, one at a
time, after a backup.

## Backups and restore

`backup/backup.sh` writes `backups/<UTC timestamp>/` (override with `BACKUP_DIR`) with:

- `scylla.tar.gz` + `schema.cql`: a `nodetool snapshot` of the keyspace (consistent, taken on
  the live node, hard-linked so it costs no extra space until the data changes) and the CQL schema;
- `s3/`: a mirror of the bucket via `rclone` (run from a container), or `aws s3 sync`
  (`BACKUP_S3_MODE=aws`), or a remote destination (`BACKUP_S3_DEST`), or skipped
  (`BACKUP_S3_MODE=none`, e.g. when the provider has versioning/replication);
- `MANIFEST`, `SHA256SUMS`.

It keeps the newest `BACKUP_KEEP` (default 14) archives. A failed run removes its partial archive.

```sh
./backup/backup.sh          # reads .env.production for keyspace, credentials and S3 settings
```

Schedule it, e.g. with the systemd timer in `backup/systemd/` (daily 03:30) or cron:

```cron
30 3 * * * cd /opt/thehideout/deploy && BACKUP_DIR=/var/backups/thehideout ./backup/backup.sh >>/var/log/thehideout-backup.log 2>&1
```

Archives on the same disk are not a backup. Copy `BACKUP_DIR` off the machine (restic, rsync,
a different bucket via `BACKUP_S3_DEST`, ...) and keep `.env.production` somewhere safe, too:
without the passwords and the LiveKit secret a restored system will not start.

### Restore

Restore into a **fresh** Scylla (new machine, or the volume deleted): `restore.sh` refuses to
touch an existing keyspace unless you pass `--drop-existing`.

```sh
cd deploy
docker compose --env-file .env.production up -d scylla scylla-init   # empty node, role created
./backup/restore.sh /var/backups/thehideout/20260101T033000Z         # asks for confirmation
# --no-s3: Scylla only; --drop-existing: DROP the keyspace first; --yes: don't prompt
```

It stops the server, recreates the schema from `schema.cql`, copies each table's snapshot
SSTables into the table's `upload/` directory and runs `nodetool refresh` (the supported
single-node way to load a snapshot; no `sstableloader` or token mapping is needed on one node),
copies the bucket back with `rclone copy` (never deleting objects), and starts the server.

The same by hand: `docker compose exec -T scylla cqlsh -u cassandra -p ... < schema.cql`; extract
`scylla.tar.gz`; for each `<table>-<old id>/snapshots/<tag>/*` copy the `*-Data.db`, `Index`,
`Statistics`, ... files into `/var/lib/scylla/data/<keyspace>/<table>-<NEW id>/upload/`
(`chown scylla:scylla`; the new id is in `system_schema.tables`); `nodetool refresh -- <keyspace> <table>`.

Practise a restore on a scratch machine before you need one. Restoring does not bring back
what lived only in memory (see below).

## Monitoring

- `GET /health`: liveness (cheap, public). `GET /ready`: checks Scylla and S3 (internal only).
  Point an external uptime monitor at `https://<domain>/health`.
- `docker compose ps` shows health; `docker compose logs -f server` has JSON logs
  (`THEHIDEOUT_LOG_FORMAT=json`, filter with `RUST_LOG`); Docker rotates them (20 MB x 5 per service).
- Watch the disk (Scylla volume, backups), certificate expiry (Caddy renews automatically) and
  that LiveKit webhooks arrive: the server logs a warning when voice joins are not confirmed.
- Voice not connecting for some users is almost always the UDP range, 7881/tcp, or TURN (3478/udp)
  being blocked: test from a phone on mobile data.
- TURN over TLS (5349/tcp) is documented but disabled in `livekit.yaml`; it needs a certificate
  (e.g. Caddy's, mounted read-only) and was not verified here.

## Operating notes and limits

- **Single node only.** Run exactly one `server` replica. Rate limits, 2FA and passkey challenge
  state, voice state (who is in which channel, server mutes) and the gateway fan-out are kept in
  memory; a restart clears them (clients and LiveKit reconcile through webhooks and rejoining).
  Scylla is one node with `SimpleStrategy:1`: a lost disk loses data, so back up.
- `THEHIDEOUT_TRUSTED_PROXIES` is set to the compose `edge` subnet (`EDGE_SUBNET`, default
  `172.28.0.0/24`) so the server reads the client IP from Caddy's `X-Forwarded-For`; the server
  is not published, so nothing else can claim to be the proxy. Change the subnet only if it clashes.
- The Caddyfile has no write timeout and `flush_interval -1` because the gateway is a long-lived
  server-streaming response; the request body limit is 60 MB (uploads are 50 MiB). Caddy
  blocks `/livekit/webhook` and `/ready` on the public site.
- LiveKit publishes its UDP range through Docker's proxy; keep the range small (default 100
  ports). For more capacity, run LiveKit with `network_mode: host` (then it must reach the server at
  `127.0.0.1:8080`, so publish that port on loopback).
- Secrets live in `.env.production` (mode 600, not in git). Rotate Scylla passwords by editing
  the file and re-running `docker compose --env-file .env.production up scylla-init` then restarting `server`.
- The data volumes are `thehideout_scylla-data`, `_caddy-data` (certificates; keep it to avoid
  Let's Encrypt rate limits when rebuilding), `_seaweedfs-data`.

## What was validated

`docker compose config` (with and without the seaweedfs profile), `caddy validate`, the image
build and start against the dev services, and `backup.sh` / `restore.sh` against a Scylla
container (snapshot, restore into a recreated schema, row counts compared, retention).
**Not** run: the production Scylla service (it needs a Linux host: Docker Desktop fails `iotune`
and the AIO limit) and therefore `scylla-init.sh` against a password-authenticated node; a
full stack with real TLS, DNS, LiveKit media/TURN and Resend. Expect to debug the first real
deploy; check `docker compose logs scylla scylla-init` first.
