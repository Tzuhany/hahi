-- Memory schema: stores persistent memories for the agent.
-- Uses pgvector for semantic similarity search on reference-type memories.

CREATE SCHEMA IF NOT EXISTS memory;

CREATE TABLE memory.memories (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     TEXT NOT NULL,
    project_id  TEXT,
    memory_type TEXT NOT NULL CHECK (memory_type IN ('user', 'feedback', 'project', 'reference')),
    name        TEXT NOT NULL,
    description TEXT NOT NULL,
    content     TEXT NOT NULL,
    embedding   vector(1536),      -- pgvector: cosine similarity for semantic recall
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for unconditional recall (user + feedback types).
CREATE INDEX idx_memories_user_type ON memory.memories (user_id, memory_type);

-- Index for time-filtered recall (project type, recent entries).
CREATE INDEX idx_memories_user_updated ON memory.memories (user_id, updated_at DESC);

-- pgvector index for semantic recall (reference type).
-- IVFFlat with 100 lists is appropriate for up to ~100k memories per user.
-- For larger datasets, consider HNSW index instead.
CREATE INDEX idx_memories_embedding ON memory.memories
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);

-- Update trigger to keep updated_at current.
CREATE OR REPLACE FUNCTION memory.update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER memories_updated_at
    BEFORE UPDATE ON memory.memories
    FOR EACH ROW EXECUTE FUNCTION memory.update_updated_at();
