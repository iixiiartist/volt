use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum JobState {
    Pending,
    InProgress,
    Completed,
    Failed,
    Stuck,
    Cancelled,
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobState::Pending => write!(f, "Pending"),
            JobState::InProgress => write!(f, "InProgress"),
            JobState::Completed => write!(f, "Completed"),
            JobState::Failed => write!(f, "Failed"),
            JobState::Stuck => write!(f, "Stuck"),
            JobState::Cancelled => write!(f, "Cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub description: String,
    pub state: JobState,
    pub context: serde_json::Value,
    pub parent_job_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub worker_id: Option<String>,
    pub last_activity_at: DateTime<Utc>,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub output: Option<String>,
}

pub struct JobManager {
    pool: Option<PgPool>,
}

impl JobManager {
    pub fn new(pool: Option<PgPool>) -> Self {
        Self { pool }
    }

    pub async fn create_job(
        &self,
        description: &str,
        context: serde_json::Value,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        if let Some(ref pool) = self.pool {
            sqlx::query(
                "INSERT INTO jobs (id, description, state, context) VALUES ($1, $2, 'Pending', $3)"
            )
            .bind(id)
            .bind(description)
            .bind(context)
            .execute(pool)
            .await?;
        }
        Ok(id)
    }

    pub async fn start_job(&self, id: Uuid, worker_id: Option<&str>) -> anyhow::Result<()> {
        if let Some(ref pool) = self.pool {
            sqlx::query(
                "UPDATE jobs SET state = 'InProgress', worker_id = $2, last_activity_at = NOW() WHERE id = $1"
            )
            .bind(id)
            .bind(worker_id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn complete_job(&self, id: Uuid, output: &str) -> anyhow::Result<()> {
        if let Some(ref pool) = self.pool {
            sqlx::query(
                "UPDATE jobs SET state = 'Completed', output = $2, completed_at = NOW(), updated_at = NOW() WHERE id = $1"
            )
            .bind(id)
            .bind(output)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn fail_job(&self, id: Uuid, error: &str) -> anyhow::Result<()> {
        if let Some(ref pool) = self.pool {
            sqlx::query(
                "UPDATE jobs SET state = 'Failed', output = $2, completed_at = NOW(), updated_at = NOW() WHERE id = $1"
            )
            .bind(id)
            .bind(error)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn cancel_job(&self, id: Uuid) -> anyhow::Result<()> {
        if let Some(ref pool) = self.pool {
            sqlx::query(
                "UPDATE jobs SET state = 'Cancelled', updated_at = NOW() WHERE id = $1"
            )
            .bind(id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn heartbeat(&self, id: Uuid) -> anyhow::Result<()> {
        if let Some(ref pool) = self.pool {
            sqlx::query(
                "UPDATE jobs SET last_activity_at = NOW() WHERE id = $1"
            )
            .bind(id)
            .execute(pool)
            .await?;
        }
        Ok(())
    }

    pub async fn list_jobs(&self, state_filter: Option<&str>) -> anyhow::Result<Vec<Job>> {
        if let Some(ref pool) = self.pool {
            let rows: Vec<JobRow> = match state_filter {
                Some(state) => {
                    sqlx::query_as(
                        "SELECT id, description, state, context, parent_job_id, created_at, updated_at, completed_at, worker_id, last_activity_at, attempt_count, max_attempts, output FROM jobs WHERE state = $1 ORDER BY created_at DESC"
                    )
                    .bind(state)
                    .fetch_all(pool)
                    .await?
                }
                None => {
                    sqlx::query_as(
                        "SELECT id, description, state, context, parent_job_id, created_at, updated_at, completed_at, worker_id, last_activity_at, attempt_count, max_attempts, output FROM jobs ORDER BY created_at DESC"
                    )
                    .fetch_all(pool)
                    .await?
                }
            };
            return Ok(rows.into_iter().map(|r| r.into()).collect());
        }
        Ok(Vec::new())
    }

    pub async fn transition_to_stuck(&self, timeout_secs: i64) -> anyhow::Result<Vec<Uuid>> {
        let mut stuck = Vec::new();
        if let Some(ref pool) = self.pool {
            let rows: Vec<(Uuid,)> = sqlx::query_as(
                "UPDATE jobs SET state = 'Stuck', updated_at = NOW()
                 WHERE state = 'InProgress' AND last_activity_at < NOW() - INTERVAL '1 second' * $1
                 RETURNING id"
            )
            .bind(timeout_secs)
            .fetch_all(pool)
            .await?;
            stuck = rows.into_iter().map(|r| r.0).collect();
        }
        Ok(stuck)
    }

    pub async fn retry_job(&self, id: Uuid) -> anyhow::Result<bool> {
        if let Some(ref pool) = self.pool {
            let row: Option<(i32, i32)> = sqlx::query_as(
                "SELECT attempt_count, max_attempts FROM jobs WHERE id = $1"
            )
            .bind(id)
            .fetch_optional(pool)
            .await?;
            if let Some((attempts, max)) = row {
                if attempts < max {
                    sqlx::query(
                        "UPDATE jobs SET state = 'Pending', attempt_count = attempt_count + 1, updated_at = NOW() WHERE id = $1"
                    )
                    .bind(id)
                    .execute(pool)
                    .await?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: Uuid,
    description: String,
    state: String,
    context: serde_json::Value,
    parent_job_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    worker_id: Option<String>,
    last_activity_at: DateTime<Utc>,
    attempt_count: i32,
    max_attempts: i32,
    output: Option<String>,
}

impl From<JobRow> for Job {
    fn from(row: JobRow) -> Self {
        Job {
            id: row.id,
            description: row.description,
            state: match row.state.as_str() {
                "Pending" => JobState::Pending,
                "InProgress" => JobState::InProgress,
                "Completed" => JobState::Completed,
                "Failed" => JobState::Failed,
                "Stuck" => JobState::Stuck,
                "Cancelled" => JobState::Cancelled,
                _ => JobState::Pending,
            },
            context: row.context,
            parent_job_id: row.parent_job_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            completed_at: row.completed_at,
            worker_id: row.worker_id,
            last_activity_at: row.last_activity_at,
            attempt_count: row.attempt_count,
            max_attempts: row.max_attempts,
            output: row.output,
        }
    }
}
