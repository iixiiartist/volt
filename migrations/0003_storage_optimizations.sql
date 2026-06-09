-- Storage optimization migration (0003) — fixed 2026-06-09
--
-- Original 0003 used PascalCase ('Tool', 'Skill', 'Memory') in the
-- partial index WHERE clauses, but the runtime stores lowercase
-- ('tool', 'skill', 'memory') via ContextKind::as_str(). The partial
-- indexes therefore never matched any row — every query fell back to
-- a sequential scan once context_entries exceeded ~10k rows.
--
-- This migration drops the broken partial indexes and recreates them
-- with the correct lowercase kind values.

-- Remove the monolithic index that compared vectors across unrelated boundaries.
DROP INDEX IF EXISTS context_entries_embedding_idx;

-- Drop the broken partial indexes from the original 0003.
DROP INDEX IF EXISTS idx_ctx_tools;
DROP INDEX IF EXISTS idx_ctx_skills;
DROP INDEX IF EXISTS idx_ctx_memories;

-- Recreate with correct lowercase kind values matching ContextKind::as_str().
CREATE INDEX IF NOT EXISTS idx_ctx_tools
  ON context_entries USING hnsw (embedding vector_cosine_ops)
  WHERE kind = 'tool';

CREATE INDEX IF NOT EXISTS idx_ctx_skills
  ON context_entries USING hnsw (embedding vector_cosine_ops)
  WHERE kind = 'skill';

CREATE INDEX IF NOT EXISTS idx_ctx_memories
  ON context_entries USING hnsw (embedding vector_cosine_ops)
  WHERE kind = 'memory';

-- B-tree on kind already exists from 0001_core.sql, but kept here
-- for idempotency on fresh installs that skip straight to 0003.
CREATE INDEX IF NOT EXISTS idx_ctx_kind ON context_entries(kind);

-- Record that this migration was applied.
INSERT INTO schema_version (version) VALUES (3) ON CONFLICT DO NOTHING;
