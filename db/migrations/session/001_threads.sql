CREATE TABLE IF NOT EXISTS threads (
    id         TEXT        PRIMARY KEY,
    user_id    TEXT        NOT NULL,
    title      TEXT        NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS threads_user_id_idx ON threads (user_id, created_at DESC);
