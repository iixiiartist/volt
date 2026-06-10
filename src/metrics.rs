use std::sync::atomic::{AtomicI64, Ordering};

static CONTEXT_ENTRIES_TOTAL: AtomicI64 = AtomicI64::new(0);
static CONTEXT_ENTRIES_BY_KIND: [AtomicI64; 3] = [
    AtomicI64::new(0), // Tool
    AtomicI64::new(0), // Memory
    AtomicI64::new(0), // Conversation
];
static QUERIES_TOTAL: AtomicI64 = AtomicI64::new(0);
static QUERIES_HITS: AtomicI64 = AtomicI64::new(0);
static QUERIES_MISSES: AtomicI64 = AtomicI64::new(0);
static TOOL_CALLS_TOTAL: AtomicI64 = AtomicI64::new(0);
static TOOL_CALLS_ERRORS: AtomicI64 = AtomicI64::new(0);
static EMBEDDING_CALLS: AtomicI64 = AtomicI64::new(0);
static AUDIT_LOG_SIZE: AtomicI64 = AtomicI64::new(0);

pub fn inc_context_entries(kind_idx: usize) {
    CONTEXT_ENTRIES_TOTAL.fetch_add(1, Ordering::Relaxed);
    if kind_idx < 3 {
        CONTEXT_ENTRIES_BY_KIND[kind_idx].fetch_add(1, Ordering::Relaxed);
    }
}

pub fn inc_queries() {
    QUERIES_TOTAL.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_hits() {
    QUERIES_HITS.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_misses() {
    QUERIES_MISSES.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_tool_calls() {
    TOOL_CALLS_TOTAL.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_tool_errors() {
    TOOL_CALLS_ERRORS.fetch_add(1, Ordering::Relaxed);
}
pub fn inc_embedding_calls() {
    EMBEDDING_CALLS.fetch_add(1, Ordering::Relaxed);
}
pub fn set_audit_log_size(n: i64) {
    AUDIT_LOG_SIZE.store(n, Ordering::Relaxed);
}

pub fn render_prometheus() -> String {
    let empty = String::new();
    vec![
        "# HELP volt_context_entries_total Total context entries across all kinds".to_string(),
        "# TYPE volt_context_entries_total gauge".to_string(),
        format!(
            "volt_context_entries_total {}",
            CONTEXT_ENTRIES_TOTAL.load(Ordering::Relaxed)
        ),
        empty.clone(),
        "# HELP volt_context_entries_by_kind Context entries per kind".to_string(),
        "# TYPE volt_context_entries_by_kind gauge".to_string(),
        format!(
            "volt_context_entries_by_kind{{kind=\"tool\"}} {}",
            CONTEXT_ENTRIES_BY_KIND[0].load(Ordering::Relaxed)
        ),
        format!(
            "volt_context_entries_by_kind{{kind=\"memory\"}} {}",
            CONTEXT_ENTRIES_BY_KIND[1].load(Ordering::Relaxed)
        ),
        format!(
            "volt_context_entries_by_kind{{kind=\"conversation\"}} {}",
            CONTEXT_ENTRIES_BY_KIND[2].load(Ordering::Relaxed)
        ),
        empty.clone(),
        "# HELP volt_queries_total Total context store queries".to_string(),
        "# TYPE volt_queries_total counter".to_string(),
        format!(
            "volt_queries_total {}",
            QUERIES_TOTAL.load(Ordering::Relaxed)
        ),
        "# HELP volt_queries_hits_total Query hits".to_string(),
        "# TYPE volt_queries_hits_total counter".to_string(),
        format!(
            "volt_queries_hits_total {}",
            QUERIES_HITS.load(Ordering::Relaxed)
        ),
        "# HELP volt_queries_misses_total Query misses".to_string(),
        "# TYPE volt_queries_misses_total counter".to_string(),
        format!(
            "volt_queries_misses_total {}",
            QUERIES_MISSES.load(Ordering::Relaxed)
        ),
        empty.clone(),
        "# HELP volt_tool_calls_total Total tool calls".to_string(),
        "# TYPE volt_tool_calls_total counter".to_string(),
        format!(
            "volt_tool_calls_total {}",
            TOOL_CALLS_TOTAL.load(Ordering::Relaxed)
        ),
        "# HELP volt_tool_calls_errors_total Tool call errors".to_string(),
        "# TYPE volt_tool_calls_errors_total counter".to_string(),
        format!(
            "volt_tool_calls_errors_total {}",
            TOOL_CALLS_ERRORS.load(Ordering::Relaxed)
        ),
        empty.clone(),
        "# HELP volt_embedding_calls_total Total embedding API calls".to_string(),
        "# TYPE volt_embedding_calls_total counter".to_string(),
        format!(
            "volt_embedding_calls_total {}",
            EMBEDDING_CALLS.load(Ordering::Relaxed)
        ),
        empty.clone(),
        "# HELP volt_audit_log_size Current in-memory audit log entries".to_string(),
        "# TYPE volt_audit_log_size gauge".to_string(),
        format!(
            "volt_audit_log_size {}",
            AUDIT_LOG_SIZE.load(Ordering::Relaxed)
        ),
    ]
    .join("\n")
}
