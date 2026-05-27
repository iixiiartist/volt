use sqlx::PgPool;
use uuid::Uuid;

pub struct ToolFailureTracker {
    pool: Option<PgPool>,
}

#[derive(Debug, Clone)]
pub struct ToolFailure {
    pub id: i32,
    pub job_id: Option<Uuid>,
    pub tool_name: String,
    pub error: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

impl ToolFailureTracker {
    pub fn new(pool: Option<PgPool>) -> Self {
        Self { pool }
    }

    pub async fn record_failure(
        &self,
        job_id: Uuid,
        tool_name: &str,
        error: &str,
    ) {
        if let Some(ref pool) = self.pool {
            let _ = sqlx::query(
                "INSERT INTO tool_failures (job_id, tool_name, error) VALUES ($1, $2, $3)"
            )
            .bind(job_id)
            .bind(tool_name)
            .bind(error)
            .execute(pool)
            .await;
        }
    }

    pub async fn recent_failures(
        &self,
        tool_name: &str,
        window_secs: i64,
    ) -> i64 {
        if let Some(ref pool) = self.pool {
            let row: (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM tool_failures WHERE tool_name = $1 AND occurred_at > NOW() - INTERVAL '1 second' * $2"
            )
            .bind(tool_name)
            .bind(window_secs)
            .fetch_one(pool)
            .await
            .unwrap_or((0,));
            row.0
        } else {
            0
        }
    }

    pub async fn should_avoid(&self, tool_name: &str) -> Option<String> {
        if std::env::var("VOLT_FAILURE_TRACKING").ok().as_deref() == Some("false") {
            return None;
        }
        let count = self.recent_failures(tool_name, 600).await; // 10-min window
        let threshold: i64 = std::env::var("VOLT_FAILURE_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        if count >= threshold {
            Some(format!(
                "Tool '{}' failed {} times recently; consider an alternative.",
                tool_name, count
            ))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_avoid_disabled() {
        std::env::set_var("VOLT_FAILURE_TRACKING", "false");
        let tracker = ToolFailureTracker::new(None);
        assert_eq!(
            futures::executor::block_on(async { tracker.should_avoid("web_search").await }),
            None
        );
    }
}
