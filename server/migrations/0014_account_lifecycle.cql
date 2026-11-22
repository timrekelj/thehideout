-- Account lifecycle: anonymizing deletion, suspension, terms acceptance,
-- sliding sessions, data exports.

-- 'deleting' while the background cleanup runs, 'deleted' once it finished.
ALTER TABLE users ADD deletion_state text;
ALTER TABLE users ADD deleted_at timestamp;

-- Accounts whose cleanup still has to run (resumed by the startup sweep).
CREATE TABLE account_deletions (
    kind text,
    user_id bigint,
    PRIMARY KEY (kind, user_id)
);

ALTER TABLE users ADD suspended boolean;
ALTER TABLE users ADD suspended_until timestamp;
ALTER TABLE users ADD suspended_reason text;
ALTER TABLE users ADD suspended_at timestamp;

ALTER TABLE users ADD accepted_terms_version text;
ALTER TABLE users ADD accepted_terms_at timestamp;

-- Last time a session's expiry was pushed out (sliding expiry).
ALTER TABLE sessions ADD refreshed_at timestamp;

-- Guilds a user was removed from or left, so their messages there can still
-- be found for export and deletion.
CREATE TABLE user_left_guilds (
    user_id bigint,
    guild_id bigint,
    PRIMARY KEY (user_id, guild_id)
);

-- One export per user per 24 h (the row carries a 24 h TTL).
CREATE TABLE export_requests (
    user_id bigint PRIMARY KEY,
    requested_at timestamp
);

-- Finished exports, by the hash of the download token. Rows expire with the link.
CREATE TABLE data_exports (
    token_hash text PRIMARY KEY,
    user_id bigint,
    object_key text,
    expires_at timestamp
);

-- Index of data_exports for the sweeper that deletes expired objects.
CREATE TABLE data_exports_index (
    kind text,
    token_hash text,
    object_key text,
    expires_at timestamp,
    PRIMARY KEY (kind, token_hash)
);
