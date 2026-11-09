use super::*;

#[tokio::test]
async fn test_human_verification_excludes_manual_wait_from_search_budget() {
    let value = with_search_budget(Duration::from_millis(100), async {
        let guard = pause_for_human();
        tokio::time::sleep(Duration::from_millis(250)).await;
        drop(guard);
        42
    }).await;
    assert_eq!(value.unwrap(), 42);
}

#[tokio::test]
async fn test_human_verification_resumes_timeout_after_manual_wait() {
    let value = with_search_budget(Duration::from_millis(100), async {
        { let _guard = pause_for_human(); tokio::time::sleep(Duration::from_millis(150)).await; }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }).await;
    assert!(value.is_err());
}

#[tokio::test]
async fn test_human_verification_does_not_pause_other_searches() {
    let (manual, ordinary) = tokio::join!(
        with_search_budget(Duration::from_millis(100), async {
            let _guard = pause_for_human();
            tokio::time::sleep(Duration::from_millis(250)).await;
        }),
        with_search_budget(Duration::from_millis(100), tokio::time::sleep(Duration::from_millis(250))),
    );
    assert!(manual.is_ok());
    assert!(ordinary.is_err());
}
