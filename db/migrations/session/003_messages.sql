CREATE TABLE IF NOT EXISTS messages (
    id          TEXT        PRIMARY KEY,
    thread_id   TEXT        NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    role        TEXT        NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS messages_thread_id_idx ON messages (thread_id, created_at ASC);
