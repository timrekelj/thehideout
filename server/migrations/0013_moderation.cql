-- Moderation. `guild_bans`: one row per banned user and guild (a banned user
-- cannot rejoin with an invite code). `message_nonces`: short-lived markers
-- (written with a TTL) that deduplicate a retried SendMessage with the same
-- nonce.
CREATE TABLE guild_bans (
    guild_id bigint,
    user_id bigint,
    banned_by bigint,
    reason text,
    created_at timestamp,
    PRIMARY KEY (guild_id, user_id)
);

CREATE TABLE message_nonces (
    user_id bigint,
    channel_id bigint,
    nonce text,
    message_id bigint,
    PRIMARY KEY ((user_id, channel_id), nonce)
);
