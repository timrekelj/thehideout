#!/usr/bin/env bash
# Back up thehideout: a ScyllaDB snapshot of the keyspace plus the S3 bucket.
#
#   ./backup.sh                       # run from anywhere; reads ../.env.production
#
# Produces  $BACKUP_DIR/<UTC timestamp>/  containing
#   scylla.tar.gz   the snapshot SSTables (data/<table-dir>/snapshots/<tag>/...)
#   schema.cql      DESCRIBE KEYSPACE output, needed to restore
#   s3/             mirror of the bucket (BACKUP_S3_MODE=rclone|aws, no BACKUP_S3_DEST)
#   MANIFEST        what was backed up
# and prunes all but the newest $BACKUP_KEEP archive directories.
#
# Configuration (environment, or ../.env.production; real env wins):
#   BACKUP_DIR           archive root                      (default ./backups next to this script)
#   BACKUP_KEEP          archives to keep                  (default 14)
#   THEHIDEOUT_KEYSPACE  keyspace to back up               (default thehideout)
#   SCYLLA_CONTAINER     container name                    (default: `docker compose` service scylla)
#   SCYLLA_SUPERUSER     CQL user for DESCRIBE             (default cassandra; empty = no auth)
#   SCYLLA_SUPERUSER_PASSWORD
#   BACKUP_S3_MODE       rclone (default, via docker) | aws (host aws cli) | none
#   BACKUP_S3_DEST       rclone remote:path or s3:// URL; empty = mirror into the archive dir
#   THEHIDEOUT_S3_*      source bucket, from the server's env
#   COMPOSE_NETWORK      network rclone runs in            (default thehideout_edge)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
deploy_dir="$(dirname "$here")"
# shellcheck source=/dev/null
load_env() { # load only variables not already set in the environment
  local f=$1 line k v
  [[ -f "$f" ]] || return 0
  while IFS= read -r line || [[ -n "$line" ]]; do
    [[ "$line" =~ ^[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)=(.*)$ ]] || continue
    k=${BASH_REMATCH[1]}; v=${BASH_REMATCH[2]}
    v=${v%\"}; v=${v#\"}
    [[ -n "${!k+x}" ]] || export "$k=$v"
  done <"$f"
}
load_env "${ENV_FILE:-$deploy_dir/.env.production}"

BACKUP_DIR=${BACKUP_DIR:-$here/backups}
BACKUP_KEEP=${BACKUP_KEEP:-14}
KEYSPACE=${THEHIDEOUT_KEYSPACE:-thehideout}
SU=${SCYLLA_SUPERUSER-cassandra}
SU_PASS=${SCYLLA_SUPERUSER_PASSWORD:-}
S3_MODE=${BACKUP_S3_MODE:-rclone}
NETWORK=${COMPOSE_NETWORK:-thehideout_edge}
DATA_DIR=/var/lib/scylla/data

log() { printf '%s backup: %s\n' "$(date -u +%H:%M:%S)" "$*"; }
die() { log "ERROR: $*" >&2; exit 1; }

[[ "$KEYSPACE" =~ ^[A-Za-z0-9_]+$ ]] || die "invalid keyspace name"
[[ "$BACKUP_KEEP" =~ ^[0-9]+$ && "$BACKUP_KEEP" -ge 1 ]] || die "BACKUP_KEEP must be a positive integer"

container=${SCYLLA_CONTAINER:-}
if [[ -z "$container" ]]; then
  container=$(docker compose -f "$deploy_dir/compose.yml" ps -q scylla 2>/dev/null | head -n1 || true)
fi
[[ -n "$container" ]] || die "Scylla container not found (set SCYLLA_CONTAINER)"
docker inspect -f '{{.State.Running}}' "$container" 2>/dev/null | grep -q true || die "container $container is not running"

cqlsh_auth=()
[[ -n "$SU" && -n "$SU_PASS" ]] && cqlsh_auth=(-u "$SU" -p "$SU_PASS")

ts=$(date -u +%Y%m%dT%H%M%SZ)
tag="backup-$ts"
dest="$BACKUP_DIR/$ts"
mkdir -p "$dest"
chmod 700 "$BACKUP_DIR" "$dest"
ok=0
cleanup() {
  docker exec "$container" nodetool clearsnapshot -t "$tag" "$KEYSPACE" >/dev/null 2>&1 || true
  # A failed run leaves no half-written archive behind.
  [[ "$ok" == 1 ]] || rm -rf -- "$dest"
}
trap cleanup EXIT

# 1. Schema (restore recreates tables from this before loading SSTables).
log "dumping schema of keyspace $KEYSPACE"
docker exec "$container" cqlsh ${cqlsh_auth[@]+"${cqlsh_auth[@]}"} -e "DESCRIBE KEYSPACE $KEYSPACE" >"$dest/schema.cql"
grep -q "CREATE KEYSPACE" "$dest/schema.cql" || die "schema dump looks empty (keyspace $KEYSPACE missing, wrong credentials?)"

# 2. Snapshot (hard links, consistent per table, safe on a live node), then tar it out.
log "taking snapshot $tag"
docker exec "$container" nodetool snapshot -t "$tag" "$KEYSPACE" >/dev/null
log "copying snapshot out of the container"
# (The Scylla image has no tar: list the snapshot dirs, `docker cp` each to a
# staging dir on the host, tar there.)
stage=$(mktemp -d "$dest/.stage.XXXXXX")
snap_dirs=$(docker exec "$container" find "$DATA_DIR/$KEYSPACE" -type d -path "*/snapshots/$tag") || die "could not list the snapshot"
[[ -n "$snap_dirs" ]] || die "snapshot produced no directories (keyspace $KEYSPACE empty or missing?)"
while IFS= read -r d; do
  rel=${d#"$DATA_DIR"/}
  mkdir -p "$stage/$rel"
  docker cp "$container:$d/." "$stage/$rel" >/dev/null || die "docker cp failed for $rel"
done <<<"$snap_dirs"
tar -czf "$dest/scylla.tar.gz" -C "$stage" "$KEYSPACE" || die "could not archive the snapshot"
rm -rf -- "$stage"
tables=$(tar -tzf "$dest/scylla.tar.gz" | awk -F/ 'NF>=4 && $3=="snapshots" && $4=="'"$tag"'" && $5=="" {print $2}' | sort -u)
[[ -n "$tables" ]] || die "snapshot archive contains no tables"
log "archived $(echo "$tables" | wc -l | tr -d ' ') tables ($(du -h "$dest/scylla.tar.gz" | cut -f1))"

# 3. Object storage.
s3_summary="skipped (BACKUP_S3_MODE=$S3_MODE)"
if [[ "$S3_MODE" != none ]]; then
  : "${THEHIDEOUT_S3_ENDPOINT:?THEHIDEOUT_S3_ENDPOINT not set}" "${THEHIDEOUT_S3_BUCKET:?}" "${THEHIDEOUT_S3_ACCESS_KEY:?}" "${THEHIDEOUT_S3_SECRET_KEY:?}"
  region=${THEHIDEOUT_S3_REGION:-us-east-1}
  case "$S3_MODE" in
    rclone)
      # Source remote configured purely through env vars; the destination is either
      # a user-supplied rclone remote (configured the same way, see README) or a
      # local mirror inside the archive directory.
      mkdir -p "$dest/s3"
      dst_args=(/dest)
      mount_args=(-v "$dest/s3:/dest")
      if [[ -n "${BACKUP_S3_DEST:-}" ]]; then dst_args=("$BACKUP_S3_DEST"); mount_args=(); fi
      log "syncing bucket ${THEHIDEOUT_S3_BUCKET} with rclone"
      docker run --rm --network "$NETWORK" ${mount_args[@]+"${mount_args[@]}"} \
        -e RCLONE_CONFIG_SRC_TYPE=s3 -e RCLONE_CONFIG_SRC_PROVIDER=Other \
        -e RCLONE_CONFIG_SRC_ENDPOINT="$THEHIDEOUT_S3_ENDPOINT" \
        -e RCLONE_CONFIG_SRC_REGION="$region" \
        -e RCLONE_CONFIG_SRC_ACCESS_KEY_ID="$THEHIDEOUT_S3_ACCESS_KEY" \
        -e RCLONE_CONFIG_SRC_SECRET_ACCESS_KEY="$THEHIDEOUT_S3_SECRET_KEY" \
        -e RCLONE_CONFIG_SRC_FORCE_PATH_STYLE="${THEHIDEOUT_S3_PATH_STYLE:-true}" \
        -e RCLONE_S3_NO_CHECK_BUCKET=true \
        ${RCLONE_EXTRA_ENV:+$RCLONE_EXTRA_ENV} \
        rclone/rclone:1.68 sync "SRC:${THEHIDEOUT_S3_BUCKET}" "${dst_args[0]}" --fast-list --transfers 8 \
        || die "rclone sync failed"
      s3_summary="rclone ${THEHIDEOUT_S3_BUCKET} -> ${BACKUP_S3_DEST:-$dest/s3}"
      ;;
    aws)
      command -v aws >/dev/null || die "aws cli not installed"
      target=${BACKUP_S3_DEST:-$dest/s3}
      [[ "$target" == s3://* ]] || mkdir -p "$target"
      log "syncing bucket ${THEHIDEOUT_S3_BUCKET} with aws s3 sync"
      AWS_ACCESS_KEY_ID=$THEHIDEOUT_S3_ACCESS_KEY AWS_SECRET_ACCESS_KEY=$THEHIDEOUT_S3_SECRET_KEY AWS_DEFAULT_REGION=$region \
        aws s3 sync "s3://${THEHIDEOUT_S3_BUCKET}" "$target" --endpoint-url "$THEHIDEOUT_S3_ENDPOINT" --only-show-errors \
        || die "aws s3 sync failed"
      s3_summary="aws ${THEHIDEOUT_S3_BUCKET} -> $target"
      ;;
    *) die "BACKUP_S3_MODE must be rclone, aws or none" ;;
  esac
fi

cat >"$dest/MANIFEST" <<EOF
created=$ts
keyspace=$KEYSPACE
snapshot_tag=$tag
tables=$(echo "$tables" | tr '\n' ' ')
s3=$s3_summary
EOF
(cd "$dest" && find . -type f ! -path './s3/*' -exec sha256sum {} + 2>/dev/null || find . -type f ! -path './s3/*' -exec shasum -a 256 {} +) >"$dest/SHA256SUMS" || true
ok=1

# 4. Retention: keep the newest $BACKUP_KEEP timestamped directories.
total=$(find "$BACKUP_DIR" -mindepth 1 -maxdepth 1 -type d -name '[0-9]*T[0-9]*Z' | wc -l | tr -d ' ')
if ((total > BACKUP_KEEP)); then
  find "$BACKUP_DIR" -mindepth 1 -maxdepth 1 -type d -name '[0-9]*T[0-9]*Z' | sort | head -n "$((total - BACKUP_KEEP))" |
    while IFS= read -r old; do
      log "pruning old archive $(basename "$old")"
      rm -rf -- "$old"
    done
fi
log "done: $dest ($(du -sh "$dest" | cut -f1)); s3: $s3_summary"
