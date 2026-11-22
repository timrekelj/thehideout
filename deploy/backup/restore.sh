#!/usr/bin/env bash
# Restore a backup made by backup.sh into the (single-node) Scylla and the S3 bucket.
#
#   ./restore.sh <archive-dir> [--no-s3] [--drop-existing] [--yes]
#
# What it does (the same steps you can run by hand, see README "Restore"):
#   1. stops the server (so nothing writes during the restore),
#   2. refuses to touch a keyspace that already exists unless --drop-existing,
#   3. recreates the schema from schema.cql,
#   4. copies each table's snapshot SSTables into that table's `upload/` directory
#      inside the container and runs `nodetool refresh` (Scylla loads them in
#      place; this is the documented way to restore a snapshot on a single node,
#      and it works on a fresh node as long as the schema was recreated first),
#   5. restores the bucket from <archive>/s3 (or BACKUP_S3_DEST) with `rclone copy`
#      (never deletes objects that exist only in the bucket),
#   6. starts the server again.
# Test restores on a scratch machine first. Same environment variables as backup.sh;
# SKIP_SERVER=1 leaves the server container alone.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
deploy_dir="$(dirname "$here")"
load_env() {
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

archive=""
do_s3=1; drop=0; yes=0
for a in "$@"; do
  case "$a" in
    --no-s3) do_s3=0 ;;
    --drop-existing) drop=1 ;;
    --yes) yes=1 ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}"; exit 0 ;;
    -*) echo "unknown option $a" >&2; exit 2 ;;
    *) archive=$a ;;
  esac
done
[[ -d "$archive" ]] || { echo "usage: $0 <archive-dir> [--no-s3] [--drop-existing] [--yes]" >&2; exit 2; }
archive=$(cd "$archive" && pwd)

KEYSPACE=${THEHIDEOUT_KEYSPACE:-thehideout}
SU=${SCYLLA_SUPERUSER-cassandra}
SU_PASS=${SCYLLA_SUPERUSER_PASSWORD:-}
NETWORK=${COMPOSE_NETWORK:-thehideout_edge}
DATA_DIR=/var/lib/scylla/data
compose=(docker compose -f "$deploy_dir/compose.yml" --env-file "${ENV_FILE:-$deploy_dir/.env.production}")

log() { printf '%s restore: %s\n' "$(date -u +%H:%M:%S)" "$*"; }
die() { log "ERROR: $*" >&2; exit 1; }

[[ "$KEYSPACE" =~ ^[A-Za-z0-9_]+$ ]] || die "invalid keyspace name"
[[ -f "$archive/schema.cql" && -f "$archive/scylla.tar.gz" ]] || die "$archive is not a backup (schema.cql / scylla.tar.gz missing)"
grep -q "CREATE KEYSPACE $KEYSPACE\b" "$archive/schema.cql" || die "archive schema is for another keyspace than $KEYSPACE (set THEHIDEOUT_KEYSPACE)"

container=${SCYLLA_CONTAINER:-}
[[ -n "$container" ]] || container=$("${compose[@]}" ps -q scylla 2>/dev/null | head -n1 || true)
[[ -n "$container" ]] || die "Scylla container not found: start it first (docker compose ... up -d scylla scylla-init); set SCYLLA_CONTAINER to override"

cqlsh_auth=()
[[ -n "$SU" && -n "$SU_PASS" ]] && cqlsh_auth=(-u "$SU" -p "$SU_PASS")
cql() { docker exec -i "$container" cqlsh ${cqlsh_auth[@]+"${cqlsh_auth[@]}"} "$@"; }

if [[ "$yes" != 1 ]]; then
  echo "About to restore $archive into keyspace '$KEYSPACE' on container $container$([[ $do_s3 == 1 ]] && echo ' and the S3 bucket')."
  read -r -p "Type 'restore' to continue: " answer
  [[ "$answer" == restore ]] || die "aborted"
fi

if [[ "${SKIP_SERVER:-0}" != 1 ]]; then
  log "stopping the server"
  "${compose[@]}" stop server || log "warning: could not stop the server (continuing)"
fi

# 1. Schema.
exists=$(cql -e "SELECT keyspace_name FROM system_schema.keyspaces WHERE keyspace_name = '$KEYSPACE'" | grep -c "$KEYSPACE" || true)
if [[ "$exists" -gt 0 ]]; then
  [[ "$drop" == 1 ]] || die "keyspace $KEYSPACE already exists; restore into a fresh node, or pass --drop-existing to DROP it first"
  log "dropping existing keyspace $KEYSPACE"
  cql -e "DROP KEYSPACE $KEYSPACE"
