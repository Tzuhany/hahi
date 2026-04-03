-- Tool result persistence: stores large tool outputs externally.
-- Messages carry truncated previews; full content lives here.

CREATE TABLE IF NOT EXISTS tool_results (
    tool_use_id  TEXT PRIMARY KEY,
    thread_id    TEXT NOT NULL,
    content      TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tool_results_thread ON tool_results (thread_id);

-- Cleanup: auto-delete after 7 days (tool results are ephemeral).
-- In production, use pg_cron or a background job.
