use crate::session::CheckpointData;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::mpsc;

const FLUSH_INTERVAL_MS: u64 = 100;
const BATCH_FLUSH_SIZE: usize = 32;

/// In-memory checkpoint journal that batches writes to SQLite.
///
/// Agents write checkpoints to an unbounded channel; a single background
/// writer drains the channel every 100ms (or when 32 entries accumulate),
/// deduplicates by (session_id, iteration) keeping the last entry per key,
/// and commits all in one SQLite transaction.
///
/// This eliminates write-lock contention when N parallel agents each save
/// checkpoints on every iteration turn.
#[derive(Clone)]
pub struct CheckpointJournal {
    sender: mpsc::UnboundedSender<CheckpointData>,
}

impl CheckpointJournal {
    pub fn new(pool: SqlitePool) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(Self::writer_loop(pool, rx));
        Arc::new(Self { sender: tx })
    }

    pub async fn push(&self, data: CheckpointData) {
        if self.sender.send(data).is_err() {
            tracing::warn!("[checkpoint_journal] writer loop terminated");
        }
    }

    async fn writer_loop(pool: SqlitePool, mut rx: mpsc::UnboundedReceiver<CheckpointData>) {
        let mut buffer: Vec<CheckpointData> = Vec::new();
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(FLUSH_INTERVAL_MS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                Some(entry) = rx.recv() => {
                    buffer.push(entry);
                    if buffer.len() >= BATCH_FLUSH_SIZE {
                        Self::flush(&pool, &buffer).await;
                        buffer.clear();
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        Self::flush(&pool, &buffer).await;
                        buffer.clear();
                    }
                }
            }
        }
    }

    async fn flush(pool: &SqlitePool, entries: &[CheckpointData]) {
        // Deduplicate by (session_id, iteration), keeping the LAST entry
        let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
        let deduped: Vec<&CheckpointData> = entries
            .iter()
            .rev()
            .filter(|e| seen.insert((e.session_id.to_string(), e.iteration)))
            .collect::<Vec<_>>();
        let deduped: Vec<&CheckpointData> = deduped.into_iter().rev().collect();

        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::warn!("[checkpoint_journal] begin tx failed: {}", e);
                return;
            }
        };

        for entry in &deduped {
            let state_hash = crate::session::compute_state_hash(&entry.messages);
            if let Err(e) = sqlx::query(
                r#"
                INSERT OR REPLACE INTO checkpoints
                    (session_id, iteration, messages_json, token_counts_json, created_at, retry_count, state_hash)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(entry.session_id.to_string())
            .bind(entry.iteration as i64)
            .bind(serde_json::to_string(&entry.messages).unwrap_or_default())
            .bind(serde_json::to_string(&serde_json::json!({
                "prompt": entry.token_prompt,
                "completion": entry.token_completion,
            })).unwrap_or_default())
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(0i64)
            .bind(&state_hash)
            .execute(&mut *tx)
            .await
            {
                tracing::warn!("[checkpoint_journal] write failed: {}", e);
            }
        }

        if let Err(e) = tx.commit().await {
            tracing::warn!("[checkpoint_journal] commit failed: {}", e);
        }
    }
}
