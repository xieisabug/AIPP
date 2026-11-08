use super::guarded_summary_request;
use crate::errors::AppError;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_summary_never_polls_provider_when_cancelled_or_paused() {
    for cancelled in [true, false] {
        let token = CancellationToken::new();
        if cancelled { token.cancel(); }
        let called = AtomicBool::new(false);
        let validate = || if cancelled { Ok(()) } else { Err(AppError::UnknownError("工作已暂停".into())) };
        let result = guarded_summary_request(&validate, Some(&token), async {
            called.store(true, Ordering::SeqCst);
            Ok(())
        }).await;
        assert!(result.is_err());
        assert!(!called.load(Ordering::SeqCst));
    }
}

#[tokio::test]
async fn test_summary_discards_result_when_paused_during_provider_call() {
    let paused = AtomicBool::new(false);
    let validate = || if paused.load(Ordering::SeqCst) {
        Err(AppError::UnknownError("工作已暂停".into()))
    } else { Ok(()) };
    let result = guarded_summary_request(&validate, None, async {
        paused.store(true, Ordering::SeqCst);
        Ok("obsolete summary")
    }).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_summary_cancellation_interrupts_pending_provider() {
    let token = CancellationToken::new();
    let result = guarded_summary_request(&|| Ok(()), Some(&token), async {
        token.cancel();
        std::future::pending::<Result<(), AppError>>().await
    }).await;
    assert!(result.is_err());
    assert_eq!(guarded_summary_request(&|| Ok(()), None, async { Ok("valid") }).await.unwrap(), "valid");
}
