use super::admit_turn;
use std::sync::{Arc, atomic::{AtomicI64, Ordering}};

/// Both ordinary task-window sends and scoped cancellation use this same admission lock.
/// The cancellation ownership check must happen after an already-admitted new turn publishes.
#[tokio::test]
async fn test_task_admission_finishes_before_scoped_cancel_checks_input() {
    let task = 9_000_031;
    let admission = admit_turn(task).await;
    let current_input = Arc::new(AtomicI64::new(100));
    let input = current_input.clone();
    let (started_tx,started_rx) = tokio::sync::oneshot::channel();
    let (checked_tx,mut checked_rx) = tokio::sync::oneshot::channel();
    let cancellation = tokio::spawn(async move {
        started_tx.send(()).unwrap();
        let _cancel_admission = admit_turn(task).await;
        let still_owns_turn = input.load(Ordering::SeqCst) == 100;
        checked_tx.send(still_owns_turn).unwrap();
    });
    started_rx.await.unwrap();
    assert!(matches!(checked_rx.try_recv(),Err(tokio::sync::oneshot::error::TryRecvError::Empty)));
    current_input.store(101,Ordering::SeqCst);
    drop(admission);
    assert!(!checked_rx.await.unwrap(),"newer task-window turn must not be cancelled as the old followup");
    cancellation.await.unwrap();
}

#[tokio::test]
async fn test_scoped_cancel_holds_task_admission_until_halt_finishes() {
    let task = 9_000_032;
    let cancellation = admit_turn(task).await;
    let (started_tx,started_rx) = tokio::sync::oneshot::channel();
    let (published_tx,mut published_rx) = tokio::sync::oneshot::channel();
    let ordinary_send = tokio::spawn(async move {
        started_tx.send(()).unwrap();
        let _admission = admit_turn(task).await;
        published_tx.send(()).unwrap();
    });
    started_rx.await.unwrap();
    assert!(matches!(published_rx.try_recv(),Err(tokio::sync::oneshot::error::TryRecvError::Empty)));
    drop(cancellation);
    published_rx.await.unwrap();
    ordinary_send.await.unwrap();
}

#[test]
fn test_shared_peer_admission_allows_live_task_but_rejects_terminal_and_normal_chat() {
    use super::accepts_peer_turn;
    assert!(accepts_peer_turn("butler_task",Some("running"),false,false));
    assert!(accepts_peer_turn("butler_task",Some("accepted"),false,false));
    for status in ["cancelled","succeeded","failed","interrupted"] {
        assert!(!accepts_peer_turn("butler_task",Some(status),false,false));
    }
    assert!(!accepts_peer_turn("butler_task",Some("running"),true,false));
    assert!(!accepts_peer_turn("normal",None,false,false));
    assert!(!accepts_peer_turn("butler_main",None,false,false));
    assert!(accepts_peer_turn("butler_main",None,false,true));
    assert!(accepts_peer_turn("butler_contact",None,false,false));
}
