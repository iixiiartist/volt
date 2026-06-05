use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    JobCompleted { job_id: uuid::Uuid, output: String },
    JobFailed { job_id: uuid::Uuid, error: String },
    MemoryWrite { path: String },
    ArtifactCreated { path: String, tool_name: String },
    ToolExecuted { tool_name: String, success: bool },
}

/// Lightweight event bus for reactive routines.
/// Uses tokio broadcast channels for decoupled publisher/subscriber.
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(256);
        Self { tx }
    }

    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event); // Drop silently if channel full
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
