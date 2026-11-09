use std::{future::Future, time::Duration};
use tokio::sync::watch;

tokio::task_local! { static HUMAN_WAIT: watch::Sender<bool>; }

pub(crate) struct HumanWaitGuard(Option<watch::Sender<bool>>);
impl Drop for HumanWaitGuard {
    fn drop(&mut self) { if let Some(sender) = &self.0 { sender.send_replace(false); } }
}
pub(crate) fn pause_for_human() -> HumanWaitGuard {
    HumanWaitGuard(HUMAN_WAIT.try_with(|sender| {
        sender.send_replace(true);
        sender.clone()
    }).ok())
}

pub(crate) async fn with_search_budget<F: Future>(budget: Duration, future: F) -> Result<F::Output, String> {
    let (sender, mut receiver) = watch::channel(false);
    let scoped = HUMAN_WAIT.scope(sender, future);
    tokio::pin!(scoped);
    let mut remaining = budget;
    loop {
        let paused = *receiver.borrow_and_update();
        let started = tokio::time::Instant::now();
        tokio::select! {
            biased;
            result = &mut scoped => return Ok(result),
            changed = receiver.changed() => {
                if changed.is_err() { return Err("Search human-wait notification channel closed".into()); }
            }
            _ = tokio::time::sleep(remaining), if !paused => {
                return Err(format!("Search active execution timed out after {}ms (human verification wait excluded)", budget.as_millis()));
            }
        }
        if !paused { remaining = remaining.saturating_sub(started.elapsed()); }
    }
}

#[cfg(test)]
#[path = "verification_budget_tests.rs"]
mod verification_budget_tests;