fi
log "recreating schema"
cql <"$archive/schema.cql" >/dev/null

# 2. SSTables.
stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
tar -xzf "$archive/scylla.tar.gz" -C "$stage"
for tdir in "$stage/$KEYSPACE"/*/; do
  [[ -d "$tdir" ]] || continue
  tdir=${tdir%/}
  old=$(basename "$tdir")
  table=${old%-*}
  snap=$(find "$tdir/snapshots" -mindepth 1 -maxdepth 1 -type d | head -n1)
  [[ -n "$snap" ]] || continue
  # skip tables with no sstables
  ls "$snap" | grep -q -- '-Data.db$' || { log "table $table: no data, skipped"; continue; }
  # The live directory is <table>-<table id without dashes>; stale directories of
  # dropped tables (kept by Scylla for their auto-snapshots) must not be picked.
  tid=$(cql -e "SELECT id FROM system_schema.tables WHERE keyspace_name = '$KEYSPACE' AND table_name = '$table'" | grep -Eo '[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}' | head -n1 | tr -d '-')
  [[ -n "$tid" ]] || die "table $table does not exist after recreating the schema"
  newdir="$DATA_DIR/$KEYSPACE/$table-$tid"
  docker exec "$container" test -d "$newdir" || die "data directory $newdir not found"
  log "table $table: loading $(ls "$snap" | grep -c -- '-Data.db$') sstable(s) into $(basename "$newdir")"
  up="$stage/upload-$table"
  rm -rf "$up"; mkdir -p "$up"
  find "$snap" -maxdepth 1 -type f ! -name manifest.json ! -name schema.cql -exec cp {} "$up"/ \;
  docker cp "$up/." "$container:$newdir/upload/" >/dev/null
  docker exec -u root "$container" chown -R scylla:scylla "$newdir/upload"
  docker exec "$container" nodetool refresh -- "$KEYSPACE" "$table" >/dev/null
done

# 3. Object storage.
if [[ "$do_s3" == 1 && "${BACKUP_S3_MODE:-rclone}" != none ]]; then
  : "${THEHIDEOUT_S3_ENDPOINT:?}" "${THEHIDEOUT_S3_BUCKET:?}" "${THEHIDEOUT_S3_ACCESS_KEY:?}" "${THEHIDEOUT_S3_SECRET_KEY:?}"
  src_args=(/src); mount_args=(-v "$archive/s3:/src:ro")
  if [[ -n "${BACKUP_S3_DEST:-}" ]]; then src_args=("$BACKUP_S3_DEST"); mount_args=(); fi
  if [[ -n "${BACKUP_S3_DEST:-}" || -d "$archive/s3" ]]; then
    log "restoring bucket ${THEHIDEOUT_S3_BUCKET} (rclone copy, nothing deleted)"
    docker run --rm --network "$NETWORK" ${mount_args[@]+"${mount_args[@]}"} \
      -e RCLONE_CONFIG_DST_TYPE=s3 -e RCLONE_CONFIG_DST_PROVIDER=Other \
      -e RCLONE_CONFIG_DST_ENDPOINT="$THEHIDEOUT_S3_ENDPOINT" \
      -e RCLONE_CONFIG_DST_REGION="${THEHIDEOUT_S3_REGION:-us-east-1}" \
      -e RCLONE_CONFIG_DST_ACCESS_KEY_ID="$THEHIDEOUT_S3_ACCESS_KEY" \
      -e RCLONE_CONFIG_DST_SECRET_ACCESS_KEY="$THEHIDEOUT_S3_SECRET_KEY" \
      -e RCLONE_CONFIG_DST_FORCE_PATH_STYLE="${THEHIDEOUT_S3_PATH_STYLE:-true}" \
      rclone/rclone:1.68 copy "${src_args[0]}" "DST:${THEHIDEOUT_S3_BUCKET}" --transfers 8 \
      || die "rclone copy failed"
  else
    log "no s3/ directory in the archive: bucket not restored"
  fi
fi

if [[ "${SKIP_SERVER:-0}" != 1 ]]; then
  log "starting the server"
  "${compose[@]}" up -d server
fi
log "done. Verify: log in, open a guild, check recent messages and avatars."
