# Deploying thehideout on Dokploy

This deploys the same stack as [../README.md](../README.md) (server, ScyllaDB,
LiveKit; media in Cloudflare R2, email through Resend) as a Dokploy **Docker
Compose** service. Dokploy's Traefik replaces Caddy: it terminates TLS (Let's
Encrypt) for both domains and routes them to the containers. The examples use
`thehideout.zabit.si` (API, web pages, media) and `voice.thehideout.zabit.si`
(LiveKit signalling); replace them with your own.

Files in this directory:

- `docker-compose.yml`: the stack for Dokploy.
- `.env.example`: every variable to paste into Dokploy's Environment tab.

The server image is `ghcr.io/timrekelj/thehideout-server` (linux/amd64 only, so
use an x86-64 Dokploy server). Tags: `latest` (main), the version of a `v*` tag
(for `v1.2.3`: `1.2.3`) and `sha-<commit>`.

## How it fits together

| Service | Role | Reachable via |
| --- | --- | --- |
| `server` | Connect RPC (incl. the long-lived streaming gateway), `/media`, invite/passkey pages, `/livekit/webhook` | Traefik, domain 1, port 8080 (`expose` only) |
| `scylla-init` | one-shot: creates the app role, sets the superuser password | none |
| `scylla` | database, single node, named volume `scylla-data` | compose network only |
| `livekit` | voice/video SFU | signalling: Traefik, domain 2, port 7880. Media: host ports 7881/tcp and 7882/udp |
| Cloudflare R2 | S3 storage for media (external) | server connects out over HTTPS |

Differences from the plain-VPS stack:

- No Caddy, and no host port 8080 or 7880: only Traefik is public on 80/443.
- LiveKit is on the normal bridge network. It uses a single UDP port
  (`rtc.udp_port: 7882`, LiveKit's "ICE/UDP mux", which replaces the
  50000-60000 range) and `rtc.tcp_port: 7881`, both published on the host, with
  `rtc.use_external_ip: true` so it advertises the server's public IP.
- The server reaches LiveKit at `http://livekit:7880` and LiveKit posts webhooks
  to `http://server:8080/livekit/webhook`, both over the compose network.

## 1. Prerequisites

