use crate::heartbeat::Heartbeat;
use crate::jobs::monitor::SelfRepairMonitor;
use crate::jobs::JobManager;
use crate::routines::engine::RoutineEngine;
use std::sync::Arc;
use std::time::Duration;

pub async fn run_heartbeat(settings: &crate::config::Settings) -> anyhow::Result<()> {
    let pool = match crate::db::connect(&settings.database_url).await {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!("[daemon] no DB connection: {}", e);
            None
        }
    };
    let manager = Arc::new(JobManager::new(pool));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let heartbeat = Heartbeat::new(
        Duration::from_secs(60),
        manager,
        std::env::current_dir().ok(),
    );
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = tx.send(true);
    });
    heartbeat.run(rx).await;
    Ok(())
}

pub async fn run_jobs_monitor(settings: &crate::config::Settings) -> anyhow::Result<()> {
    let pool = match crate::db::connect(&settings.database_url).await {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!("[daemon] no DB connection: {}", e);
            None
        }
    };
    let manager = Arc::new(JobManager::new(pool));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let monitor =
        SelfRepairMonitor::new(manager, Duration::from_secs(30), Duration::from_secs(300));
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = tx.send(true);
    });
    monitor.run(rx).await;
    Ok(())
}

pub async fn run_routines_engine(settings: &crate::config::Settings) -> anyhow::Result<()> {
    let pool = match crate::db::connect(&settings.database_url).await {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!("[daemon] no DB connection: {}", e);
            None
        }
    };
    let manager = Arc::new(JobManager::new(pool.clone()));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let engine = RoutineEngine::new(pool, manager, Duration::from_secs(60));
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        let _ = tx.send(true);
    });
    engine.run(rx).await;
    Ok(())
}
