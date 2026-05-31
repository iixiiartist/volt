use crate::jobs::JobManager;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

pub struct SelfRepairMonitor {
    manager: Arc<JobManager>,
    check_interval: Duration,
    timeout: Duration,
}

impl SelfRepairMonitor {
    pub fn new(manager: Arc<JobManager>, check_interval: Duration, timeout: Duration) -> Self {
        Self {
            manager,
            check_interval,
            timeout,
        }
    }

    pub async fn run(&self, shutdown: tokio::sync::watch::Receiver<bool>) {
        let mut ticker = interval(self.check_interval);
        loop {
            ticker.tick().await;
            if *shutdown.borrow() {
                tracing::info!("[self-repair] shutting down");
                break;
            }
            match self
                .manager
                .transition_to_stuck(self.timeout.as_secs() as i64)
                .await
            {
                Ok(stuck_ids) => {
                    if !stuck_ids.is_empty() {
                        tracing::warn!(
                            "[self-repair] {} jobs transitioned to Stuck",
                            stuck_ids.len()
                        );
                        for id in stuck_ids {
                            match self.manager.retry_job(id).await {
                                Ok(true) => {
                                    tracing::info!("[self-repair] job {} requeued for retry", id)
                                }
                                Ok(false) => {
                                    tracing::error!(
                                        "[self-repair] job {} exhausted retries, marking Failed",
                                        id
                                    );
                                    if let Err(e) = self
                                        .manager
                                        .fail_job(id, "exhausted retries after stuck detection")
                                        .await
                                    {
                                        tracing::warn!(
                                            "[self-repair] fail_job failed for {}: {}",
                                            id,
                                            e
                                        );
                                    }
                                }
                                Err(e) => tracing::error!(
                                    "[self-repair] retry failed for job {}: {}",
                                    id,
                                    e
                                ),
                            }
                        }
                    }
                }
                Err(e) => tracing::error!("[self-repair] error checking stuck jobs: {}", e),
            }
        }
    }
}
