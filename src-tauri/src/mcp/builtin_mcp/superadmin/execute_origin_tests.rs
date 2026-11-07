use super::schedule_origin_error;
#[test]
fn test_legacy_create_rejects_every_butler_origin() {
    for kind in ["butler_main", "butler_contact", "butler_task"] {
        assert!(schedule_origin_error("schedule.create", false, Some(kind)).is_some());
        assert!(schedule_origin_error("schedule.create", true, Some(kind)).is_none());
    }
}
#[test]
fn test_legacy_origin_guard_preserves_ordinary_schedule_and_other_actions() {
    assert!(schedule_origin_error("schedule.create", false, Some("normal")).is_none());
    assert!(schedule_origin_error("schedule.create", false, None).is_none());
    assert!(schedule_origin_error("schedule.list", false, Some("butler_main")).is_none());
}
