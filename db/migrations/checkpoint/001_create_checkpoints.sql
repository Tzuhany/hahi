-- Checkpoint table: agent's conversation snapshots.
-- Indexed by thread_id (owned by Conversation module, used as key here).

CREATE TABLE IF NOT EXISTS checkpoints (
    thread_id   TEXT PRIMARY KEY,
    data        BYTEA NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
