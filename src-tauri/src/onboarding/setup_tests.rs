use super::*;

#[test]
fn test_onboarding_setup_preserves_failed_and_inflight_work_across_replay() {
    let db = SystemDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    persist(&db, |items| include_selected(items, &["uv".into(), "office".into()])).unwrap();
    persist(&db, |items| {
        items[0].state = State::Failed;
        items[0].message = "模拟断网".into();
        items[1].state = State::Running;
        Ok(())
    }).unwrap();
    let items = persist(&db, |items| include_selected(items, &["office".into(), "skills".into()])).unwrap();
    assert_eq!(items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["skills", "office", "uv"]);
    assert!(items[0].state == State::Pending && items[1].state == State::Failed && items[2].state == State::Running);
    assert!(persist(&db, |items| include_selected(items, &["unknown".into()])).is_err());
    assert_eq!(read(&db.conn).unwrap()[1].message, "模拟断网");
}

#[test]
fn test_onboarding_setup_fresh_selection_rechecks_completed_items_once_without_retrying_failed() {
    let db = SystemDatabase { conn: rusqlite::Connection::open_in_memory().unwrap() };
    db.create_tables().unwrap();
    let requested = ["skills","office","bun","uv"].map(str::to_owned);
    persist(&db, |items| {
        include_selected(items, &requested)?;
        for (item, state) in items.iter_mut().zip([State::Skipped,State::Done,State::Failed,State::Running]) {
            item.state = state;
        }
        Ok(())
    }).unwrap();
    let tx = db.conn.unchecked_transaction().unwrap();
    enqueue_selection(&tx, &requested, None).unwrap();
    tx.rollback().unwrap();
    assert!(read(&db.conn).unwrap()[0].state == State::Skipped);
    let tx = db.conn.unchecked_transaction().unwrap();
    enqueue_selection(&tx, &requested, None).unwrap();
    tx.commit().unwrap();
    let rows = read(&db.conn).unwrap();
    assert!(rows[0].state == State::Pending && rows[1].state == State::Pending);
    assert!(rows[2].state == State::Failed && rows[3].state == State::Running);
    let tx = db.conn.unchecked_transaction().unwrap();
    assert!(enqueue_selection(&tx, &requested, Some(&requested)).unwrap().is_none());
}

#[cfg(desktop)]
#[test]
fn test_onboarding_skills_selects_aipp_and_only_the_find_skills_directory() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!("preset.json")).unwrap();
    let catalog = ["anthropics-skills", "remotion-dev-skills", "taste-skill", "aipp-skills", "vercel-labs-skills-find-skills"]
        .iter().map(|id| serde_json::from_value(serde_json::json!({"id":id,"name":id})).unwrap()).collect();
    let [aipp, find] = onboarding_skill_packages(catalog, &fixture).unwrap();
    assert_eq!(aipp.id, "aipp-skills");
    assert_eq!(find.dirs.len(), 1);
    assert_eq!(find.dirs[0].from, "skills/find-skills");
    assert_eq!(find.dirs[0].to, "find-skills");
}
