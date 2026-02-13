use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub struct MessageTokenManager {
    task_handles: Arc<Mutex<HashMap<i64, AbortHandle>>>,
    cancelled_conversations: Arc<Mutex<HashSet<i64>>>,
    cancel_tokens: Arc<Mutex<HashMap<i64, CancellationToken>>>,
}

impl MessageTokenManager {
    pub fn new() -> Self {
        Self {
            task_handles: Arc::new(Mutex::new(HashMap::new())),
            cancelled_conversations: Arc::new(Mutex::new(HashSet::new())),
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn exist(&self, conversation_id: i64) -> bool {
        let map = self.task_handles.lock().await;
        map.contains_key(&conversation_id)
    }

    pub async fn store_task_handle(
        &self,
        conversation_id: i64,
        handle: JoinHandle<Result<(), anyhow::Error>>,
    ) {
        self.store_abort_handle(conversation_id, handle.abort_handle()).await;
    }

    pub async fn store_abort_handle(&self, conversation_id: i64, handle: AbortHandle) {
        {
            let mut map = self.task_handles.lock().await;
            map.insert(conversation_id, handle);
        }
        {
            let mut tokens = self.cancel_tokens.lock().await;
            tokens.insert(conversation_id, CancellationToken::new());
        }
        {
            let mut cancelled = self.cancelled_conversations.lock().await;
            cancelled.remove(&conversation_id);
        }
    }

    pub async fn reset_cancel_token(&self, conversation_id: i64) -> CancellationToken {
        let token = CancellationToken::new();
        {
            let mut tokens = self.cancel_tokens.lock().await;
            tokens.insert(conversation_id, token.clone());
        }
        {
            let mut cancelled = self.cancelled_conversations.lock().await;
            cancelled.remove(&conversation_id);
        }
        token
    }

    pub async fn cancel_request(&self, conversation_id: i64) {
        {
            let mut cancelled = self.cancelled_conversations.lock().await;
            cancelled.insert(conversation_id);
        }
        {
            let mut task_handles = self.task_handles.lock().await;
            if let Some(handle) = task_handles.remove(&conversation_id) {
                handle.abort();
                debug!(conversation_id, "Successfully aborted conversation task");
            } else {
                warn!(conversation_id, "Attempted to abort non-existent conversation task");
            }
        }
        let token = {
            let mut tokens = self.cancel_tokens.lock().await;
            if let Some(token) = tokens.get(&conversation_id) {
                token.clone()
            } else {
                let token = CancellationToken::new();
                tokens.insert(conversation_id, token.clone());
                token
            }
        };
        token.cancel();
    }

    pub async fn remove_task_handle(&self, conversation_id: i64) {
        let mut task_handles = self.task_handles.lock().await;
        task_handles.remove(&conversation_id);
    }

    pub async fn is_cancelled(&self, conversation_id: i64) -> bool {
        let cancelled = self.cancelled_conversations.lock().await;
        cancelled.contains(&conversation_id)
    }

    pub async fn get_cancel_token(&self, conversation_id: i64) -> Option<CancellationToken> {
        let tokens = self.cancel_tokens.lock().await;
        tokens.get(&conversation_id).cloned()
    }

    pub fn get_task_handles(
        &self,
    ) -> Arc<Mutex<HashMap<i64, AbortHandle>>> {
        Arc::clone(&self.task_handles)
    }
}
