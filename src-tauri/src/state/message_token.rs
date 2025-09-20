use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

pub struct MessageTokenManager {
    task_handles: Arc<Mutex<HashMap<i64, JoinHandle<Result<(), anyhow::Error>>>>>,
}

impl MessageTokenManager {
    pub fn new() -> Self {
        Self {
            task_handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn exist(&self, conversation_id: i64) -> bool {
        let map = self.task_handles.lock().await;
        map.contains_key(&conversation_id)
    }

    pub async fn store_task_handle(&self, conversation_id: i64, handle: JoinHandle<Result<(), anyhow::Error>>) {
        let mut map = self.task_handles.lock().await;
        map.insert(conversation_id, handle);
    }

    pub async fn cancel_request(&self, conversation_id: i64) {
        let mut task_handles = self.task_handles.lock().await;
        if let Some(handle) = task_handles.remove(&conversation_id) {
            handle.abort();
            debug!(conversation_id, "Successfully aborted conversation task");
        } else {
            warn!(conversation_id, "Attempted to abort non-existent conversation task");
        }
    }

    pub async fn remove_task_handle(&self, conversation_id: i64) {
        let mut task_handles = self.task_handles.lock().await;
        task_handles.remove(&conversation_id);
    }

    pub fn get_task_handles(&self) -> Arc<Mutex<HashMap<i64, JoinHandle<Result<(), anyhow::Error>>>>> {
        Arc::clone(&self.task_handles)
    }
}