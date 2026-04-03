-- Skill schema: stores user/platform-created skills.
-- Bundled skills are compiled into the agent binary — they don't live here.

CREATE SCHEMA IF NOT EXISTS skill;

CREATE TABLE skill.skills (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    when_to_use TEXT NOT NULL DEFAULT '',
    content     TEXT NOT NULL,
    mode        TEXT NOT NULL DEFAULT 'inline' CHECK (mode IN ('inline', 'forked')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_skills_name ON skill.skills (name);

CREATE TRIGGER skills_updated_at
    BEFORE UPDATE ON skill.skills
    FOR EACH ROW EXECUTE FUNCTION memory.update_updated_at();
