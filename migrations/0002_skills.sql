-- Volt skills schema: compiled manifest pattern.
-- Skills are authored as SKILL.md, compiled into DB entities via 'volt provision --path'.

CREATE TABLE IF NOT EXISTS skills (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '1.0.0',
    content TEXT NOT NULL,
    embedding vector(1024),
    mcp_servers TEXT NOT NULL DEFAULT '[]',
    source_path TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS skill_tools (
    id UUID PRIMARY KEY,
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    input_schema JSONB,
    requires_sandbox BOOLEAN NOT NULL DEFAULT false,
    mcp_server TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS skills_embedding_idx
    ON skills USING hnsw (embedding vector_cosine_ops);

CREATE INDEX IF NOT EXISTS skills_name_idx ON skills(name);
CREATE INDEX IF NOT EXISTS skill_tools_skill_id_idx ON skill_tools(skill_id);

CREATE OR REPLACE TRIGGER set_skills_updated_at
    BEFORE UPDATE ON skills
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();