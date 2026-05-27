use volt::tool_failure_tracker::{ToolFailureTracker};

#[test]
fn test_disabled_returns_none() {
    std::env::set_var("VOLT_FAILURE_TRACKING", "false");
    let tracker = ToolFailureTracker::new(None);
    assert_eq!(
        futures::executor::block_on(async { tracker.should_avoid("web_search").await }),
        None
    );
}

#[test]
fn test_no_pool_returns_none() {
    std::env::remove_var("VOLT_FAILURE_TRACKING");
    let tracker = ToolFailureTracker::new(None);
    assert_eq!(
        futures::executor::block_on(async { tracker.should_avoid("web_search").await }),
        None
    );
}

#[test]
fn test_disabled_tracking_env() {
    std::env::set_var("VOLT_FAILURE_TRACKING", "false");
    let tracker = ToolFailureTracker::new(None);
    let result = futures::executor::block_on(async { tracker.should_avoid("tool".into()).await });
    assert!(result.is_none());
}

#[test]
fn test_no_pool_no_tracking() {
    std::env::remove_var("VOLT_FAILURE_TRACKING");
    let tracker = ToolFailureTracker::new(None);
    let result = futures::executor::block_on(async { tracker.should_avoid("tool".into()).await });
    assert!(result.is_none());
}
