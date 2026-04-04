CREATE TABLE IF NOT EXISTS runs (
    id                   TEXT        PRIMARY KEY,
    thread_id            TEXT        NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    status               TEXT        NOT NULL DEFAULT 'pending',
    agent_id             TEXT,
    started_at           TIMESTAMPTZ,
    ended_at             TIMESTAMPTZ,
    result_message_id    TEXT,
    failure_reason       TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS runs_thread_id_idx ON runs (thread_id, created_at DESC);
CREATE INDEX IF NOT EXISTS runs_status_idx    ON runs (status) WHERE status IN ('pending', 'running', 'completing');
