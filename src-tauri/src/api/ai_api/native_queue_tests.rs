use super::wait_for_interrupted_turn;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn test_native_queue_interrupt_waits_for_terminal_after_cancel_ack() {
    let (terminal, settled) = oneshot::channel();
    let acknowledged = AtomicBool::new(false);
    let stop = CancellationToken::new();
    let cancel = async { acknowledged.store(true, Ordering::SeqCst); Ok(()) };
    let mut waiting = Box::pin(wait_for_interrupted_turn(cancel, settled, &stop, "acp", "run1"));
    tokio::select! {
        biased;
        _ = &mut waiting => panic!("cancel acknowledgement started the next turn prematurely"),
        _ = std::future::ready(()) => {}
    }
    assert!(acknowledged.load(Ordering::SeqCst));
    terminal.send(()).unwrap();
    waiting.await.unwrap();
}

#[tokio::test]
async fn test_native_queue_stop_wins_over_a_delayed_terminal() {
    let (terminal, settled) = oneshot::channel();
    let stop = CancellationToken::new();
    terminal.send(()).unwrap();
    stop.cancel();
    let result = wait_for_interrupted_turn(async { panic!("must not cancel twice") }, settled, &stop, "cursor", "run2").await;
    assert!(result.unwrap_err().to_string().contains("用户已停止"));
}

#[tokio::test]
async fn test_native_queue_finished_turn_does_not_interrupt_next_prompt() {
    let (terminal, settled) = oneshot::channel();
    terminal.send(()).unwrap();
    wait_for_interrupted_turn(async { panic!("turn already settled") }, settled, &CancellationToken::new(), "claude_sdk", "run3").await.unwrap();
}
