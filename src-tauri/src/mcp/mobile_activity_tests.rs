use super::{with_local_tool_activity, LocalToolActivity};

#[test]
fn test_mobile_activity_survives_duplicate_scope_and_cleans_up_on_drop() {
    let id = -831_001;
    let execution = LocalToolActivity::enter(id);
    let duplicate = LocalToolActivity::enter(id);
    assert!(with_local_tool_activity(|ids| ids.contains(&id)));
    drop(duplicate);
    assert!(with_local_tool_activity(|ids| ids.contains(&id)));
    drop(execution);
    assert!(!with_local_tool_activity(|ids| ids.contains(&id)));
}
