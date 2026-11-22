#!/bin/bash
# One-shot Scylla role bootstrap, run by the `scylla-init` compose service.
#
# A fresh Scylla with PasswordAuthenticator has one superuser, cassandra /
# cassandra (created a short while after startup). This script:
#   1. waits until it can log in (default password, or the configured one),
#   2. creates / updates the application role (THEHIDEOUT_SCYLLA_USERNAME) with
#      the right to create and use keyspaces (the server runs its migrations as
#      this role, which includes CREATE KEYSPACE),
#   3. replaces the default superuser password.
# Safe to re-run; re-running re-applies the passwords from the env file.
set -euo pipefail

HOST=scylla
APP_USER=${THEHIDEOUT_SCYLLA_USERNAME}
APP_PASS=${THEHIDEOUT_SCYLLA_PASSWORD}
SU_PASS=${SCYLLA_SUPERUSER_PASSWORD}

for v in "$APP_USER" "$APP_PASS" "$SU_PASS"; do
  # Values are interpolated into CQL: keep them to a safe alphabet.
  if [[ ! "$v" =~ ^[A-Za-z0-9_.@+=-]+$ ]]; then
    echo "scylla-init: credentials may only contain letters, digits and _.@+=- (use: openssl rand -hex 32)" >&2
    exit 1
  fi
done
if [[ "$APP_USER" == "cassandra" ]]; then
  echo "scylla-init: THEHIDEOUT_SCYLLA_USERNAME must not be the superuser 'cassandra'" >&2
  exit 1
fi

cql() { # cql <password> <statement>
  cqlsh "$HOST" 9042 -u cassandra -p "$1" -e "$2" >/dev/null 2>&1
}

pass=""
for i in $(seq 1 60); do
  if cql "$SU_PASS" "SELECT now() FROM system.local"; then pass=$SU_PASS; break; fi
  if cql cassandra "SELECT now() FROM system.local"; then pass=cassandra; break; fi
  echo "scylla-init: waiting for the superuser role ($i/60)"
  sleep 5
done
if [[ -z "$pass" ]]; then
  echo "scylla-init: could not log in as cassandra with the default or the configured password" >&2
  exit 1
fi

cqlsh "$HOST" 9042 -u cassandra -p "$pass" <<CQL
CREATE ROLE IF NOT EXISTS '${APP_USER}' WITH PASSWORD = '${APP_PASS}' AND LOGIN = true AND SUPERUSER = false;
ALTER ROLE '${APP_USER}' WITH PASSWORD = '${APP_PASS}';
GRANT ALL PERMISSIONS ON ALL KEYSPACES TO '${APP_USER}';
ALTER ROLE cassandra WITH PASSWORD = '${SU_PASS}';
CQL
echo "scylla-init: role '${APP_USER}' ready, superuser password set"
