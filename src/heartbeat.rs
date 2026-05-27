use crate::jobs::JobManager;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

pub struct Heartbeat {
    interval: Duration,
    job_manager: Arc<JobManager>,
    workspace: Option<PathBuf>,
}

impl Heartbeat {
    pub fn new(
        interval: Duration,
        job_manager: Arc<JobManager>,
        workspace: Option<PathBuf>,
    ) -> Self {
        Self { interval, job_manager, workspace }
    }

    pub async fn run(&self,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        let mut ticker = interval(self.interval);
        loop {
            ticker.tick().await;
            if *shutdown.borrow() {
                tracing::info!("[heartbeat] shutting down");
                break;
            }
            let path = match &self.workspace {
                Some(w) => w.join("HEARTBEAT.md"),
                None => PathBuf::from("HEARTBEAT.md"),
            };
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if !content.trim().is_empty() {
                    match self.job_manager.create_job(
                        &content,
                        serde_json::json!({"source": "heartbeat", "file": path.to_string_lossy()})
                    ).await {
                        Ok(id) => tracing::info!("[heartbeat] created job {} from HEARTBEAT.md", id),
                        Err(e) => tracing::warn!("[heartbeat] failed to create job: {}", e),
                    }
                }
            }
        }
    }
}
