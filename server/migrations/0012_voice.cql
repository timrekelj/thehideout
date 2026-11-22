-- Voice channels. `kind` is "text" or "voice", set at creation and never
-- changed. Channels created before this migration have it null: they are text.
ALTER TABLE channels ADD kind text;