- A Dokploy installation (<https://docs.dokploy.com/docs/core/installation>) on an
  x86-64 server, with the Dokploy UI reachable.
- DNS A records (and AAAA if the server has IPv6) for both domains pointing at the
  Dokploy server. They must resolve before you deploy, or Let's Encrypt fails:

  ```
  thehideout.zabit.si        A  <Dokploy server IP>
  voice.thehideout.zabit.si  A  <Dokploy server IP>
  ```

- Firewall (cloud firewall and/or `ufw`) open for:

  | Port | Protocol | Used by |
  | --- | --- | --- |
  | 80 | tcp | HTTP, Let's Encrypt challenge (Traefik) |
  | 443 | tcp, udp | HTTPS (Traefik) |
  | 7881 | tcp | LiveKit media over TCP |
  | 7882 | udp | LiveKit media (single-port UDP mux) |

  Keep 7880 and 8080 closed; they are never published. Docker publishes 7881 and
  7882 around `ufw`, but a cloud firewall still has to allow them. Dokploy's own
  ports (SSH, the UI on 3000) are your call.
- Cloudflare R2 bucket and token, and a Resend account (steps 8 and 9).

## 2. Make the image pullable

The first run of the `server-image` workflow publishes the GHCR package as
private. Either:

- (Simplest) make it public: GitHub > your profile > Packages >
  `thehideout-server` > Package settings > Change visibility > Public. Then
  nothing needs configuring in Dokploy.
- Or keep it private and log the Dokploy server's Docker in to GHCR
  (`docker login ghcr.io` on that server, with a token that has `read:packages`).
  Dokploy also has a Registry section (Settings > Registry); whether a Compose
  service uses it for pulls was not verified, so test the deploy.

## 3. Create the Compose service

1. In Dokploy create (or open) a Project, then **Create Service > Compose**.
   Choose type **Docker Compose** (not Stack: Stack is Swarm mode and does not
   fit this file).
2. Source, one of:
   - **GitHub** (or Git): select `timrekelj/thehideout`, branch `main`, and set
     the **Compose Path** to `deploy/dokploy/docker-compose.yml`. This is the
     recommended way: updates to the file arrive with the repo. The repo is
     public, so the **Git** provider with `https://github.com/timrekelj/thehideout.git`
     needs no credentials.
   - **Raw**: paste the contents of `docker-compose.yml` into the editor. Nothing
     in this file reads other files from the repo, so this works too, but you
     update it by hand.
3. The file keeps LiveKit's config inline (`configs: content:`) instead of
   bind-mounting a file from the repo, because Dokploy re-clones the repo on each
   deploy and would wipe relative bind mounts. Do not change that to a bind mount.

## 4. Enable Isolated Deployments

Open the service's **Advanced** tab and enable **Isolated Deployments**. Dokploy
then creates a network for this service, attaches every service to it and
connects Traefik to it. That is what makes `scylla`, `server` and `livekit`
resolve by name, and you do not have to add `dokploy-network` yourself.

(Without it, Dokploy only adds `dokploy-network` to the services you attach a
domain to, so `server` and `livekit` would be on a different network from
`scylla` and you would have to put `dokploy-network` on every service by hand.)

## 5. Environment

Open the **Environment** tab and paste the contents of `.env.example`, then edit:

- `THEHIDEOUT_DOMAIN`, `LIVEKIT_DOMAIN`.
- `THEHIDEOUT_RESEND_API_KEY` and `THEHIDEOUT_EMAIL_FROM` (step 9).
- `THEHIDEOUT_S3_ENDPOINT`, `THEHIDEOUT_S3_BUCKET`, `THEHIDEOUT_S3_ACCESS_KEY`
  and `THEHIDEOUT_S3_SECRET_KEY` from step 8. Leave `THEHIDEOUT_S3_REGION=auto`
  and `THEHIDEOUT_S3_PATH_STYLE=true`.
- Generate the LiveKit secrets (hex only, they are written into a YAML file):

  ```sh
  openssl rand -hex 16   # LIVEKIT_API_KEY
  openssl rand -hex 32   # LIVEKIT_API_SECRET
  ```

  LiveKit needs an API secret of at least 32 characters.
- Generate the Scylla credentials (hex only: they are interpolated into CQL):

  ```sh
  openssl rand -hex 32   # SCYLLA_SUPERUSER_PASSWORD
  openssl rand -hex 32   # THEHIDEOUT_SCYLLA_PASSWORD
  ```

  Scylla runs with password authentication. The one-shot `scylla-init` service
  creates the `thehideout` application role and replaces the default
  `cassandra/cassandra` superuser password on every deploy (the same script as
  `deploy/scylla-init.sh`, inlined in the compose file). To rotate, change the
  values and redeploy.
- `THEHIDEOUT_CONTACT_EMAIL` (required: shown on `/privacy` and `/terms`) and
  optionally `THEHIDEOUT_OPERATOR_NAME`.
- Optionally `THEHIDEOUT_MIN_CLIENT_VERSION` (and the `_DESKTOP`, `_IOS`,
  `_ANDROID` overrides) to force old clients to update, and the `THEHIDEOUT_APNS_*`
  / `THEHIDEOUT_FCM_SERVICE_ACCOUNT` push-notification values (empty = off).
- `THEHIDEOUT_TRUSTED_PROXIES`: Traefik's address or subnet on the stack's Docker
  network, so the server sees real client IPs behind it. Find it with
  `docker network ls`, then `docker network inspect <app name>` (the subnet of the
  isolated network). Empty is safe but every request then appears to come from
  Traefik, which weakens per-IP rate limits.
- Optionally `THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL` (step 7), `SCYLLA_SMP` and
  `SCYLLA_MEMORY` (size them to the server; Scylla runs with `--developer-mode 1`
  by default and shares the machine with Dokploy; set `SCYLLA_DEVELOPER_MODE=0`
  on a tuned host).

Dokploy writes these to a `.env` next to the compose file but does not inject them
into containers by itself. The compose file therefore references each one as
`${VAR}`, and the same values feed LiveKit's config and the server.

## 6. Domains

Open the **Domains** tab and add two domains:

| Host | Service | Port | HTTPS | Certificate |
| --- | --- | --- | --- | --- |
| `thehideout.zabit.si` | `server` | `8080` | on | Let's Encrypt |
| `voice.thehideout.zabit.si` | `livekit` | `7880` | on | Let's Encrypt |

Dokploy adds the Traefik labels at deploy time (use **Preview Compose** to see
the final file). LiveKit signalling is a websocket, which Traefik proxies
without extra setup.

## 7. Deploy and create the first admin

Press **Deploy** and watch the deployment log. The first start takes a few
minutes: Scylla must become healthy and `scylla-init` must finish before the
server starts and runs its migrations. Then open `https://thehideout.zabit.si` and check the service logs
(Logs tab).

Accounts are invite-only. Either:

- set `THEHIDEOUT_BOOTSTRAP_ADMIN_EMAIL` in the environment before the first
  deploy. On an empty install the server emails a site-admin invite and logs it
  (`created bootstrap admin invite`); or
- run the invite CLI in the running container at any time. On the Dokploy server:

  ```sh
  APP=<app name shown in Dokploy>
  docker exec -it "$(docker ps -q -f label=com.docker.compose.project=$APP -f label=com.docker.compose.service=server)" \
    thehideout-server invite you@example.com --admin
  ```

  (Dokploy runs the stack as the Compose project named after the service's app
  name, so the labels pick exactly the `server` container.) You can also
  open a terminal on the `server` container from Dokploy's Docker section and run
  `thehideout-server invite you@example.com --admin`.

Open the invite link from the email (it lives under `THEHIDEOUT_PUBLIC_URL`) to
create the account. Further people are invited from the dashboard.

## 8. R2 setup

1. In the Cloudflare dashboard open R2 and create a bucket (for example
   `thehideout-media`). Keep it private: the server streams `/media/{key}` itself.
2. R2 > Manage API tokens > Create API token, permission **Object Read & Write**,
   scoped to that bucket. Copy the **Access Key ID** and **Secret Access Key**
   (shown once).
3. The S3 endpoint is `https://<ACCOUNT_ID>.r2.cloudflarestorage.com` (without
   the bucket name).

Details: [../README.md](../README.md#4-r2-setup). The stack does not create the bucket.

## 9. Resend

Add your sending domain at <https://resend.com>, add the SPF/DKIM records it
shows, wait for verification, then create an API key and put it in
`THEHIDEOUT_RESEND_API_KEY`, with `THEHIDEOUT_EMAIL_FROM` on the verified domain.
Redeploy to apply. Details: [../README.md](../README.md#8-resend).

## Updating

- With `THEHIDEOUT_VERSION=latest`: press **Deploy** (or **Redeploy**) on the
  service. The server service has `pull_policy: always`, so the new image is
  pulled.
- To pin a version, set `THEHIDEOUT_VERSION=1.2.3` (or `sha-<commit>`) in the
  Environment tab and deploy. To roll back, set the earlier tag and deploy.
  Migrations run on startup and are not reverted, so back up first.

### Build on Dokploy instead of pulling the image

To build the server from the repo, use the GitHub/Git source and replace the
`image` and `pull_policy` lines of the `server` service with:

```yaml
    build:
      context: ../..
      dockerfile: Dockerfile
```

Compose resolves `context` relative to the compose file, so `../..` is the repo
root when the compose path is `deploy/dokploy/docker-compose.yml` (the same
context the GitHub workflow uses). It does not work with the Raw source, and the
build runs on the Dokploy server (Rust build; slow and memory hungry). Pulling
the published image is the default and recommended.

## Auto-deploy

Dokploy's per-service **webhook URL** is for Git pushes: it checks the branch (and
watch paths) of a GitHub/GitLab/Gitea/Bitbucket push payload, so a plain POST from
the image workflow is rejected (`Branch Not Match`). The documented alternative
that fits "new image pushed" is the Dokploy API, `POST /api/compose.deploy` with
`x-api-key` and `{"composeId": "..."}`.

`.github/workflows/server-image.yml` does that after the image push on `main`
(the `latest` tag only), when these GitHub repository secrets are set; if any is
missing it skips with a `::notice::`:

| Secret | Value |
| --- | --- |
| `DOKPLOY_URL` | Base URL of your Dokploy UI, e.g. `https://dokploy.example.com` |
| `DOKPLOY_API_KEY` | API key created in your Dokploy profile settings |
| `DOKPLOY_COMPOSE_ID` | The Compose service's ID (it is in the service page URL) |

If you pin `THEHIDEOUT_VERSION`, leave the secrets unset: there is nothing new to
deploy. (Turning on the service's **Auto Deploy** toggle is only useful when the
source is a Git repo and you want compose-file changes deployed on push; it does
not react to new images.)

## Backups

The database is the named volume `<appName>_scylla-data`; media is in R2. Keep
the environment values backed up separately: the credentials must match your R2
token and LiveKit setup.

A raw copy of a live Scylla volume can be inconsistent (memtables are not flushed,
files change while copying). Dokploy's **Volume Backups** (Volume Backups tab:
schedule, S3 destination, service `scylla`, volume `scylla-data`) back up named
volumes to S3, and have a **Turn off container** option that stops the container
during the backup. Use it, or take a snapshot first:

1. Take a snapshot with a **Schedule Job** (Schedules tab, type Compose, service
   `scylla`; Dokploy runs the command with `docker exec` in the container), for
   example daily:

   ```sh
   nodetool snapshot -t daily thehideout
   ```

   Snapshots are hard links under `/var/lib/scylla/data/thehideout/<table>/snapshots/`
   inside the same volume, so the Volume Backup then contains a consistent copy.
   Use the keyspace from `THEHIDEOUT_KEYSPACE` if you changed it. A tag that is
   reused fails, so clear the old one first (`nodetool clearsnapshot -t daily thehideout`)
   in a job that runs just before, or wrap both in `sh -c '...'` with a dated
   tag; unused snapshots take disk space.
2. Schedule the Volume Backup shortly after the snapshot job.

Restore as in [../README.md](../README.md#backups): start the stack on an empty
volume so migrations create the tables, copy the snapshot files back into each
table directory and run `nodetool refresh`. Volume Backup's restore needs a
target volume that does not exist and is not in use. Test the restore before you
need it.

For media, R2 is replicated by Cloudflare; mirror it with rclone if you want an
independent copy.

## Troubleshooting

**Logs**: the service's Logs tab in Dokploy per service, or on the server
`docker logs -f <container>` (`docker ps` shows them).

**Server does not start**: Scylla must be healthy first (watch its logs, the first
start takes minutes; `scylla-init` logs `role ... ready` when it is done). S3 errors mean wrong R2 values: endpoint without the bucket,
token scoped to the bucket, bucket exists. Compose fails early with `set X in the
environment` when a required variable is missing.

**Certificate not issued / 404 from Traefik**: check the DNS records resolve to the
server, ports 80/443 are open, and the domain's service and port match step 6.

**Voice does not connect** (page works, joining a voice channel hangs):

- Check 7881/tcp and 7882/udp are open in the cloud firewall and `ufw`, and
  published: `docker ps` should show `0.0.0.0:7881->7881/tcp` and
  `0.0.0.0:7882->7882/udp` for LiveKit. A UDP probe, from outside:
  `nc -zvu <server IP> 7882` (UDP checks are not conclusive; LiveKit's TCP
  fallback on 7881 should still connect).
- The domain `voice.…` must reach LiveKit: `https://voice.thehideout.zabit.si`
  should answer `OK`. If not, check the domain row in step 6.
- LiveKit must advertise the public IP: its log at startup lists the node IP
  (`use_external_ip`). A private IP there means STUN discovery failed; allow
  outbound UDP on the server.
- Webhooks: if the call connects but participant state in the app is wrong,
  LiveKit cannot reach the server. Both must be on the same (isolated) network;
  look for webhook errors in `livekit` logs. The path is
  `http://server:8080/livekit/webhook`.

**Gateway stream drops through Traefik**: the app's long-lived server-streaming
connection goes through Traefik's HTTP/websocket entry point, which has no idle
cut-off by default. If it drops at a fixed interval, look for a timeout set on the
Traefik side (custom Traefik config) or a proxy/CDN in front of the domain (for
example Cloudflare's proxy: use DNS only, grey cloud, for both domains).

## Limits

- Single node only: one server instance (`THEHIDEOUT_WORKER_ID=0`) and one Scylla node.
- The image is linux/amd64 only.
- One LiveKit node with a single UDP port: no clustering.
