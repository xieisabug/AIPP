use super::*;

#[tokio::test]
async fn test_scheduled_claim_released_when_future_is_aborted() {
    let state = SchedulerState::new();
    let claim = state.claim_scheduled_task(9).unwrap();
    assert!(state.claim_scheduled_task(9).is_none());
    let (started, ready) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        let _claim = claim;
        let _ = started.send(());
        std::future::pending::<()>().await;
    });
    ready.await.unwrap();
    handle.abort();
    let _ = handle.await;
    assert!(state.claim_scheduled_task(9).is_some());
}

#[tokio::test]
async fn test_scheduled_claim_released_on_panic() {
    let state = SchedulerState::new();
    let claim = state.claim_scheduled_task(10).unwrap();
    let handle = tokio::spawn(async move {
        let _claim = claim;
        panic!("test panic");
    });
    assert!(handle.await.unwrap_err().is_panic());
    assert!(state.claim_scheduled_task(10).is_some());
}
