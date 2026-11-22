-- Push notifications: devices (rows carry a 30 day TTL, renewed whenever the
-- app registers again) and notification settings.

CREATE TABLE push_devices (
    user_id bigint,
    id bigint,
    session_hash text,
    platform text,
    push_token text,
    app_version text,
    device_name text,
    locale text,
    sandbox boolean,
    created_at timestamp,
    updated_at timestamp,
    PRIMARY KEY (user_id, id)
);

-- The device currently holding a token (a token belongs to one device).
CREATE TABLE push_devices_by_token (
    platform text,
    push_token text,
    user_id bigint,
    device_id bigint,
    PRIMARY KEY ((platform, push_token))
);

-- Devices of a session, removed when the session is revoked.
CREATE TABLE push_devices_by_session (
    session_hash text,
    device_id bigint,
    user_id bigint,
    PRIMARY KEY (session_hash, device_id)
);

CREATE TABLE notification_settings (
    user_id bigint PRIMARY KEY,
    dms_enabled boolean,
    guild_default text,
    show_previews boolean,
    updated_at timestamp
);

-- scope: 'guild' or 'channel'; level: all | mentions | nothing | inherit.
CREATE TABLE notification_overrides (
    user_id bigint,
    scope text,
    scope_id bigint,
    level text,
    muted_until timestamp,
    PRIMARY KEY (user_id, scope, scope_id)
);
