use crate::jobs::JobManager;
use chrono::{DateTime, Utc};
use cron_parser::parse;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use uuid::Uuid;

pub struct RoutineEngine {
    pool: Option<PgPool>,
    job_manager: Arc<JobManager>,
    check_interval: Duration,
}

impl RoutineEngine {
    pub fn new(
        pool: Option<PgPool>,
        job_manager: Arc<JobManager>,
        check_interval: Duration,
    ) -> Self {
        Self {
            pool,
            job_manager,
            check_interval,
        }
    }

    pub async fn run(&self, shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut ticker = interval(self.check_interval);
        loop {
            ticker.tick().await;
            if *shutdown.borrow() {
                tracing::info!("[routines] shutting down");
                break;
            }
            if let Some(ref pool) = self.pool {
                match self.fire_due_routines(pool).await {
                    Ok(count) => {
                        if count > 0 {
                            tracing::info!("[routines] fired {} routines", count);
                        }
                    }
                    Err(e) => tracing::error!("[routines] error firing routines: {}", e),
                }
            }
        }
    }

    async fn fire_due_routines(&self, pool: &PgPool) -> anyhow::Result<usize> {
        let rows: Vec<RoutineRow> = sqlx::query_as(
            "SELECT id, name, cron, action_prompt, last_run, next_run, trigger_type, trigger_config, guardrails_max_tokens, guardrails_max_tool_calls, guardrails_allowed_tools, guardrails_timeout_secs FROM routines
             WHERE enabled = true AND next_run IS NOT NULL AND next_run <= NOW()"
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut fired = 0;
        for row in rows {
            let id = row.id;
            let action = row.action_prompt;
            let guardrails = serde_json::json!({"max_tokens": row.guardrails_max_tokens,"max_tool_calls": row.guardrails_max_tool_calls,"allowed_tools": row.guardrails_allowed_tools,"timeout_secs": row.guardrails_timeout_secs});
            let context = serde_json::json!({"routine_id": id,"routine_name": &row.name,"trigger_type": &row.trigger_type,"trigger_config": &row.trigger_config,"last_run": row.last_run,"next_run": row.next_run,"guardrails": guardrails});
            let _ = self.job_manager.create_job(&action, context).await;
            tracing::info!("[routines] fired routine '{}' ({})", row.name, id);
            sqlx::query("UPDATE routines SET last_run = NOW(), next_run = NULL WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await?;
            if let Some(ref cron) = row.cron {
                if let Ok(next) = parse(cron, Utc::now()) {
                    sqlx::query("UPDATE routines SET next_run = $1 WHERE id = $2")
                        .bind(next)
                        .bind(id)
                        .execute(pool)
                        .await?;
                }
            }
            fired += 1;
        }
        Ok(fired)
    }
}

#[derive(sqlx::FromRow)]
struct RoutineRow {
    id: Uuid,
    name: String,
    cron: Option<String>,
    action_prompt: String,
    last_run: Option<DateTime<Utc>>,
    next_run: Option<DateTime<Utc>>,
    trigger_type: String,
    trigger_config: Option<serde_json::Value>,
    guardrails_max_tokens: Option<i64>,
    guardrails_max_tool_calls: Option<i32>,
    guardrails_allowed_tools: Option<Vec<String>>,
    guardrails_timeout_secs: Option<i64>,
}
