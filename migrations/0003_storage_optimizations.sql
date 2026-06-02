-- Storage optimization migration (0003)
-- Applies partial HNSW indexes to context_entries for the most heavily
-- queried RAG kinds, preventing cross-kind vector comparisons.
--
-- Also drops the generic HNSW index in favor of kind-filtered partial indexes.

-- Remove the monolithic index that compared vectors across unrelated boundaries.
DROP INDEX IF EXISTS context_entries_embedding_idx;

-- Partial HNSW indexes for high-traffic context kinds.
-- PostgreSQL will use these automatically when queries filter by kind.
CREATE INDEX IF NOT EXISTS idx_ctx_tools
  ON context_entries USING hnsw (embedding vector_cosine_ops)
  WHERE kind = 'Tool';

CREATE INDEX IF NOT EXISTS idx_ctx_skills
  ON context_entries USING hnsw (embedding vector_cosine_ops)
  WHERE kind = 'Skill';

CREATE INDEX IF NOT EXISTS idx_ctx_memories
  ON context_entries USING hnsw (embedding vector_cosine_ops)
  WHERE kind = 'Memory';

-- B-tree on kind already exists from 0001_core.sql, but kept here
-- for idempotency on fresh installs that skip straight to 0003.
CREATE INDEX IF NOT EXISTS idx_ctx_kind ON context_entries(kind);
