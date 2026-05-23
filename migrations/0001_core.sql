-- Volt core persistence schema.
-- Requires PostgreSQL with pgvector.

CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS agent_tools (
  id SERIAL PRIMARY KEY,
  tool_name VARCHAR(255) UNIQUE NOT NULL,
  description TEXT NOT NULL,
  language VARCHAR(50) NOT NULL,
  source_code TEXT NOT NULL,
  parameter_schema JSONB NOT NULL DEFAULT '{"type":"object"}'::jsonb,
  embedding vector(1024),
  is_marketplace_verified BOOLEAN DEFAULT false,
  cryptographic_signature VARCHAR(512),
  source_sha256 VARCHAR(64) NOT NULL,
  manifest JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS asset_relationships (
  parent_id INT REFERENCES agent_tools(id) ON DELETE CASCADE,
  child_id INT REFERENCES agent_tools(id) ON DELETE CASCADE,
  relationship_type VARCHAR(100) NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (parent_id, child_id, relationship_type)
);

CREATE TABLE IF NOT EXISTS tool_executions (
  id BIGSERIAL PRIMARY KEY,
  tool_id INT REFERENCES agent_tools(id) ON DELETE SET NULL,
  tool_name VARCHAR(255) NOT NULL,
  input JSONB NOT NULL DEFAULT '{}'::jsonb,
  output JSONB NOT NULL DEFAULT '{}'::jsonb,
  status VARCHAR(40) NOT NULL,
  error TEXT,
  duration_ms INT NOT NULL DEFAULT 0,
  execution_id UUID NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS registry_events (
  id BIGSERIAL PRIMARY KEY,
  pkg_id VARCHAR(255) NOT NULL,
  tool_name VARCHAR(255),
  event_type VARCHAR(80) NOT NULL,
  status VARCHAR(40) NOT NULL,
  details JSONB NOT NULL DEFAULT '{}'::jsonb,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS agent_tools_embedding_idx
  ON agent_tools USING hnsw (embedding vector_cosine_ops);

CREATE INDEX IF NOT EXISTS agent_tools_language_idx ON agent_tools(language);
CREATE INDEX IF NOT EXISTS tool_executions_tool_name_idx ON tool_executions(tool_name);
CREATE INDEX IF NOT EXISTS tool_executions_created_at_idx ON tool_executions(created_at DESC);
CREATE INDEX IF NOT EXISTS registry_events_pkg_id_idx ON registry_events(pkg_id);

CREATE TABLE IF NOT EXISTS memories (
  id BIGSERIAL PRIMARY KEY,
  kind VARCHAR(100) NOT NULL DEFAULT 'general',
  content TEXT NOT NULL,
  embedding vector(1024),
  session_id UUID,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS memories_kind_idx ON memories(kind);
CREATE INDEX IF NOT EXISTS memories_embedding_idx
  ON memories USING hnsw (embedding vector_cosine_ops);

CREATE TABLE IF NOT EXISTS skills (
  id UUID PRIMARY KEY,
  name VARCHAR(255) UNIQUE NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  version VARCHAR(50) NOT NULL DEFAULT '1.0.0',
  content TEXT NOT NULL DEFAULT '',
  embedding vector(1024),
  mcp_servers JSONB NOT NULL DEFAULT '[]'::jsonb,
  source_path VARCHAR(1024),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS skills_name_idx ON skills(name);
CREATE INDEX IF NOT EXISTS skills_embedding_idx
  ON skills USING hnsw (embedding vector_cosine_ops);

CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at = NOW();
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS set_agent_tools_updated_at ON agent_tools;
CREATE TRIGGER set_agent_tools_updated_at
BEFORE UPDATE ON agent_tools
FOR EACH ROW EXECUTE FUNCTION set_updated_at();