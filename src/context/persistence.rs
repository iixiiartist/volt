use super::{ContextEntry, ContextKind, ContextStore, StoredEntry};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

impl ContextStore {
    pub fn set_db(&self, pool: sqlx::PgPool) {
        let _ = self.db.set(pool);
    }

    pub async fn seed_truncated_history_persistent(
        &self,
        session_id: &str,
        truncated_text: String,
        summary_hint: String,
        db_pool: &PgPool,
    ) -> anyhow::Result<()> {
        let entry = ContextEntry {
            id: Uuid::new_v4(),
            kind: ContextKind::Conversation,
            content: truncated_text,
            embedding: None,
            metadata: json!({
                "session_id": session_id,
                "summary": summary_hint,
                "archived_at": chrono::Utc::now().to_rfc3339()
            }),
            frequency: 0,
            success_rate: 0.0,
            usage_count: 0,
            last_used_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
        };

        {
            let mut entries_guard = self.entries.write().await;
            entries_guard.push(StoredEntry {
                entry: entry.clone(),
            });
        }

        crate::db::insert_context_entry(db_pool, &entry).await?;

        tracing::info!(
            session_id = %session_id,
            entry_id = %entry.id,
            "persisted L2 cold-history snapshot to pgvector"
        );
        Ok(())
    }

    pub async fn hydrate_from_db(&self, limit: i64) -> anyhow::Result<usize> {
        let db = self
            .db()
            .ok_or_else(|| anyhow::anyhow!("no database connection configured"))?;
        let entries = crate::db::load_context_entries(db, limit).await?;
        let count = entries.len();
        let mut store = self.entries.write().await;
        for entry in entries {
            store.push(StoredEntry { entry });
        }
        Ok(count)
    }
}
