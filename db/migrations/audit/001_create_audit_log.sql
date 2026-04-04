-- Execution audit trail: records every significant agent action.

CREATE TABLE IF NOT EXISTS audit_log (
    id          BIGSERIAL PRIMARY KEY,
    thread_id   TEXT NOT NULL,
    action      JSONB NOT NULL,
    timestamp   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_audit_thread_time ON audit_log (thread_id, timestamp DESC);

-- Auto-cleanup: partition by month or use pg_cron to drop old entries.
