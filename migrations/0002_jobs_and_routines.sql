-- Phase 3: Jobs, Routines, Secrets, and Tool Failure Tracking

CREATE TABLE IF NOT EXISTS jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    description TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'Pending',
    context JSONB,
    parent_job_id UUID REFERENCES jobs(id),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    worker_id TEXT,
    last_activity_at TIMESTAMPTZ DEFAULT NOW(),
    attempt_count INT DEFAULT 0,
    max_attempts INT DEFAULT 3,
    output TEXT
);

CREATE INDEX IF NOT EXISTS idx_jobs_state ON jobs(state);
CREATE INDEX IF NOT EXISTS idx_jobs_parent ON jobs(parent_job_id);

CREATE TABLE IF NOT EXISTS routines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT UNIQUE NOT NULL,
    cron TEXT,
    action_prompt TEXT NOT NULL,
    enabled BOOLEAN DEFAULT true,
    last_run TIMESTAMPTZ,
    next_run TIMESTAMPTZ,
    trigger_type TEXT DEFAULT 'cron',
    trigger_config JSONB,
    guardrails JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_routines_next_run ON routines(next_run) WHERE enabled = true;

CREATE TABLE IF NOT EXISTS tool_failures (
    id SERIAL PRIMARY KEY,
    job_id UUID,
    tool_name TEXT NOT NULL,
    error TEXT,
    occurred_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tool_failures_tool ON tool_failures(tool_name, occurred_at DESC);

CREATE TABLE IF NOT EXISTS secrets (
    name TEXT PRIMARY KEY,
    encrypted_value BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
