use super::*;

#[test]
fn test_simultaneous_starts_are_scoped_to_the_target_conversation() {
    let target = Destination::Existing { conversation: 55113 };
    let active = Starting::claim(&target).unwrap();
    assert!(Starting::claim(&target).is_err());
    assert!(Starting::claim(&Destination::Existing { conversation: 55114 }).is_ok());
    drop(active);
    assert!(Starting::claim(&target).is_ok());
}

#[tokio::test]
async fn test_local_send_and_other_phone_invalidate_previous_stop_owner() {
    let conversation = 55115;
    let turn = Uuid::new_v4();
    let scope = |device: &str, turn| Scope {
        device: device.into(),
        turn,
        destination: Destination::Existing { conversation },
    };
    REMOTE.scope(scope("phone-A", turn), begin_turn(conversation)).await;
    let slot = OWNERS.lock().unwrap().get(&conversation).unwrap().clone();
    assert_eq!(*slot.lock().await, Some(turn));
    let next = Uuid::new_v4();
    REMOTE.scope(scope("phone-B", next), begin_turn(conversation)).await;
    assert_eq!(*slot.lock().await, Some(next));
    drop(slot);
    begin_turn(conversation).await;
    assert!(!OWNERS.lock().unwrap().contains_key(&conversation));
}
