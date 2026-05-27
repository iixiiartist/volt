use std::sync::Arc;
use std::time::Duration;
use volt::jobs::JobManager;
use volt::heartbeat::Heartbeat;
use volt::jobs::monitor::SelfRepairMonitor;
use volt::routines::engine::RoutineEngine;

#[tokio::test]
async fn test_heartbeat_instantiation() {
    let manager = Arc::new(JobManager::new(None));
    let _hb = Heartbeat::new(Duration::from_secs(60), manager, None);
    assert!(true);
}

#[tokio::test]
async fn test_jobs_monitor_instantiation() {
    let manager = Arc::new(JobManager::new(None));
    let _monitor = SelfRepairMonitor::new(manager, Duration::from_secs(30), Duration::from_secs(300));
    assert!(true);
}

#[tokio::test]
async fn test_routines_engine_instantiation() {
    let manager = Arc::new(JobManager::new(None));
    let _engine = RoutineEngine::new(None, manager, Duration::from_secs(60));
    assert!(true);
}